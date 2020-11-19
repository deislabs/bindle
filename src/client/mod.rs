//! Client implementation for consuming a Bindle API. Although written in Rust, it is not specific
//! to the Rust implementation. It is meant to consume any spec-compliant bindle implementation.
//! Also included are various filtering tools for selecting the correct parcels as specified by an
//! invoice

mod error;
pub mod load;

use std::convert::TryInto;
use std::path::Path;

use reqwest::header;
use reqwest::Client as HttpClient;
use url::Url;

pub use error::ClientError;

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Clone)]
pub struct Client {
    client: HttpClient,
    base_url: Url,
}

impl Client {
    /// Returns a new Client with the given URL. Will return an error if the URL is not valid
    pub fn new(base_url: &str) -> Result<Self> {
        let base_parsed = Url::parse(base_url)?;
        // TODO: As this evolves, we might want to allow for setting time outs and accepting
        // self-signed certs
        let client = HttpClient::builder()
            .http2_prior_knowledge()
            .build()
            .map_err(|e| ClientError::Other(e.to_string()))?;
        Ok(Client {
            client,
            base_url: base_parsed,
        })
    }

    /// Creates the given invoice, returns a response containing the created invoice and a list of
    /// missing parcels (that have not yet been uploaded)
    pub async fn create_invoice(
        &self,
        inv: crate::Invoice,
    ) -> Result<crate::InvoiceCreateResponse> {
        todo!()
    }

    /// Same as [`create_invoice`](Client::create_invoice), but takes a path to an invoice file
    /// instead.
    pub async fn create_invoice_from_file<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Result<crate::InvoiceCreateResponse> {
        // TODO: Do we want to just put the file in the body rather than loading?
        let inv: crate::Invoice = load::toml(file_path).await?;
        self.create_invoice(inv).await
    }

    /// Returns the requested invoice from the bindle server if it exists. This can take any form
    /// that can convert into the `Id` type, but generally speaking, this is the canonical name of
    /// the bindle (e.g. `example.com/foo/1.0.0`)
    pub async fn get_invoice<I: TryInto<crate::Id>>(&self, id: I) -> Result<crate::Invoice> {
        todo!()
    }

    /// Yanks the invoice from availability on the bindle server. This can take any form that can
    /// convert into the `Id` type, but generally speaking, this is the canonical name of the bindle
    /// (e.g. `example.com/foo/1.0.0`)
    pub async fn yank_invoice<I: TryInto<crate::Id>>(&self, id: I) -> Result<()> {
        todo!()
    }

    /// Creates the given parcel using the label and the raw parcel data to upload to the server.
    /// Returns the label of the created parcel
    pub async fn create_parcel(
        &self,
        label: crate::Label,
        data: impl AsRef<[u8]>,
    ) -> Result<crate::Label> {
        todo!()
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes paths to the label and parcel
    /// files. This will be more efficient for large files as it will stream the data into the body
    /// rather than taking the intermediate step of loading the bytes into a `Vec`
    pub async fn create_parcel_from_files<L, D>(
        &self,
        label_path: L,
        data_path: D,
    ) -> Result<crate::Label>
    where
        L: AsRef<Path>,
        D: AsRef<Path>,
    {
        // NOTE: This function doesn't call `create_parcel` as it is streaming the data rather than loading it all into memory
        todo!()
    }

    /// Returns the requested parcel (identified by its SHA) as a vector of bytes
    pub async fn get_parcel(&self, sha: &str) -> Result<Vec<u8>> {
        todo!()
    }

    /// Returns the requested parcel (identified by its SHA) as a stream of bytes. This is useful
    /// for when you don't want to read it into memory but are instead writing to a file or other
    /// location
    pub async fn get_parcel_stream(&self, sha: &str) -> Result<()> {
        // TODO: should return the proper stream type
        todo!()
    }
}
