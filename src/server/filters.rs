use std::error::Error;
use std::fmt;
use std::io::Read;

use bytes::buf::BufExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use warp::reject::{custom, Reject, Rejection};
use warp::Filter;

use super::TOML_MIME_TYPE;

/// Query string options for the invoice endpoint
#[derive(Debug, Deserialize)]
pub struct InvoiceQuery {
    pub yanked: Option<bool>,
}

/// A warp filter that parses the body of a request from TOML to the specified type
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

pub(crate) async fn handle_deserialize_rejection(
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
struct BodyDeserializeError {
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
