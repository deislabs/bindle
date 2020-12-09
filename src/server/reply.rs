use serde::Serialize;
use warp::http::status::StatusCode;
use warp::reply::Response;
use warp::Reply;

use super::TOML_MIME_TYPE;
use crate::storage::StorageError;

// Borrowed and modified from https://docs.rs/warp/0.2.5/src/warp/reply.rs.html#102
pub fn toml<T>(val: &T) -> Toml
where
    T: Serialize,
{
    Toml {
        inner: toml::to_vec(val).map_err(|e| {
            log::error!("Error while serializing TOML: {:?}", e);
        }),
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
    let mut error = error;
    let status_code = match &error {
        StorageError::CreateYanked => StatusCode::UNPROCESSABLE_ENTITY,
        StorageError::NotFound => StatusCode::NOT_FOUND,
        StorageError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Remap the error in the case this is a not found error
            error = StorageError::NotFound;
            StatusCode::NOT_FOUND
        }
        StorageError::Exists => StatusCode::CONFLICT,
        StorageError::Malformed(_)
        | StorageError::Unserializable(_)
        | StorageError::DigestMismatch
        | StorageError::InvalidId => StatusCode::BAD_REQUEST,
        StorageError::Yanked => StatusCode::FORBIDDEN,
        #[cfg(feature = "caching")]
        StorageError::CacheError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        StorageError::Other(_) | StorageError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };

    reply_from_error(error, status_code)
}

// A more generic wrapper that takes any ToString implementation (which includes Errors) and builds
// a TOML error body with the given status code
pub fn reply_from_error(
    error: impl std::string::ToString,
    status_code: warp::http::StatusCode,
) -> warp::reply::WithStatus<Toml> {
    warp::reply::with_status(
        toml(&crate::ErrorResponse {
            error: error.to_string(),
        }),
        status_code,
    )
}
