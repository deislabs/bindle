//! A file system `Provider` implementation.
//!
//! The format on disk is
//! [documented](https://github.com/deislabs/bindle/blob/master/docs/file-layout.md) in the main
//! Bindle repo.
//!
//! This will only be available if the `provider` feature is enabled

use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::{convert::TryInto, ffi::OsString};

use ::lru::LruCache;
use sha2::{Digest, Sha256};
use tokio::fs::{create_dir_all, File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex as TokioMutex;
use tokio_stream::{Stream, StreamExt};
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument, trace, warn};
use tracing_futures::Instrument;

use crate::provider::{Provider, ProviderError, Result};
use crate::search::Search;
use crate::verification::Verified;
use crate::{Id, Signed};

/// The folder name for the invoices directory
const INVOICE_DIRECTORY: &str = "invoices";
/// The folder name for the parcels directory
pub const PARCEL_DIRECTORY: &str = "parcels";
const INVOICE_TOML: &str = "invoice.toml";
pub const PARCEL_DAT: &str = "parcel.dat";
const CACHE_SIZE: usize = 50;
const PART_EXTENSION: &str = "part";

/// A file system backend for storing and retrieving bindles and parcles.
///
/// Given a root directory, FileProvider brings its own storage layout for keeping track
/// of Bindles.
///
/// A FileProvider needs a search engine implementation. When invoices are created or yanked,
/// the index will be updated.
pub struct FileProvider<T> {
    root: PathBuf,
    index: T,
    invoice_cache: Arc<TokioMutex<LruCache<Id, crate::Invoice>>>,
}

impl<T: Clone> Clone for FileProvider<T> {
    fn clone(&self) -> Self {
        FileProvider {
            root: self.root.clone(),
            index: self.index.clone(),
            invoice_cache: Arc::clone(&self.invoice_cache),
        }
    }
}

impl<T: Search + Send + Sync> FileProvider<T> {
    pub async fn new<P: AsRef<Path>>(path: P, index: T) -> Self {
        debug!(path = %path.as_ref().display(), cache_size = CACHE_SIZE, "Creating new file provider");
        let fs = FileProvider {
            root: path.as_ref().to_owned(),
            index,
            invoice_cache: Arc::new(TokioMutex::new(LruCache::new(CACHE_SIZE))),
        };
        debug!("warming index");
        if let Err(e) = fs.warm_index().await {
            warn!(error = %e, "Error warming index");
        }
        fs
    }

    /// This warms the index by loading all of the invoices currently on disk.
    ///
    /// Warming the index is something that the storage backend should do, though I am
    /// not sure whether EVERY storage backend should do it. It is the responsibility of
    /// storage because storage is the sole authority about what documents are actually
    /// in the repository. So it needs to communicate (on startup) what documents it knows
    /// about. The storage engine merely needs to store any non-duplicates. So we can
    /// safely insert, but ignore errors that come back because of duplicate entries.
    #[instrument(level = "trace", skip(self))]
    async fn warm_index(&self) -> anyhow::Result<()> {
        // Read all invoices
        info!(path = %self.root.display(), "Beginning index warm");
        let mut total_indexed: u64 = 0;
        // Check if the invoice directory exists. If it doesn't, this is likely the first time and
        // we should just return
        let invoice_path = self.invoice_path("");
        match tokio::fs::metadata(&invoice_path).await {
            Ok(_) => (),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut readdir = tokio::fs::read_dir(invoice_path).await?;
        while let Some(e) = readdir.next_entry().await? {
            let p = e.path();
            let sha = match p.file_name().map(|f| f.to_string_lossy()) {
                Some(sha_opt) => sha_opt,
                None => continue,
            };
            // Load invoice
            let inv_path = self.invoice_toml_path(&sha);
            info!(path = %inv_path.display(), "Loading invoice into search index");
            // Open file
            let inv_toml = tokio::fs::read(inv_path).await?;

            // Parse
            let invoice: crate::Invoice = toml::from_slice(&inv_toml)?;
            let digest = invoice.canonical_name();
            if sha != digest {
                anyhow::bail!(
                    "SHA {} did not match computed digest {}. Delete this record.",
                    sha,
                    digest
                );
            }

            if let Err(e) = self.index.index(&invoice).await {
                error!(invoice_id = %invoice.bindle.id, error = %e, "Error indexing invoice");
            }
            total_indexed += 1;
        }
        debug!(total_indexed, "Warmed index");
        Ok(())
    }

    /// Return the path to the invoice directory for a particular bindle.
    fn invoice_path(&self, invoice_id: &str) -> PathBuf {
        let mut path = self.root.join(INVOICE_DIRECTORY);
        path.push(invoice_id);
        path
    }
    /// Return the path for an invoice.toml for a particular bindle.
    fn invoice_toml_path(&self, invoice_id: &str) -> PathBuf {
        self.invoice_path(invoice_id).join(INVOICE_TOML)
    }
    /// Return the parcel-specific path for storing a parcel.
    fn parcel_path(&self, parcel_id: &str) -> PathBuf {
        let mut path = self.root.join(PARCEL_DIRECTORY);
        path.push(parcel_id);
        path
    }
    /// Return the path to the parcel.dat file for the given box ID
    fn parcel_data_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_path(parcel_id).join(PARCEL_DAT)
    }
}

#[async_trait::async_trait]
impl<T: crate::search::Search + Send + Sync> Provider for FileProvider<T> {
    #[instrument(level = "trace", skip(self, invoice), fields(invoice_id = tracing::field::Empty))]
    async fn create_invoice<I>(&self, invoice: I) -> Result<(crate::Invoice, Vec<crate::Label>)>
    where
        I: Signed + Verified + Send + Sync,
    {
        let inv = invoice.signed();
        tracing::span::Span::current()
            .record("invoice_id", &tracing::field::display(&inv.bindle.id));
        // It is illegal to create a yanked invoice.
        if inv.yanked.unwrap_or(false) {
            debug!(id = %inv.bindle.id, "Invoice being created is set to yanked");
            return Err(ProviderError::CreateYanked);
        }

        let invoice_id = inv.canonical_name();

        // Create the base path if necessary
        let inv_path = self.invoice_path(&invoice_id);
        trace!(path = %inv_path.display(), "Checking if invoice base path already exists on disk");
        // With errors we default to false in this logic block, so just convert to an Option
        let metadata = tokio::fs::metadata(&inv_path).await.ok();
        if !metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            // If it exists and is a regular file, we have a problem
            if metadata.map(|m| m.is_file()).unwrap_or(false) {
                debug!("Invoice being created already exists in storage");
                return Err(ProviderError::Exists);
            }
            trace!(path = %inv_path.display(), "Base path doesn't exist, creating");
            if let Err(e) = create_dir_all(inv_path).await {
                error!(error = %e, "Unable to create invoice storage directory");
                return Err(e.into());
            }
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(&invoice_id);
        trace!(path = %dest.display(), "Checking if invoice already exists on disk");
        // We can't just call `exists` because it can do IO calls, so look up using the metadata
        if tokio::fs::metadata(&dest)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            debug!("Invoice being created already exists in storage");
            return Err(ProviderError::Exists);
        }

        // Create the part file to indicate that we are currently writing
        let mut part = PartFile::new(dest).await?;
        part.write_invoice(&inv).await?;
        part.finalize().await?;

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv).await {
            error!(error = %e, "Error indexing new invoice");
        }

        // if there are no parcels, bail early
        if inv.parcel.is_none() {
            return Ok((inv, Vec::with_capacity(0)));
        }

        trace!("Checking for missing parcels listed in newly created invoice");
        // Note: this will not allocate
        let zero_vec = Vec::with_capacity(0);
        // Loop through the boxes and see what exists
        let missing = inv
            .parcel
            .as_ref()
            .unwrap_or(&zero_vec)
            .iter()
            .map(|k| async move {
                let parcel_path = self.parcel_path(k.label.sha256.as_str());
                // Stat k to see if it exists. If it does not exist or is not a directory, add it.
                let res = tokio::fs::metadata(parcel_path).await;
                match res {
                    Ok(stat) if !stat.is_dir() => Some(k.label.clone()),
                    Err(_e) => Some(k.label.clone()),
                    _ => None,
                }
            });

        let labels = futures::future::join_all(missing)
            .instrument(tracing::trace_span!("lookup_missing"))
            .await
            .into_iter()
            .flatten()
            .collect();
        Ok((inv, labels))
    }

    #[instrument(level = "trace", skip(self, id), fields(id))]
    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id: Id = id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));

        if let Some(inv) = self.invoice_cache.lock().await.get(&parsed_id) {
            debug!("Found invoice in cache, returning");
            return Ok(inv.clone());
        }
        debug!("Getting invoice from file system");

        let invoice_id = parsed_id.sha();

        // Now construct a path and read it
        let invoice_path = self.invoice_toml_path(&invoice_id);

        debug!(
            path = %invoice_path.display(),
            "Reading invoice"
        );
        // Open file
        let inv_toml = tokio::fs::read(invoice_path).await.map_err(map_io_error)?;

        // Parse
        trace!("Parsing invoice from raw TOML data");
        let invoice: crate::Invoice = toml::from_slice(&inv_toml)?;

        // Put it into the cache
        trace!("Putting invoice into cache");
        self.invoice_cache
            .lock()
            .await
            .put(parsed_id, invoice.clone());

        // Return object
        Ok(invoice)
    }

    #[instrument(level = "trace", skip(self, id), fields(id))]
    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        trace!("Fetching invoice from storage");
        let mut inv = self.get_yanked_invoice(&parsed_id).await?;
        inv.yanked = Some(true);

        debug!("Yanking invoice");

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        trace!("Indexing yanked invoice");
        if let Err(e) = self.index.index(&inv).await {
            error!(error = %e, "Error indexing yanked invoice");
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(&inv.canonical_name());

        // Encode the invoice into a TOML object
        trace!("Encoding invoice to TOML");
        let data = toml::to_vec(&inv)?;
        // NOTE: Right now, this just force-overwites the existing invoice. We are assuming
        // that the bindle has already been confirmed to be present. However, we have not
        // ensured that here. So it is theoretically possible (if get_invoice was not used
        // to build the invoice) that this could _create_ a new file. We could probably change
        // this behavior with OpenOptions.
        debug!(path = %dest.display(), "Writing yanked invoice to disk");
        tokio::fs::write(dest, data).await?;

        // Drop the invoice from the cache (as it is unlikely that someone will want to fetch it
        // right after yanking it)
        trace!("Dropping yanked invoice from cache");
        self.invoice_cache.lock().await.pop(&parsed_id);
        Ok(())
    }

    #[instrument(level = "trace", skip(self, bindle_id, data), fields(id))]
    async fn create_parcel<I, R, B>(&self, bindle_id: I, parcel_id: &str, data: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf + Send,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        let label = self.validate_parcel(parsed_id, parcel_id).await?;

        // Test if a dir with that SHA exists. If so, this is an error.
        let par_path = self.parcel_path(parcel_id);
        if tokio::fs::metadata(&par_path)
            .await
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            debug!(path = %par_path.display(), "Parcel directory already exists");
            return Err(ProviderError::Exists);
        }
        // Create box dir
        trace!(path = %par_path.display(), "Creating parcel directory");
        if let Err(e) = create_dir_all(par_path).await {
            error!(error = %e, "Unable to create parcel storage directory");
            return Err(e.into());
        }

        // Write data
        let mut part = PartFile::new(self.parcel_data_path(parcel_id)).await?;
        part.write_parcel(data, parcel_id, label.size).await?;
        part.finalize().await
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(id))]
    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        self.validate_parcel(parsed_id, parcel_id).await?;

        let name = self.parcel_data_path(parcel_id);
        debug!(path = %name.display(), "Getting parcel from storage");
        let reader = File::open(name).await.map_err(map_io_error)?;
        Ok::<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>, _>(Box::new(
            FramedRead::new(reader, BytesCodec::new())
                .map(|res| res.map_err(map_io_error).map(|b| b.freeze())),
        ))
    }

    #[instrument(level = "trace", skip(self, bindle_id), fields(id))]
    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        debug!("Validating bindle -> parcel relationship");
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::Span::current().record("id", &tracing::field::display(&parsed_id));
        self.validate_parcel(parsed_id, parcel_id).await?;

        let label_path = self.parcel_data_path(parcel_id);
        debug!(path = %label_path.display(), "Checking if parcel exists in storage");
        match tokio::fs::metadata(label_path).await {
            Ok(m) => Ok(m.is_file()),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

fn map_io_error(e: std::io::Error) -> ProviderError {
    if matches!(e.kind(), std::io::ErrorKind::NotFound) {
        return ProviderError::NotFound;
    }
    ProviderError::from(e)
}

/// An internal wrapper to implement `AsyncWrite` on Sha256
pub(crate) struct AsyncSha256 {
    inner: Mutex<Sha256>,
}

impl AsyncSha256 {
    /// Equivalent to the `Sha256::new()` function
    pub(crate) fn new() -> Self {
        AsyncSha256 {
            inner: Mutex::new(Sha256::new()),
        }
    }

    /// Consumes self and returns the bare Sha256. This should only be called once you are done
    /// writing. This will only return an error if for some reason the underlying mutex was poisoned
    pub(crate) fn into_inner(self) -> std::sync::LockResult<Sha256> {
        self.inner.into_inner()
    }
}

impl tokio::io::AsyncWrite for AsyncSha256 {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        // Because the hasher is all in memory, we only need to make sure only one caller at a time
        // can write using the mutex
        let mut inner = match self.inner.try_lock() {
            Ok(l) => l,
            Err(_) => return Poll::Pending,
        };

        Poll::Ready(inner.write(buf))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        let mut inner = match self.inner.try_lock() {
            Ok(l) => l,
            Err(_) => return Poll::Pending,
        };

        Poll::Ready(inner.flush())
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        // There are no actual shutdown tasks to perform, so just flush things as defined in the
        // trait documentation
        self.poll_flush(cx)
    }
}

/// Validate that the File path matches the given SHA256
async fn validate_sha256(file: &mut File, sha: &str) -> Result<()> {
    let mut hasher = AsyncSha256::new();
    tokio::io::copy(file, &mut hasher).await?;
    let hasher = match hasher.into_inner() {
        Ok(h) => h,
        Err(_) => {
            return Err(ProviderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "data write corruption, mutex poisoned",
            )))
        }
    };
    let result = hasher.finalize();

    if format!("{:x}", result) != sha {
        return Err(ProviderError::DigestMismatch);
    }

    Ok(())
}

/// A helper struct for a part file that will clean up the file on drop if it still exists. Also
/// contains functionality for writing to the file and finalizing it (i.e moving it to the correct
/// location)
struct PartFile {
    path: PathBuf,
    final_location: PathBuf,
    file: File,
}

impl PartFile {
    /// Creates a new PartFile that will eventually be located at the given `final_location`. This
    /// will attempt to create a new part file and return an error if one already exists
    async fn new(final_location: PathBuf) -> Result<Self> {
        let extension = match final_location.extension() {
            Some(s) => {
                let mut ext = s.to_owned();
                ext.push(".");
                ext.push(PART_EXTENSION);
                ext
            }
            None => OsString::from(PART_EXTENSION),
        };
        let part = final_location.with_extension(extension);
        trace!(path = %part.display(), "Checking that a write is not currently in progress");
        // Make sure we aren't already writing
        if tokio::fs::metadata(&part)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(ProviderError::WriteInProgress);
        }
        #[cfg(target_family = "windows")]
        let file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            // 4 is the value for FILE_SHARE_DELETE so we don't have to import the winapi constants.
            // We need this so we can delete the file on drop. See
            // https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea
            .share_mode(4)
            .open(&part)
            .await
        {
            Ok(f) => f,
            // This is the error code for a sharing violation, which will happen if there is a write
            // in progress
            Err(e) if e.raw_os_error().unwrap_or_default() == 32 => {
                return Err(ProviderError::WriteInProgress)
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        #[cfg(target_family = "unix")]
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(&part)
            .await?;
        Ok(PartFile {
            path: part,
            final_location,
            file,
        })
    }

    async fn write_invoice(&mut self, inv: &crate::Invoice) -> Result<()> {
        debug!(
            path = %self.path.display(),
            "Storing invoice in part file"
        );

        trace!("Encoding invoice to TOML");
        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        self.file
            .write_all(data.as_slice())
            .await
            .map_err(|e| e.into())
    }

    async fn write_parcel<R, B>(
        &mut self,
        data: R,
        parcel_id: &str,
        expected_length: u64,
    ) -> Result<()>
    where
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf + Send,
    {
        // Create the part file to indicate that we are currently writing
        debug!(
            path = %self.path.display(),
            parcel_id,
            "Storing parcel data in part file"
        );
        trace!("Copying data to open file");
        let written = tokio::io::copy(
            &mut StreamReader::new(
                data.map(|res| res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
            ),
            &mut self.file,
        )
        .instrument(tracing::trace_span!("parcel_data_write"))
        .await?;

        // Make sure the right amount of data was sent
        trace!(bytes_written = written, "Wrote data to part file");
        if written != expected_length {
            return Err(ProviderError::SizeMismatch);
        }
        // Verify parcel by rewinding the parcel and then hashing it.
        // This MUST be after the last write to out, otherwise the results will
        // not be correct.
        self.file.flush().await?;
        self.file.seek(std::io::SeekFrom::Start(0)).await?;
        trace!("Validating data for parcel");
        validate_sha256(&mut self.file, parcel_id)
            .instrument(tracing::trace_span!("parcel_data_validation"))
            .await?;
        trace!("SHA data validated");
        Ok(())
    }

    /// Moves the file to the configured final location, consuming the part file
    async fn finalize(mut self) -> Result<()> {
        debug!(
            renamed_path = %self.final_location.display(),
            "Renaming part file for parcel"
        );

        // Close the file handle to avoid any problems with unfinished IO operations
        self.file.shutdown().await?;

        tokio::fs::rename(&self.path, &self.final_location)
            .await
            .map_err(|e| e.into())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        // Attempt to delete the file, logging an error if the delete failed
        if let Err(e) = std::fs::remove_file(&self.path) {
            // If it is any other error besides NotFound, log the error
            if !matches!(e.kind(), std::io::ErrorKind::NotFound) {
                error!(error = %e, "Unable to clean up part file, this could lead to conflicts");
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::verification::NoopVerified;
    use crate::{testing, NoopSigned};
    use tempfile::tempdir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_should_generate_paths() {
        let f = FileProvider::new("test", crate::search::StrictEngine::default()).await;
        assert_eq!(PathBuf::from("test/invoices/123"), f.invoice_path("123"));
        assert_eq!(
            PathBuf::from("test/invoices/123/invoice.toml"),
            f.invoice_toml_path("123")
        );
        assert_eq!(PathBuf::from("test/parcels/123"), f.parcel_path("123"));
        assert_eq!(
            PathBuf::from("test/parcels/123/parcel.dat"),
            f.parcel_data_path("123")
        );
    }

    #[tokio::test]
    async fn test_should_create_yank_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let scaffold = testing::Scaffold::load("valid_v1").await;
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;
        let inv_name = scaffold.invoice.canonical_name();

        let signed = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        // Create an invoice
        let (_, missing) = store.create_invoice(signed).await.unwrap();
        assert_eq!(1, missing.len());

        // Out-of-band read the invoice
        assert!(store.invoice_toml_path(&inv_name).exists());

        // Yank the invoice
        store
            .yank_invoice(&scaffold.invoice.bindle.id)
            .await
            .unwrap();

        // Make sure the invoice is yanked
        let inv2 = store
            .get_yanked_invoice(&scaffold.invoice.bindle.id)
            .await
            .unwrap();
        assert!(inv2.yanked.unwrap_or(false));

        // Sanity check that this produces an error
        assert!(store.get_invoice(scaffold.invoice.bindle.id).await.is_err());
    }

    #[tokio::test]
    async fn test_should_reject_yanked_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let mut scaffold = testing::Scaffold::load("valid_v1").await;
        scaffold.invoice.yanked = Some(true);
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;

        let signed = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        assert!(store.create_invoice(signed).await.is_err());
    }

    #[tokio::test]
    async fn test_should_write_read_parcel() {
        let scaffold = testing::Scaffold::load("valid_v1").await;
        let parcel = scaffold.parcel_files.get("parcel").unwrap();
        let root = tempdir().expect("create tempdir");
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;

        let signed = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        // Create the invoice so we can create a parcel
        store
            .create_invoice(signed)
            .await
            .expect("should be able to create invoice");

        store
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data.clone()), BytesCodec::new()),
            )
            .await
            .expect("create parcel");

        // Now make sure the parcels reads as existing
        assert!(
            store
                .parcel_exists(&scaffold.invoice.bindle.id, &parcel.sha)
                .await
                .expect("Shouldn't get an error while checking for parcel existence"),
            "Parcel should be reported as existing"
        );

        // Attempt to read the parcel from the store
        let mut data = Vec::new();
        let stream = store
            .get_parcel(&scaffold.invoice.bindle.id, &parcel.sha)
            .await
            .expect("load parcel data");
        let mut reader = StreamReader::new(
            stream.map(|res| res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
        );
        reader
            .read_to_end(&mut data)
            .await
            .expect("read file into string");
        assert_eq!(data, parcel.data);
    }

    #[tokio::test]
    async fn test_should_store_and_retrieve_bindle() {
        let root = tempdir().expect("create tempdir");
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;

        let scaffold = testing::Scaffold::load("valid_v1").await;

        let signed = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        // Store an invoice first and then create the parcel for it
        store
            .create_invoice(signed)
            .await
            .expect("should be able to create an invoice");

        let parcel = scaffold.parcel_files.get("parcel").unwrap();

        store
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data.clone()), BytesCodec::new()),
            )
            .await
            .expect("unable to store the parcel");

        // Get the bindle
        let inv = store
            .get_invoice(&scaffold.invoice.bindle.id)
            .await
            .expect("get the invoice we just stored");

        let first_parcel = inv
            .parcel
            .expect("parsel vector")
            .pop()
            .expect("got a parcel");
        assert_eq!(first_parcel.label.sha256, parcel.sha)
    }

    #[tokio::test]
    async fn test_should_reject_invalid_length() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let mut scaffold = testing::Scaffold::load("valid_v1").await;
        let mut parcels = scaffold.invoice.parcel.take().unwrap();
        // Completely invalid size
        parcels[0].label.size = 100000;
        scaffold.invoice.parcel = Some(parcels);
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;

        let signed = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        store
            .create_invoice(signed)
            .await
            .expect("Invoice should be created");

        let parcel = scaffold.parcel_files.get("parcel").unwrap();
        let err = store
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data.clone()), BytesCodec::new()),
            )
            .await
            .expect_err("Creating a parcel with invalid length should fail");
        assert!(
            matches!(err, ProviderError::SizeMismatch),
            "Error should be of type SizeMismatch"
        )
    }

    // Running this as multi thread to make sure both processes run simultaneously
    #[tokio::test(flavor = "multi_thread")]
    async fn test_double_write() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let scaffold = testing::Scaffold::load("valid_v1").await;
        let store = FileProvider::new(root.path(), crate::search::StrictEngine::default()).await;

        // We want two copies to try and write at the same time
        let signed1 = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        let signed2 = NoopSigned(NoopVerified(scaffold.invoice.clone()));
        let (first, second) =
            tokio::join!(store.create_invoice(signed1), store.create_invoice(signed2));

        // At least one should fail
        assert!(
            !(first.is_ok() && second.is_ok()),
            "One of the create invoice tasks should fail"
        );
        // At least one should succeed
        assert!(
            first.is_ok() || second.is_ok(),
            "One of the create invoice tasks should succeed"
        );

        // Now for sanity sake, try the same thing with the parcel
        let parcel = scaffold.parcel_files.get("parcel").unwrap();
        let (firstp, secondp) = tokio::join!(
            store.create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data.clone()), BytesCodec::new()),
            ),
            store.create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data.clone()), BytesCodec::new()),
            ),
        );

        // At least one should fail
        assert!(
            !(firstp.is_ok() && secondp.is_ok()),
            "One of the create parcel tasks should fail"
        );
        // At least one should succeed
        assert!(
            firstp.is_ok() || secondp.is_ok(),
            "One of the create parcel tasks should succeed"
        );
    }
}
