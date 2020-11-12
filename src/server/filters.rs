use std::error::Error;
use std::fmt;
use std::io::Read;

use bytes::buf::BufExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use warp::reject::{custom, Reject, Rejection};
use warp::Filter;

use super::TOML_MIME_TYPE;
use crate::search::SearchOptions;

#[derive(Debug, Deserialize)]
pub struct InvoiceQuery {
    pub yanked: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryOptions {
    #[serde(alias = "q")]
    pub query: Option<String>,
    #[serde(alias = "v")]
    pub version: Option<String>,
    #[serde(alias = "o")]
    pub offset: Option<u64>,
    #[serde(alias = "l")]
    pub limit: Option<u8>,
    pub strict: Option<bool>,
    pub yanked: Option<bool>,
}

// This isn't a `From` implementation because it isn't symmetrical. Converting from a
// `SearchOptions` to a `QueryOptions` would always end up with some fields set to none.
impl Into<SearchOptions> for QueryOptions {
    fn into(self) -> SearchOptions {
        let defaults = SearchOptions::default();
        SearchOptions {
            limit: self.limit.unwrap_or(defaults.limit),
            offset: self.offset.unwrap_or(defaults.offset),
            strict: self.strict.unwrap_or(defaults.strict),
            yanked: self.yanked.unwrap_or(defaults.yanked),
        }
    }
}

// Lovingly borrowed from https://docs.rs/warp/0.2.5/src/warp/filters/body.rs.html
pub fn toml<T: DeserializeOwned + Send>() -> impl Filter<Extract = (T,), Error = Rejection> + Copy {
    // We can't use the http type constant here because clippy is warning about it having internal
    // mutability.
    warp::filters::header::exact_ignore_case("Content-Type", TOML_MIME_TYPE)
        .and(warp::body::aggregate())
        .and_then(parse_toml)
}

async fn parse_toml<T: DeserializeOwned + Send>(buf: impl warp::Buf) -> Result<T, Rejection> {
    let mut raw = Vec::new();
    buf.reader()
        .read_to_end(&mut raw)
        .map_err(|err| custom(BodyDeserializeError { cause: err.into() }))?;
    toml::from_slice(&raw).map_err(|err| custom(BodyDeserializeError { cause: err.into() }))
}

pub async fn handle_deserialize_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(e) = err.find::<BodyDeserializeError>() {
        Ok(crate::server::reply::reply_from_error(
            e,
            warp::http::StatusCode::BAD_REQUEST,
        ))
    } else {
        Err(err)
    }
}

#[derive(Debug)]
pub struct BodyDeserializeError {
    cause: Box<dyn Error + Send + Sync>,
}

impl fmt::Display for BodyDeserializeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Request body toml deserialize error: {}", self.cause)
    }
}

impl Error for BodyDeserializeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

impl Reject for BodyDeserializeError {}
