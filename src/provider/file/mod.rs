//! A file system `Storage` implementation. The format on disk is
//! [documented](https://github.com/deislabs/bindle/blob/master/docs/file-layout.md) in the main
//! Bindle repo.

use std::convert::TryInto;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use log::{debug, error, trace};
use sha2::{Digest, Sha256};
use tokio::fs::{create_dir_all, File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::stream::{Stream, StreamExt};
use tokio_util::codec::{BytesCodec, FramedRead};

use crate::provider::{Provider, ProviderError, Result};
use crate::Id;
use crate::{async_util, search::Search};

/// The folder name for the invoices directory
const INVOICE_DIRECTORY: &str = "invoices";
/// The folder name for the parcels directory
const PARCEL_DIRECTORY: &str = "parcels";
const INVOICE_TOML: &str = "invoice.toml";
const PARCEL_DAT: &str = "parcel.dat";

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
}

impl<T: Clone> Clone for FileProvider<T> {
    fn clone(&self) -> Self {
        FileProvider {
            root: self.root.clone(),
            index: self.index.clone(),
        }
    }
}

impl<T: Search + Send + Sync> FileProvider<T> {
    pub async fn new<P: AsRef<Path>>(path: P, index: T) -> Self {
        let fs = FileProvider {
            root: path.as_ref().to_owned(),
            index,
        };
        if let Err(e) = fs.warm_index().await {
            log::error!("Error warming index: {}", e);
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
    async fn warm_index(&self) -> anyhow::Result<()> {
        // Read all invoices
        debug!("Beginning index warm from {}", self.root.display());
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
        while let Some(e) = readdir.next().await {
            let p = match e {
                Ok(path) => path.path(),
                Err(e) => {
                    error!("Error while reading directory entry: {:?}", e);
                    continue;
                }
            };
            let sha = match p.file_name().map(|f| f.to_string_lossy()) {
                Some(sha_opt) => sha_opt,
                None => continue,
            };
            log::info!("Loading invoice {}/invoice.toml into search index", sha);
            // Load invoice
            let inv_path = self.invoice_toml_path(&sha);
            // Open file
            let inv_toml = std::fs::read_to_string(inv_path)?;

            // Parse
            let invoice: crate::Invoice = toml::from_str(inv_toml.as_str())?;
            let digest = invoice.canonical_name();
            if sha != digest {
                return Err(anyhow::anyhow!(
                    "SHA {} did not match computed digest {}. Delete this record.",
                    sha,
                    digest
                ));
            }

            if let Err(e) = self.index.index(&invoice).await {
                log::error!("Error indexing {}: {}", sha, e);
            }
            total_indexed += 1;
        }
        trace!("Warmed index with {} entries", total_indexed);
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
    async fn create_invoice(&self, inv: &crate::Invoice) -> Result<Vec<crate::Label>> {
        // It is illegal to create a yanked invoice.
        if inv.yanked.unwrap_or(false) {
            return Err(ProviderError::CreateYanked);
        }

        let invoice_id = inv.canonical_name();

        // Create the base path if necessary
        let inv_path = self.invoice_path(&invoice_id);
        if !inv_path.is_dir() {
            // If it exists and is a regular file, we have a problem
            if inv_path.is_file() {
                return Err(ProviderError::Exists);
            }
            create_dir_all(inv_path).await?;
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(&invoice_id);
        if dest.exists() {
            return Err(ProviderError::Exists);
        }
        debug!(
            "Storing invoice with ID {:?} in {}",
            inv.bindle.id,
            dest.display()
        );
        let mut out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(dest)
            .await?;

        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        out.write_all(data.as_slice()).await?;

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv).await {
            log::error!("Error indexing {:?}: {}", inv.bindle.id, e);
        }

        // if there are no parcels, bail early
        if inv.parcel.is_none() {
            return Ok(Vec::with_capacity(0));
        }

        trace!(
            "Checking for missing parcels listed in newly created invoice {:?}",
            inv.bindle.id
        );
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

        Ok(futures::future::join_all(missing)
            .await
            .into_iter()
            .filter_map(|x| x)
            .collect())
    }

    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id: Id = id.try_into().map_err(|e| e.into())?;
        trace!("Getting invoice {:?}", parsed_id);

        let invoice_id = parsed_id.sha();

        // Now construct a path and read it
        let invoice_path = self.invoice_toml_path(&invoice_id);

        trace!(
            "Reading invoice {:?} from {}",
            parsed_id,
            invoice_path.display()
        );
        // Open file
        let inv_toml = tokio::fs::read_to_string(invoice_path)
            .await
            .map_err(map_io_error)?;

        // Parse
        let invoice: crate::Invoice = toml::from_str(&inv_toml)?;

        // Return object
        Ok(invoice)
    }

    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let mut inv = self.get_yanked_invoice(id).await?;
        inv.yanked = Some(true);

        let invoice_id = inv.canonical_name();
        trace!("Yanking invoice {:?}", invoice_id);

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv).await {
            log::error!("Error indexing {}: {}", invoice_id, e);
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(&invoice_id);

        // Encode the invoice into a TOML object
        let data = toml::to_vec(&inv)?;
        // NOTE: Right now, this just force-overwites the existing invoice. We are assuming
        // that the bindle has already been confirmed to be present. However, we have not
        // ensured that here. So it is theoretically possible (if get_invoice was not used)
        // to build the invoice) that this could _create_ a new file. We could probably change
        // this behavior with OpenOptions.

        tokio::fs::write(dest, data).await?;
        Ok(())
    }

    async fn create_parcel<R, B>(&self, parcel_id: &str, data: &mut R) -> Result<()>
    where
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync,
        B: bytes::Buf,
    {
        debug!("Creating parcel with SHA {}", parcel_id);

        // Test if a dir with that SHA exists. If so, this is an error.
        let par_path = self.parcel_path(parcel_id);
        if par_path.is_dir() {
            return Err(ProviderError::Exists);
        }
        // Create box dir
        create_dir_all(par_path).await?;

        // Write data
        {
            let data_file = self.parcel_data_path(parcel_id);
            trace!(
                "Writing parcel data for SHA {} at {}",
                parcel_id,
                data_file.display()
            );
            let mut out = OpenOptions::new()
                // TODO: Right now, if we write the file and then sha validation fails, the user
                // will not be able to create this parcel again because the file already exists
                .create_new(true)
                .write(true)
                .read(true)
                .open(data_file.clone())
                .await?;

            tokio::io::copy(&mut async_util::BodyReadBuffer(data), &mut out).await?;
            // Verify parcel by rewinding the parcel and then hashing it.
            // This MUST be after the last write to out, otherwise the results will
            // not be correct.
            out.flush();
            out.seek(std::io::SeekFrom::Start(0)).await?;
            trace!("Validating data for SHA {}", parcel_id);
            validate_sha256(&mut out, parcel_id).await?;
            trace!("SHA {} data validated", parcel_id);
            // TODO: Should we also validate length? We use it for returning the proper content length
        }

        Ok(())
    }

    async fn get_parcel<I>(
        &self,
        _bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Because this is a "terminal provider" implementation (i.e. we aren't forwarding it anywhere), the
        // bindle ID doesn't matter in this case
        debug!("Getting parcel with SHA {}", parcel_id);
        let name = self.parcel_data_path(parcel_id);
        let reader = File::open(name).await.map_err(map_io_error)?;
        Ok(Box::new(
            FramedRead::new(reader, BytesCodec::new())
                .map(|res| res.map_err(map_io_error).map(|b| b.freeze())),
        ))
    }

    async fn parcel_exists<I>(&self, _bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        debug!("Checking if parcel sha {} exists", parcel_id);
        let label_path = self.parcel_data_path(parcel_id);
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::provider::test_common::*;
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
        let inv = invoice_fixture();
        let store = FileProvider::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;
        let inv_name = inv.canonical_name();
        // Create an file
        let missing = store.create_invoice(&inv).await.unwrap();
        assert_eq!(3, missing.len());

        // Out-of-band read the invoice
        assert!(store.invoice_toml_path(&inv_name).exists());

        // Yank the invoice
        store.yank_invoice(&inv.bindle.id).await.unwrap();

        // Make sure the invoice is yanked
        let inv2 = store.get_yanked_invoice(inv.name()).await.unwrap();
        assert!(inv2.yanked.unwrap_or(false));

        // Sanity check that this produces an error
        assert!(store.get_invoice(inv.bindle.id).await.is_err());

        // Drop the temporary directory
        assert!(root.close().is_ok());
    }

    #[tokio::test]
    async fn test_should_reject_yanked_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let mut inv = invoice_fixture();
        inv.yanked = Some(true);
        let store = FileProvider::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;
        // Create an file
        assert!(store.create_invoice(&inv).await.is_err());
        assert!(root.close().is_ok());
    }

    #[tokio::test]
    async fn test_should_write_read_parcel() {
        let content = "abcdef1234567890987654321";
        let (label, data) = parcel_fixture(content).await;
        let id = label.sha256.as_str();
        let root = tempdir().expect("create tempdir");
        let store = FileProvider::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;

        store
            .create_parcel(id, &mut FramedRead::new(data, BytesCodec::new()))
            .await
            .expect("create parcel");

        // Now make sure the parcels reads as existing
        assert!(
            store
                .parcel_exists("doesn't matter", id)
                .await
                .expect("Shouldn't get an error while checking for parcel existence"),
            "Parcel should be reported as existing"
        );

        // Attempt to read the parcel from the store
        let mut data = String::new();
        let stream = store
            .get_parcel("doesn't matter", id)
            .await
            .expect("load parcel data");
        let mut reader = crate::async_util::BodyReadBuffer(stream);
        reader
            .read_to_string(&mut data)
            .await
            .expect("read file into string");
        assert_eq!(data, content);
    }

    #[tokio::test]
    async fn test_should_store_and_retrieve_bindle() {
        let root = tempdir().expect("create tempdir");
        let store = FileProvider::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;

        // Store a parcel
        let content = "abcdef1234567890987654321";
        let (label, data) = parcel_fixture(content).await;
        let mut invoice = invoice_fixture();

        let parcel = crate::Parcel {
            label: label.clone(),
            conditions: None,
        };
        invoice.parcel = Some(vec![parcel]);

        store
            .create_parcel(&label.sha256, &mut FramedRead::new(data, BytesCodec::new()))
            .await
            .expect("stored the parcel");

        // Store an invoice that points to that parcel

        store.create_invoice(&invoice).await.expect("create parcel");

        // Get the bindle
        let inv = store
            .get_invoice(invoice.bindle.id)
            .await
            .expect("get the invoice we just stored");

        let first_parcel = inv
            .parcel
            .expect("parsel vector")
            .pop()
            .expect("got a parcel");
        assert_eq!(first_parcel.label.name, "foo.toml".to_owned())
    }
}
