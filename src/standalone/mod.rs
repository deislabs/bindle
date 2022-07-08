//! Functions and types for reading and writing to standalone bindles
use std::collections::HashMap;
use std::convert::TryInto;
use std::path::{Path, PathBuf};

use async_compression::tokio::{bufread::GzipDecoder, write::GzipEncoder};
use futures::{future, stream, TryStreamExt};
use tokio::fs::{read_dir, File};
use tokio::io::{AsyncRead, AsyncWriteExt};
use tokio_stream::{Stream, StreamExt};
use tokio_tar::Archive;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{debug, info, instrument, trace};

use crate::client::{tokens::TokenManager, Client, ClientError, Result};
use crate::Id;

/// Maximum number of assets to upload in parallel
const MAX_PARALLEL_UPLOADS: usize = 16;

/// The name of the invoice file
pub const INVOICE_FILE: &str = "invoice.toml";
/// The name of the parcels directory
pub use crate::provider::file::{PARCEL_DAT, PARCEL_DIRECTORY as PARCEL_DIR};

/// A struct containing paths to all of the key components of a standalone bindle
pub struct StandaloneRead {
    pub invoice_file: PathBuf,
    pub parcel_dir: PathBuf,
    pub parcels: Vec<PathBuf>,
    // We keep the tarball tempdir in scope so it cleans up on drop
    #[allow(dead_code)]
    tarball_dir: Option<tempfile::TempDir>,
}

impl StandaloneRead {
    /// Returns a new StandaloneRead constructed using the given base path and bindle ID. It will
    /// attempt to list all parcel files, but will not provide any validation such as whether it is
    /// a regular file.
    ///
    /// ```no_run
    /// use bindle::standalone::StandaloneRead;
    /// # #[tokio::main]
    /// # async fn main() {
    /// StandaloneRead::new("/foo/bar", "example.com/baz/1.0.0").await.unwrap();
    /// # }
    /// ```
    ///
    /// In the above example, the `StandaloneWrite` will be configured to read bindle data from the
    /// `/foo/bar/187e908f466500c76c13953c3191fafa869c277e2689f451e92d75cda32452df` directory
    #[instrument(level = "trace", skip(base_path, bindle_id))]
    pub async fn new<P, I>(base_path: P, bindle_id: I) -> Result<StandaloneRead>
    where
        P: AsRef<Path>,
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        parse_dir(
            base_path
                .as_ref()
                .join(bindle_id.try_into().map_err(|e| e.into())?.sha()),
            None,
        )
        .await
    }

    /// Returns a new StandaloneRead constructed using the given tarball.
    ///
    /// This will extract the tarball to a temporary location on disk. It will attempt to list all
    /// parcel files, but will not provide any validation such as whether it is a regular file.
    ///
    /// ```no_run
    /// use bindle::standalone::StandaloneRead;
    /// # #[tokio::main]
    /// # async fn main() {
    /// StandaloneRead::new_from_tarball("/foo/bar.tar.gz").await.unwrap();
    /// # }
    /// ```
    #[instrument(level = "trace", skip(tarball), fields(%path = tarball.as_ref().display()))]
    pub async fn new_from_tarball<P: AsRef<Path>>(tarball: P) -> Result<StandaloneRead> {
        let file = File::open(tarball).await?;

        let tempdir = tokio::task::spawn_blocking(tempfile::TempDir::new)
            .await
            .map_err(|e| {
                ClientError::Other(format!("Thread error when creating tempdir: {}", e))
            })??;

        let mut archive = Archive::new(GzipDecoder::new(tokio::io::BufReader::new(file)));

        trace!("Unpacking tarball");
        archive.unpack(tempdir.path()).await?;
        trace!("Tarball unpacked, parsing directory");

        // Get the path to the newly unpacked directory
        let mut readdir = read_dir(tempdir.path()).await?;
        // There should only be one entry, the expanded directory
        let entry = readdir.next_entry().await?.ok_or_else(|| {
            ClientError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Unable to find unpacked directory",
            ))
        })?;
        if !entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false) {
            return Err(ClientError::Other(format!(
                "Found entry in temp directory that is not the expected directory: {}",
                entry.file_name().to_string_lossy()
            )));
        }

        parse_dir(entry.path(), Some(tempdir)).await
    }

    /// Push this standalone bindle to a bindle server using the given client. This function will
    /// automatically handle cases where the invoice or some of the parcels already exist on the
    /// target bindle server
    #[instrument(level = "trace", skip(self, client))]
    pub async fn push<T: TokenManager + Clone>(&self, client: &Client<T>) -> Result<()> {
        let inv_create = create_or_get_invoice(client, &self.invoice_file).await?;
        let missing = inv_create.missing.unwrap_or_default();
        let inv = inv_create.invoice;
        let to_upload: Vec<(String, PathBuf)> = self
            .parcels
            .iter()
            .filter_map(|path| {
                let sha = match path.file_stem() {
                    Some(s) => s.to_string_lossy().to_string(),
                    None => return None,
                };
                Some((sha, path))
            })
            .filter_map(|(sha, path)| {
                if let Some(label) = missing.iter().find(|label| label.sha256 == sha) {
                    Some((label.sha256.clone(), path.clone()))
                } else {
                    info!(%sha, "Parcel not in missing parcels, skipping...");
                    None
                }
            })
            .collect();

        debug!(
            num_parcels = to_upload.len(),
            "Found parcels in this bindle that do not yet exist on the server"
        );

        let parcel_futures = to_upload
            .into_iter()
            .map(|(sha, path)| (sha, path, inv.bindle.id.clone(), (*client).clone()))
            .map(|(sha, path, bindle_id, client)| async move {
                debug!(%sha, "Uploading parcel to server");
                client
                    .create_parcel_from_file(bindle_id, &sha, path)
                    .await?;
                debug!(%sha, "Finished uploading parcel to server");
                Ok(())
            });

        futures::StreamExt::buffer_unordered(stream::iter(parcel_futures), MAX_PARALLEL_UPLOADS)
            .try_for_each(future::ok)
            .await
    }

    /// Retrieve the invoice from the standalone bindle.
    ///
    /// This loads the invoice from disk and then parse it into an invoice.
    /// Errors can result either from reading from disk or from parsing the TOML
    pub async fn get_invoice(&self) -> Result<crate::invoice::Invoice> {
        let data = tokio::fs::read(&self.invoice_file)
            .await
            .map_err(ClientError::Io)?;
        let inv = toml::from_slice(&data)?;
        Ok(inv)
    }

    /// Get the path to a parcel in a standalone bindle
    pub fn parcel_data_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_dir.join(format!("{}.dat", parcel_id))
    }

    /// Read the parcel off of the filesystem and return the data.
    pub async fn get_parcel(&self, parcel_id: &str) -> Result<Vec<u8>> {
        let local_path = self.parcel_data_path(parcel_id);
        tokio::fs::read(local_path).await.map_err(ClientError::Io)
    }

    /// This fetches a parcel from within a standalone bindle.
    ///
    /// Note that while the general API checks for invoice membership, this API does not
    /// because we know that if the parcel is included in the standalone, it is by
    /// definition a member of the bindle.
    pub async fn get_parcel_stream(
        &self,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>> {
        // Get the parcel from the array of parcels.
        let local_path = self.parcel_data_path(parcel_id);
        let reader = File::open(local_path).await.map_err(ClientError::Io)?;
        Ok::<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>, _>(Box::new(
            FramedRead::new(reader, BytesCodec::new())
                .map(|res| res.map_err(ClientError::Io).map(|b| b.freeze())),
        ))
    }
}

/// Helper function for creating an invoice or fetching it if it already exists. For security
/// reasons, we need to fetch the invoice (and its missing parcels) if it already exists as the user
/// submitted one could be incorrect (intentionally or unintentionally)
async fn create_or_get_invoice<T: TokenManager>(
    client: &Client<T>,
    invoice_path: &Path,
) -> Result<crate::InvoiceCreateResponse> {
    // Load the invoice into memory so we can have access to its ID for fetching if needed
    trace!(path = %invoice_path.display(), "Loading invoice file from disk");
    let inv: crate::Invoice = crate::client::load::toml(invoice_path).await?;
    let id = inv.bindle.id.clone();
    debug!(invoice_id = %id, "Attempting to create invoice");
    match client.create_invoice(inv).await {
        Ok(resp) => {
            debug!(invoice_id = %id, "Invoice created");
            Ok(resp)
        }
        Err(e) if matches!(e, crate::client::ClientError::InvoiceAlreadyExists) => {
            info!(invoice_id = %id, "Invoice already exists on the bindle server. Fetching existing invoice and missing parcels list");
            let invoice = client.get_invoice(&id).await?;
            let missing = client.get_missing_parcels(id).await?;
            let missing = if missing.is_empty() {
                None
            } else {
                Some(missing)
            };
            Ok(crate::InvoiceCreateResponse {
                invoice: invoice.into(),
                missing,
            })
        }
        Err(e) => Err(e),
    }
}

/// Helper function for parsing the directory and returning a StandaloneRead
async fn parse_dir<P: AsRef<Path>>(
    base_path: P,
    tarball_dir: Option<tempfile::TempDir>,
) -> Result<StandaloneRead> {
    let base = tokio::fs::canonicalize(base_path).await?;
    let invoice_file = base.join(INVOICE_FILE);
    trace!(invoice_path = %invoice_file.display(), "Computed invoice file path");
    let parcel_dir = base.join(PARCEL_DIR);
    trace!(parcels_dir = %parcel_dir.display(), "Listing parcels in parcels directory");
    let stream = read_dir(&parcel_dir).await?;
    let parcels = tokio_stream::wrappers::ReadDirStream::new(stream)
        .map(|res| res.map(|entry| entry.path()).map_err(|e| e.into()))
        .collect::<Result<_>>()
        .await?;
    Ok(StandaloneRead {
        invoice_file,
        parcel_dir,
        parcels,
        tarball_dir,
    })
}

/// A type that can write all bindle data to the configured location as a standalone bindle
pub struct StandaloneWrite {
    base_path: PathBuf,
}

impl StandaloneWrite {
    /// Returns a new `StandaloneWrite` that can write all components of a bindle as a standalone
    /// bindle using the given base path and bindle ID
    ///
    /// ```no_run
    /// use bindle::standalone::StandaloneWrite;
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// StandaloneWrite::new("/foo/bar", "example.com/baz/1.0.0").await.unwrap();
    /// # }
    /// ```
    ///
    /// In the above example, the `StandaloneWrite` will be configured to write all the bindle data
    /// into the `/foo/bar/187e908f466500c76c13953c3191fafa869c277e2689f451e92d75cda32452df`
    /// directory
    pub async fn new<P, I>(base_path: P, bindle_id: I) -> Result<StandaloneWrite>
    where
        P: AsRef<Path>,
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let path = base_path
            .as_ref()
            .join(bindle_id.try_into().map_err(|e| e.into())?.sha());

        trace!(path = %path.display(), "Ensuring that directory exists");
        tokio::fs::create_dir_all(&path).await?;

        Ok(StandaloneWrite {
            base_path: tokio::fs::canonicalize(path).await?,
        })
    }

    /// Returns a reference to the output directory for this standalone bindle
    pub fn path(&self) -> &Path {
        self.base_path.as_ref()
    }

    /// Creates a tarball, consuming the `StandaloneWrite` and outputting the tarball at the given
    /// path
    #[instrument(level = "debug", skip(self, output_dir), fields(output_dir = %output_dir.as_ref().display()))]
    pub async fn tarball(self, output_dir: impl AsRef<Path>) -> Result<()> {
        // First, check if there is anything in the directory, if it is empty, then we abort
        if read_dir(&self.base_path)
            .await?
            .next_entry()
            .await?
            .is_none()
        {
            return Err(ClientError::Other(
                "Standalone bindle directory is empty. Unable to create tarball".to_string(),
            ));
        }
        // We know the directory name will be the bindle ID, so grab it from there
        // NOTE: The unwrap here is ok as we have canonicalized the path and created
        let mut filename = self.base_path.file_name().unwrap().to_owned();
        filename.push(".tar.gz");
        let file = File::create(output_dir.as_ref().join(filename)).await?;
        let encoder = GzipEncoder::new(file);

        let mut builder = tokio_tar::Builder::new(encoder);
        builder
            .append_dir_all(self.base_path.file_name().unwrap(), &self.base_path)
            .await?;

        // Make sure everything is flushed to disk, otherwise we might miss closing data block
        let mut encoder = builder.into_inner().await?;
        encoder.flush().await?;
        encoder.shutdown().await?;

        Ok(())
    }

    /// Writes the given invoice and `HashMap` of parcels (as readers). The key of the `HashMap`
    /// should be the SHA of the parcel
    ///
    /// The `HashMap` key should be the sha256 of the data, and the value should be the parcel content.
    #[instrument(level = "trace", skip(self, inv, parcels), fields(invoice_id = %inv.bindle.id, num_parcels = parcels.len(), base_dir = %self.base_path.display()))]
    pub async fn write<T: AsyncRead + Unpin + Send + Sync>(
        &self,
        inv: crate::Invoice,
        parcels: HashMap<String, T>,
    ) -> Result<()> {
        validate_shas(&inv, parcels.keys())?;

        // Should create all the directories needed down to the parcels directory
        trace!("Creating necessary subdirectories");
        tokio::fs::create_dir_all(self.base_path.join(PARCEL_DIR)).await?;

        // Write the invoice into the directory
        trace!("Writing invoice");
        write_invoice(&self.base_path, &inv).await?;

        // TODO(thomastaylor312): we might be able to dedup this and the work done in the other
        // function, but I don't want to mess with an async FnMut constraint right now
        let parcel_writes = parcels.into_iter().map(|(sha, mut reader)| async move {
            let path = self.base_path.join(PARCEL_DIR).join(format!("{}.dat", sha));

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // Make sure we aren't overwriting
                .open(&path)
                .await?;

            trace!(path = %path.display(), "Writing parcel");
            tokio::io::copy(&mut reader, &mut file).await?;
            file.flush().await?;
            trace!(path = %path.display(), "Finished writing parcel");
            Ok(())
        });
        futures::future::join_all(parcel_writes)
            .await
            .into_iter()
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(())
    }

    /// Writes the given invoice and collection of parcel streams
    #[instrument(level = "trace", skip(self, inv, parcels), fields(invoice_id = %inv.bindle.id, num_parcels = parcels.len(), base_dir = %self.base_path.display()))]
    pub async fn write_stream<E, T>(
        &self,
        inv: crate::Invoice,
        parcels: HashMap<String, T>,
    ) -> Result<()>
    where
        E: std::error::Error,
        T: Stream<Item = std::result::Result<bytes::Bytes, E>> + Unpin,
    {
        validate_shas(&inv, parcels.keys())?;

        // Should create all the directories needed down to the parcels directory
        trace!("Creating necessary subdirectories");
        tokio::fs::create_dir_all(self.base_path.join(PARCEL_DIR)).await?;

        trace!("Writing invoice");
        write_invoice(&self.base_path, &inv).await?;

        let parcel_writes = parcels.into_iter().map(|(sha, mut stream)| async move {
            let path = self.base_path.join(PARCEL_DIR).join(format!("{}.dat", sha));

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // Make sure we aren't overwriting
                .open(&path)
                .await?;

            trace!(path = %path.display(), "Writing parcel");

            while let Some(b) = stream.next().await {
                let b =
                    b.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                file.write_all(&b).await?;
            }
            file.flush().await?;

            trace!(path = %path.display(), "Finished writing parcel");
            Ok(())
        });
        futures::future::join_all(parcel_writes)
            .await
            .into_iter()
            .collect::<std::io::Result<Vec<_>>>()?;

        Ok(())
    }
}

#[instrument(level = "trace", skip(base_path, inv), fields(invoice_id = %inv.bindle.id, outfile = %base_path.as_ref().display()))]
async fn write_invoice(base_path: impl AsRef<Path>, inv: &crate::Invoice) -> Result<()> {
    debug!("Writing invoice file");
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // Make sure we aren't overwriting
        .open(base_path.as_ref().join(INVOICE_FILE))
        .await?
        .write_all(&toml::to_vec(inv)?)
        .await?;

    debug!("Invoice file written");
    Ok(())
}

/// Validates all shas in the hashmap to make sure they exist in the invoice. Returns an error
/// containing a list of the offending SHAs that aren't found in the invoice
#[instrument(level = "trace", skip(inv, parcels), fields(invoice_id = %inv.bindle.id))]
fn validate_shas<'a, T: Iterator<Item = &'a String>>(
    inv: &crate::Invoice,
    parcels: T,
) -> Result<()> {
    let zero_vec = Vec::with_capacity(0);
    let offending_shas: Vec<String> = parcels
        .filter(|s| {
            !inv.parcel
                .as_ref()
                .unwrap_or(&zero_vec)
                .iter()
                .any(|p| &p.label.sha256 == *s)
        })
        .cloned()
        .collect();
    if !offending_shas.is_empty() {
        Err(ClientError::Other(format!(
            "Got collection of parcels containing parcels that do not exist in the invoice: {}",
            offending_shas.join(", ")
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;
    use tokio_stream::StreamExt;

    use crate::{
        standalone::{StandaloneRead, StandaloneWrite},
        BindleSpec, Id, Invoice, Label, Parcel,
    };

    #[tokio::test]
    async fn should_round_trip() {
        // Create temp dir
        let dir = tempdir().expect("create a temp dir");
        // Create invoice
        let id: Id = "standalone/roundtrip/1.0.0"
            .parse()
            .expect("expect valid ID");
        let mut inv = Invoice::new(BindleSpec {
            id: id.clone(),
            description: Some("testing standalone bindle".to_owned()),
            authors: None,
        });

        // Create parcel
        let parcel_data = "I'm a test fixture".as_bytes();
        let sha = Sha256::digest(parcel_data);
        let sha_string = format!("{:x}", sha);

        // Add parcel
        inv.parcel = Some(vec![Parcel {
            label: Label {
                name: "fixture.txt".to_owned(),
                media_type: "text/plain".to_owned(),
                size: parcel_data.len() as u64,
                sha256: sha_string.clone(),
                annotations: None,
                origin: None,
                feature: None,
            },
            conditions: None,
        }]);
        let mut parcels = HashMap::new();
        parcels.insert(sha_string.clone(), parcel_data);

        // Save
        let writer = StandaloneWrite::new(&dir.path(), &id)
            .await
            .expect("Create a writer");
        writer
            .write(inv, parcels)
            .await
            .expect("Write parcel to disk");

        let reader = StandaloneRead::new(dir.path(), "standalone/roundtrip/1.0.0")
            .await
            .expect("construct a reader");

        {
            let md = tokio::fs::metadata(&reader.invoice_file)
                .await
                .expect("stat invoice path");
            assert!(md.is_file());
        }

        // Verify that the parcel directory exists
        {
            let md = tokio::fs::metadata(&reader.parcel_dir)
                .await
                .expect("stat parcel path");
            assert!(md.is_dir());
        }

        // Verify that the parcel exists
        for p in reader.parcels.iter() {
            tokio::fs::metadata(p)
                .await
                .unwrap_or_else(|e| panic!("failed to find {}: {}", p.display(), e));
        }

        // Load invoice
        let inv2 = reader.get_invoice().await.expect("load invoice");
        assert_eq!(
            "standalone/roundtrip/1.0.0".to_string(),
            inv2.bindle.id.to_string()
        );

        // Load parcel
        let parcel_data2 = reader
            .get_parcel(sha_string.as_str())
            .await
            .expect("load parcel data");

        assert_eq!(parcel_data, &parcel_data2);

        // Load the parcel from a stream
        let mut parcel_stream = reader
            .get_parcel_stream(sha_string.as_str())
            .await
            .expect("got the parcel stream");

        let parcel_data3 = parcel_stream
            .next()
            .await
            .expect("at least one parcel in the stream")
            .expect("successfully loaded the parcel");

        assert_eq!(parcel_data, parcel_data3);

        // This keeps dir from being deleted until we are done with the test.
        // Otherwise, tmpfile will clean up the tmpdir too soon.
        dir.close().expect("deleted temp dir");
    }

    #[tokio::test]
    async fn should_round_trip_tarball() {
        // Create temp dir
        let dir = tempdir().expect("create a temp dir");
        // Create invoice
        let id: Id = "standalone/roundtrip/1.0.0"
            .parse()
            .expect("expect valid ID");
        let mut inv = Invoice::new(BindleSpec {
            id: id.clone(),
            description: Some("testing standalone bindle".to_owned()),
            authors: None,
        });

        // Create parcel
        let parcel_data = "I'm a test fixture".as_bytes();
        let sha = Sha256::digest(parcel_data);
        let sha_string = format!("{:x}", sha);

        // Add parcel
        inv.parcel = Some(vec![Parcel {
            label: Label {
                name: "fixture.txt".to_owned(),
                media_type: "text/plain".to_owned(),
                size: parcel_data.len() as u64,
                sha256: sha_string.clone(),
                annotations: None,
                origin: None,
                feature: None,
            },
            conditions: None,
        }]);
        let mut parcels = HashMap::new();
        parcels.insert(sha_string.clone(), parcel_data);

        // Save
        let writer = StandaloneWrite::new(&dir.path(), &id)
            .await
            .expect("Create a writer");
        writer
            .write(inv, parcels)
            .await
            .expect("Write parcel to disk");

        // Tarball it
        let output_dir = tempdir().expect("unable to create tempdir");
        writer
            .tarball(output_dir.path())
            .await
            .expect("Tarball should write to disk");

        // Check that the tarball exists with the given name
        let tarball_path = output_dir.path().join(format!("{}.tar.gz", id.sha()));
        {
            let md = tokio::fs::metadata(&tarball_path)
                .await
                .expect("stat tarball path");
            assert!(md.is_file());
        }

        // Now attempt to read it
        let reader = StandaloneRead::new_from_tarball(&tarball_path)
            .await
            .expect("construct a reader");

        {
            let md = tokio::fs::metadata(&reader.invoice_file)
                .await
                .expect("stat invoice path");
            assert!(md.is_file());
        }

        // Verify that the parcel directory exists
        {
            let md = tokio::fs::metadata(&reader.parcel_dir)
                .await
                .expect("stat parcel path");
            assert!(md.is_dir());
        }

        // Verify that the parcel exists
        for p in reader.parcels.iter() {
            tokio::fs::metadata(p)
                .await
                .unwrap_or_else(|e| panic!("failed to find {}: {}", p.display(), e));
        }

        // This keeps dir from being deleted until we are done with the test.
        // Otherwise, tmpfile will clean up the tmpdir too soon.
        dir.close().expect("deleted temp dir");
        output_dir.close().expect("deleted temp output dir");
    }
}
