use serde::Serialize;
use warp::http::status::StatusCode;
use warp::reply::Response;
use warp::Reply;

use super::TOML_MIME_TYPE;
use crate::storage::StorageError;
use crate::{Invoice, Label};

/// A custom wrapper for responding to invoice creation responses. Because invoices can be created
/// before parcels are uploaded, we need to inform the user if there are missing parcels in the
/// bindle spec
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InvoiceCreateResponse {
    pub invoice: Invoice,
    pub missing: Option<Vec<Label>>,
}

// Borrowed and modified from https://docs.rs/warp/0.2.5/src/warp/reply.rs.html#102
pub fn toml<T>(val: &T) -> Toml
where
    T: Serialize,
{
    Toml {
        // TODO: When we add logging, log the error here so we know when there is a serialize
        // failure
        inner: toml::to_vec(val).map_err(|_| ()),
    }
}

/// A JSON formatted reply.
pub struct Toml {
    inner: Result<Vec<u8>, ()>,
}

impl Reply for Toml {
    #[inline]
    fn into_response(self) -> Response {
        match self.inner {
            Ok(body) => {
                let mut res = Response::new(body.into());
                res.headers_mut().insert(
                    warp::http::header::CONTENT_TYPE,
                    warp::http::header::HeaderValue::from_static(TOML_MIME_TYPE),
                );
                res
            }
            Err(()) => warp::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

/// A helper function for converting a [`StorageError`](crate::storage::StorageError) into a Warp
/// `Reply` with the proper status code. It will return a TOML body that looks like:
/// ```toml
/// error = "bindle is yanked"
/// ```
pub fn into_reply(error: StorageError) -> warp::reply::WithStatus<Toml> {
    let status_code = match &error {
        StorageError::Yanked => StatusCode::BAD_REQUEST,
        StorageError::CreateYanked => StatusCode::UNPROCESSABLE_ENTITY,
        StorageError::NotFound => StatusCode::NOT_FOUND,
        StorageError::IO(_) => StatusCode::INTERNAL_SERVER_ERROR,
        StorageError::Exists => StatusCode::BAD_REQUEST,
        StorageError::Malformed(_) => StatusCode::BAD_REQUEST,
        StorageError::Unserializable(_) => StatusCode::BAD_REQUEST,
    };

    warp::reply::with_status(
        toml(&format!("error = \"{}\"", error.to_string())),
        status_code,
    )
}
