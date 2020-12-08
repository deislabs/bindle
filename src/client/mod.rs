//! Client implementation for consuming a Bindle API. Although written in Rust, it is not specific
//! to the Rust implementation. It is meant to consume any spec-compliant bindle implementation.

mod error;
pub mod load;

use std::convert::TryInto;
use std::path::Path;

use log::{debug, info};
use reqwest::header;
use reqwest::multipart::{Form, Part};
use reqwest::Client as HttpClient;
use reqwest::{Body, RequestBuilder, StatusCode};
use tokio::stream::{Stream, StreamExt};
use url::Url;

use crate::Id;

pub use error::ClientError;

pub type Result<T> = std::result::Result<T, ClientError>;

const INVOICE_ENDPOINT: &str = "_i";
const PARCEL_ENDPOINT: &str = "_p";
const QUERY_ENDPOINT: &str = "_q";
const RELATIONSHIP_ENDPOINT: &str = "_r";
const TOML_MIME_TYPE: &str = "application/toml";

#[derive(Clone)]
pub struct Client {
    client: HttpClient,
    base_url: Url,
}

impl Client {
    /// Returns a new Client with the given URL. This URL should be the FQDN plus any namespacing
    /// (like `v1`). So if you were running a bindle server mounted at the v1 endpoint, your URL
    /// would look something like `http://my.bindle.com/v1/`. Will return an error if the URL is not
    /// valid
    pub fn new(base_url: &str) -> Result<Self> {
        // Note that the trailing slash is important, otherwise the URL parser will treat is as a
        // "file" component of the URL. So we need to check that it is added before parsing
        let mut base = base_url.to_owned();
        if !base.ends_with('/') {
            info!("Provided base URL missing trailing slash, adding...");
            base.push('/');
        }
        let base_parsed = Url::parse(&base)?;
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ACCEPT, "application/toml".parse().unwrap());
        // TODO: As this evolves, we might want to allow for setting time outs and accepting
        // self-signed certs
        let client = HttpClient::builder()
            .http2_prior_knowledge()
            .default_headers(headers)
            .build()
            .map_err(|e| ClientError::Other(e.to_string()))?;
        Ok(Client {
            client,
            base_url: base_parsed,
        })
    }

    //////////////// Create Invoice ////////////////

    /// Creates the given invoice, returns a response containing the created invoice and a list of
    /// missing parcels (that have not yet been uploaded)
    pub async fn create_invoice(
        &self,
        inv: crate::Invoice,
    ) -> Result<crate::InvoiceCreateResponse> {
        let req = self.create_invoice_builder().body(toml::to_vec(&inv)?);
        self.create_invoice_request(req).await
    }

    /// Same as [`create_invoice`](Client::create_invoice), but takes a path to an invoice file
    /// instead. This will load the invoice file directly into the request, skipping serialization
    pub async fn create_invoice_from_file<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Result<crate::InvoiceCreateResponse> {
        // Create an owned version of the path to avoid worrying about lifetimes here for the stream
        let path = file_path.as_ref().to_owned();
        debug!("Loading invoice from {}", path.display());
        let (inv_stream, _) = load::raw(path).await?;
        debug!("Successfully loaded invoice stream");
        let req = self
            .create_invoice_builder()
            .body(Body::wrap_stream(inv_stream));
        self.create_invoice_request(req).await
    }

    fn create_invoice_builder(&self) -> RequestBuilder {
        // We can unwrap here because any URL error would be programmers fault
        self.client
            .post(self.base_url.join(INVOICE_ENDPOINT).unwrap())
            .header(header::CONTENT_TYPE, TOML_MIME_TYPE)
    }

    async fn create_invoice_request(
        &self,
        req: RequestBuilder,
    ) -> Result<crate::InvoiceCreateResponse> {
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Get Invoice ////////////////

    /// Returns the requested invoice from the bindle server if it exists. This can take any form
    /// that can convert into the `Id` type, but generally speaking, this is the canonical name of
    /// the bindle (e.g. `example.com/foo/1.0.0`)
    pub async fn get_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        // Validate the id
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let req = self.client.get(self.base_url.join(&format!(
            "{}/{}",
            INVOICE_ENDPOINT,
            parsed_id.to_string()
        ))?);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Query Invoice ////////////////

    /// Queries the bindle server for matching invoices as specified by the given query options
    pub async fn query_invoices(
        &self,
        query_opts: crate::QueryOptions,
    ) -> Result<crate::search::Matches> {
        let resp = self
            .client
            .get(self.base_url.join(QUERY_ENDPOINT).unwrap())
            .query(&query_opts)
            .send()
            .await?;
        let resp = unwrap_status(resp, Endpoint::Query).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Yank Invoice ////////////////

    /// Yanks the invoice from availability on the bindle server. This can take any form that can
    /// convert into the `Id` type, but generally speaking, this is the canonical name of the bindle
    /// (e.g. `example.com/foo/1.0.0`)
    pub async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let req = self.client.delete(self.base_url.join(&format!(
            "{}/{}",
            INVOICE_ENDPOINT,
            parsed_id.to_string()
        ))?);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Create Parcel ////////////////

    /// Creates the given parcel using the label and the raw parcel data to upload to the server.
    /// Returns the label of the created parcel
    pub async fn create_parcel(&self, label: crate::Label, data: Vec<u8>) -> Result<crate::Label> {
        // Once again, we can unwrap here because a mime type error is our fault
        let multipart = Form::new()
            .part(
                "label.toml",
                Part::bytes(toml::to_vec(&label)?)
                    .mime_str(TOML_MIME_TYPE)
                    .unwrap(),
            )
            .part("parcel.dat", Part::bytes(data));
        self.create_parcel_request(multipart).await
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes a path to the parcel
    /// file. This will be more efficient for large files as it will stream the data into the body
    /// rather than taking the intermediate step of loading the bytes into a `Vec`.
    ///
    /// NOTE: Currently this function does not work due to a missing upstream dependency feature.
    /// This feature has been added and we will update our code with the new version as soon as it
    /// is available. Right now, this will return an error if called
    pub async fn create_parcel_from_file<D: AsRef<Path>>(
        &self,
        _label: crate::Label,
        _data_path: D,
    ) -> Result<crate::Label> {
        Err(ClientError::Other(
            "Method is not currently supported. See documentation for additional details"
                .to_string(),
        ))
        // Copy the path to avoid lifetime issues
        // let data = data_path.as_ref().to_owned();
        // debug!("Loading parcel data from {}", data.display());
        // let (stream, _) = load::raw(data).await?;
        // debug!("Successfully loaded parcel stream");
        // let data_body = Body::wrap_stream(stream);

        // let multipart = Form::new()
        //     .part(
        //         "label.toml",
        //         Part::bytes(toml::to_vec(&label)?)
        //             .mime_str(TOML_MIME_TYPE)
        //             .unwrap(),
        //     )
        //     .part("parcel.dat", Part::stream(data_body));
        // self.create_parcel_request(multipart).await
    }

    async fn create_parcel_request(&self, form: Form) -> Result<crate::Label> {
        // We can unwrap here because any URL error would be programmers fault
        let req = self
            .client
            .post(self.base_url.join(PARCEL_ENDPOINT).unwrap())
            .multipart(form);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Parcel).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Get Parcel ////////////////

    /// Returns the requested parcel (identified by its SHA) as a vector of bytes
    pub async fn get_parcel(&self, sha: &str) -> Result<Vec<u8>> {
        let resp = self.get_parcel_request(sha).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Returns the requested parcel (identified by its SHA) as a stream of bytes. This is useful
    /// for when you don't want to read it into memory but are instead writing to a file or other
    /// location
    pub async fn get_parcel_stream(
        &self,
        sha: &str,
    ) -> Result<impl Stream<Item = Result<bytes::Bytes>>> {
        let resp = self.get_parcel_request(sha).await?;
        Ok(resp.bytes_stream().map(|r| r.map_err(|e| e.into())))
    }

    async fn get_parcel_request(&self, sha: &str) -> Result<reqwest::Response> {
        // Override the default accept header
        let resp = self
            .client
            .get(
                self.base_url
                    .join(&format!("{}/{}", PARCEL_ENDPOINT, sha))
                    .unwrap(),
            )
            .header(header::ACCEPT, "*/*")
            .send()
            .await?;
        unwrap_status(resp, Endpoint::Parcel).await
    }

    //////////////// Relationship Endpoints ////////////////

    /// Gets the labels of missing parcels, if any, of the specified bindle
    pub async fn get_missing_parcels<I>(&self, id: I) -> Result<Vec<crate::Label>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let req = self.client.get(self.base_url.join(&format!(
            "{}/{}/{}",
            RELATIONSHIP_ENDPOINT,
            "missing",
            parsed_id.to_string()
        ))?);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice::<crate::MissingParcelsResponse>(&resp.bytes().await?)?.missing)
    }
}

// A helper function and related enum to make some reusable code for unwrapping a status code and returning the right error

enum Endpoint {
    Invoice,
    Parcel,
    Query,
}

async fn unwrap_status(resp: reqwest::Response, endpoint: Endpoint) -> Result<reqwest::Response> {
    match (resp.status(), endpoint) {
        (StatusCode::OK, _) => Ok(resp),
        (StatusCode::ACCEPTED, Endpoint::Invoice) => Ok(resp),
        (StatusCode::NOT_FOUND, Endpoint::Invoice) => Err(ClientError::InvoiceNotFound),
        (StatusCode::NOT_FOUND, Endpoint::Parcel) => Err(ClientError::ParcelNotFound),
        (StatusCode::CONFLICT, Endpoint::Invoice) => Err(ClientError::InvoiceAlreadyExists),
        (StatusCode::CONFLICT, Endpoint::Parcel) => Err(ClientError::ParcelAlreadyExists),
        (StatusCode::UNAUTHORIZED, _) => Err(ClientError::Unauthorized),
        // You can't range match on u16 so we use a guard
        (_, _) if resp.status().is_server_error() => {
            Err(ClientError::ServerError(parse_error_from_body(resp).await))
        }
        (_, _) if resp.status().is_client_error() => Err(ClientError::InvalidRequest {
            status_code: resp.status(),
            message: parse_error_from_body(resp).await,
        }),
        _ => Err(ClientError::Other(format!(
            "Unknown error: {}",
            parse_error_from_body(resp).await.unwrap_or_default()
        ))),
    }
}

async fn parse_error_from_body(resp: reqwest::Response) -> Option<String> {
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return None,
    };

    match toml::from_slice::<crate::ErrorResponse>(&bytes) {
        Ok(e) => Some(e.error),
        Err(_) => None,
    }
}
