use std::convert::TryInto;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use log::{debug, error, trace};
use sha2::{Digest, Sha256};
use tokio::fs::{create_dir_all, File, OpenOptions};
use tokio::io::{AsyncRead, AsyncWriteExt};

use crate::search::Search;
use crate::storage::{Result, Storage, StorageError};
use crate::Id;

/// The folder name for the invoices directory
const INVOICE_DIRECTORY: &str = "invoices";
/// The folder name for the parcels directory
const PARCEL_DIRECTORY: &str = "parcels";
const INVOICE_TOML: &str = "invoice.toml";
const PARCEL_DAT: &str = "parcel.dat";
const LABEL_TOML: &str = "label.toml";

/// A file system backend for storing and retriving bindles and parcles.
///
/// Given a root directory, FileStorage brings its own storage layout for keeping track
/// of Bindles.
///
/// A FileStorage needs a search engine implementation. When invoices are created or yanked,
/// the index will be updated.
pub struct FileStorage<T> {
    root: PathBuf,
    index: T,
}

impl<T: Clone> Clone for FileStorage<T> {
    fn clone(&self) -> Self {
        FileStorage {
            root: self.root.clone(),
            index: self.index.clone(),
        }
    }
}

impl<T: Search + Send + Sync> FileStorage<T> {
    pub async fn new<P: AsRef<Path>>(path: P, index: T) -> Self {
        let fs = FileStorage {
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
        for e in self.invoice_path("").read_dir()? {
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
    /// Return the path to a parcel.toml for a specific parcel.
    fn label_toml_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_path(parcel_id).join(LABEL_TOML)
    }
    /// Return the path to the parcel.dat file for the given box ID
    fn parcel_data_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_path(parcel_id).join(PARCEL_DAT)
    }
}

#[async_trait::async_trait]
impl<T: crate::search::Search + Send + Sync> Storage for FileStorage<T> {
    async fn create_invoice(&self, inv: &crate::Invoice) -> Result<Vec<crate::Label>> {
        // It is illegal to create a yanked invoice.
        if inv.yanked.unwrap_or(false) {
            return Err(StorageError::CreateYanked);
        }

        let invoice_id = inv.canonical_name();

        // Create the base path if necessary
        let inv_path = self.invoice_path(&invoice_id);
        if !inv_path.is_dir() {
            // If it exists and is a regular file, we have a problem
            if inv_path.is_file() {
                return Err(StorageError::Exists);
            }
            create_dir_all(inv_path).await?;
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(&invoice_id);
        if dest.exists() {
            return Err(StorageError::Exists);
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
        I::Error: Into<StorageError>,
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
        I::Error: Into<StorageError>,
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

    async fn create_parcel<R: AsyncRead + Unpin + Send + Sync>(
        &self,
        label: &crate::Label,
        data: &mut R,
    ) -> Result<()> {
        let sha = label.sha256.as_str();
        debug!("Creating parcel with SHA {}", sha);

        // Test if a dir with that SHA exists. If so, this is an error.
        let par_path = self.parcel_path(sha);
        if par_path.is_dir() {
            return Err(StorageError::Exists);
        }
        // Create box dir
        create_dir_all(par_path).await?;

        // Write data
        {
            let data_file = self.parcel_data_path(sha);
            trace!(
                "Writing parcel data for SHA {} at {}",
                sha,
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

            tokio::io::copy(data, &mut out).await?;
            // Verify parcel by rewinding the parcel and then hashing it.
            // This MUST be after the last write to out, otherwise the results will
            // not be correct.
            out.flush();
            out.seek(std::io::SeekFrom::Start(0)).await?;
            trace!("Validating data for SHA {}", sha);
            validate_sha256(&mut out, label.sha256.as_str()).await?;
            trace!("SHA {} data validated", sha);
        }

        // Write label
        {
            trace!("Writing label for parcel SHA {}", sha);
            let dest = self.label_toml_path(sha);
            let mut out = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(dest)
                .await?;

            let data = toml::to_vec(label)?;
            out.write_all(data.as_slice()).await?;
        }
        Ok(())
    }

    async fn get_parcel(&self, parcel_id: &str) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        debug!("Getting parcel with SHA {}", parcel_id);
        let name = self.parcel_data_path(parcel_id);
        let reader = File::open(name).await.map_err(map_io_error)?;
        Ok(Box::new(reader))
    }

    async fn get_label(&self, parcel_id: &str) -> Result<crate::Label> {
        debug!("Getting label for parcel sha {}", parcel_id);
        let label_path = self.label_toml_path(parcel_id);
        let label_toml = tokio::fs::read_to_string(label_path).await?;
        let label: crate::Label = toml::from_str(label_toml.as_str())?;

        // Return object
        Ok(label)
    }
}

fn map_io_error(e: std::io::Error) -> StorageError {
    if matches!(e.kind(), std::io::ErrorKind::NotFound) {
        return StorageError::NotFound;
    }
    return StorageError::from(e);
}

/// An internal wrapper to implement `AsyncWrite` on Sha256
struct AsyncSha256 {
    inner: Mutex<Sha256>,
}

impl AsyncSha256 {
    /// Equivalent to the `Sha256::new()` function
    fn new() -> Self {
        AsyncSha256 {
            inner: Mutex::new(Sha256::new()),
        }
    }

    /// Consumes self and returns the bare Sha256. This should only be called once you are done
    /// writing. This will only return an error if for some reason the underlying mutex was poisoned
    fn into_inner(self) -> std::sync::LockResult<Sha256> {
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
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "data write corruption, mutex poisoned",
            )))
        }
    };
    let result = hasher.finalize();

    if format!("{:x}", result) != sha {
        return Err(StorageError::DigestMismatch);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::storage::test_common::*;
    use tempfile::tempdir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_should_generate_paths() {
        let f = FileStorage::new("test", crate::search::StrictEngine::default()).await;
        assert_eq!("test/invoices/123", f.invoice_path("123").to_string_lossy());
        assert_eq!(
            "test/invoices/123/invoice.toml",
            f.invoice_toml_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/parcels/123".to_owned(),
            f.parcel_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/parcels/123/label.toml".to_owned(),
            f.label_toml_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/parcels/123/parcel.dat".to_owned(),
            f.parcel_data_path("123").to_string_lossy()
        );
    }

    #[tokio::test]
    async fn test_should_create_yank_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let inv = invoice_fixture();
        let store = FileStorage::new(
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
        let store = FileStorage::new(
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
        let (label, mut data) = parcel_fixture(content).await;
        let id = label.sha256.as_str();
        let root = tempdir().expect("create tempdir");
        let store = FileStorage::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;

        store
            .create_parcel(&label, &mut data)
            .await
            .expect("create parcel");

        // Now attempt to read just the label

        let label2 = store.get_label(id).await.expect("fetch label after saving");
        let mut data = String::new();
        store
            .get_parcel(&label2.sha256)
            .await
            .expect("load parcel data")
            .read_to_string(&mut data)
            .await
            .expect("read file into string");
        assert_eq!(data, content);
    }

    #[tokio::test]
    async fn test_should_store_and_retrieve_bindle() {
        let root = tempdir().expect("create tempdir");
        let store = FileStorage::new(
            root.path().to_owned(),
            crate::search::StrictEngine::default(),
        )
        .await;

        // Store a parcel
        let content = "abcdef1234567890987654321";
        let (label, mut data) = parcel_fixture(content).await;
        let mut invoice = invoice_fixture();

        let parcel = crate::Parcel {
            label: label.clone(),
            conditions: None,
        };
        invoice.parcel = Some(vec![parcel]);

        store
            .create_parcel(&label, &mut data)
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
