//! Functions and types for reading and writing to standalone bindles
use std::collections::HashMap;
use std::convert::TryInto;
use std::path::{Path, PathBuf};

use log::debug;
use tokio::io::{AsyncRead, AsyncWriteExt};
use tokio::stream::{Stream, StreamExt};

use crate::client::{ClientError, Result};
use crate::Id;

/// The name of the invoice file
pub const INVOICE_FILE: &str = "invoice.toml";
/// The name of the parcels directory
pub const PARCEL_DIR: &str = "parcels/";

/// A struct containing paths to all of the key components of a standalone bundle
pub struct StandaloneRead {
    pub invoice_file: PathBuf,
    pub parcel_dir: PathBuf,
    pub parcels: Vec<PathBuf>,
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
    /// In the above example, the `StandaloneWrite` will be configured to read  bindle data from the
    /// `/foo/bar/187e908f466500c76c13953c3191fafa869c277e2689f451e92d75cda32452df` directory
    pub async fn new<P, I>(base_path: P, bindle_id: I) -> Result<StandaloneRead>
    where
        P: AsRef<Path>,
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let base = base_path
            .as_ref()
            .join(bindle_id.try_into().map_err(|e| e.into())?.sha());
        let invoice_file = base.join(INVOICE_FILE);
        let parcel_dir = base.join(PARCEL_DIR);
        let stream = tokio::fs::read_dir(&parcel_dir).await?;
        let parcels = stream
            .map(|res| res.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .await?;
        Ok(StandaloneRead {
            invoice_file,
            parcel_dir,
            parcels,
        })
    }

    // TODO: from a tarball
}

/// A type that can write all bindle data to the appropriate location
pub struct StandaloneWrite {
    base_path: PathBuf,
}

impl StandaloneWrite {
    /// Returns a new `StandaloneWrite` that can write all components of a bindle as a standalone
    /// bindle using the given base path and bindle ID
    ///
    /// ```
    /// use bindle::standalone::StandaloneWrite;
    ///
    /// StandaloneWrite::new("/foo/bar", "example.com/baz/1.0.0").unwrap();
    /// ```
    ///
    /// In the above example, the `StandaloneWrite` will be configured to write all the bindle data
    /// into the `/foo/bar/187e908f466500c76c13953c3191fafa869c277e2689f451e92d75cda32452df`
    /// directory
    pub fn new<P, I>(base_path: P, bindle_id: I) -> Result<StandaloneWrite>
    where
        P: AsRef<Path>,
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        Ok(StandaloneWrite {
            base_path: base_path
                .as_ref()
                .join(bindle_id.try_into().map_err(|e| e.into())?.sha()),
        })
    }

    // TODO: From a tarball

    /// Writes the given invoice and `HashMap` of parcel streams. The key of the `HashMap` should be the SHA of the parcel
    pub async fn write<T: AsyncRead + Unpin + Send + Sync>(
        &self,
        inv: crate::Invoice,
        parcels: HashMap<String, T>,
    ) -> anyhow::Result<()> {
        validate_shas(&inv, parcels.keys())?;

        tokio::fs::create_dir_all(&self.base_path).await?;

        // Write the invoice into the directory
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

            debug!("Writing parcel to {}", path.display());
            tokio::io::copy(&mut reader, &mut file).await?;
            file.flush().await?;
            debug!("Finished writing parcel to {}", path.display());
            Ok(())
        });
        futures::future::join_all(parcel_writes)
            .await
            .into_iter()
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(())
    }

    /// Writes the given invoice and collection of parcel streams
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

        tokio::fs::create_dir_all(&self.base_path).await?;

        write_invoice(&self.base_path, &inv).await?;

        let parcel_writes = parcels.into_iter().map(|(sha, mut stream)| async move {
            let path = self.base_path.join(PARCEL_DIR).join(format!("{}.dat", sha));

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // Make sure we aren't overwriting
                .open(&path)
                .await?;

            debug!("Writing parcel to {}", path.display());

            while let Some(b) = stream.next().await {
                let b =
                    b.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                file.write_all(&b).await?;
            }
            file.flush().await?;

            debug!("Finished writing parcel to {}", path.display());
            Ok(())
        });
        futures::future::join_all(parcel_writes)
            .await
            .into_iter()
            .collect::<std::io::Result<Vec<_>>>()?;

        Ok(())
    }
}

async fn write_invoice(base_path: impl AsRef<Path>, inv: &crate::Invoice) -> Result<()> {
    debug!("Writing invoice file into {}", base_path.as_ref().display());
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
    if offending_shas.is_empty() {
        Err(ClientError::Other(format!(
            "Got collection of parcels containing parcels that do not exist in the invoice: {}",
            offending_shas.join(", ")
        )))
    } else {
        Ok(())
    }
}
