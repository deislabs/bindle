use serde::Serialize;
use warp::http::status::StatusCode;
use warp::reply::Response;
use warp::Reply;

use super::TOML_MIME_TYPE;
use crate::provider::ProviderError;

// Borrowed and modified from https://docs.rs/warp/0.2.5/src/warp/reply.rs.html#102
pub fn toml<T>(val: &T) -> Toml
where
    T: Serialize,
{
    Toml {
        inner: toml::to_vec(val).map_err(|e| {
            tracing::log::error!("Error while serializing TOML: {:?}", e);
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

/// A helper function for converting a [`ProviderError`](crate::provider::ProviderError) into a Warp
/// `Reply` with the proper status code. It will return a TOML body that looks like:
/// ```toml
/// error = "bindle is yanked"
/// ```
pub fn into_reply(error: ProviderError) -> warp::reply::WithStatus<Toml> {
    let mut error = error;
    let status_code = match &error {
        ProviderError::CreateYanked => StatusCode::UNPROCESSABLE_ENTITY,
        ProviderError::NotFound => StatusCode::NOT_FOUND,
        ProviderError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Remap the error in the case this is a not found error
            error = ProviderError::NotFound;
            StatusCode::NOT_FOUND
        }
        ProviderError::Exists | ProviderError::WriteInProgress => StatusCode::CONFLICT,
        ProviderError::Malformed(_)
        | ProviderError::Unserializable(_)
        | ProviderError::DigestMismatch
        | ProviderError::InvalidId => StatusCode::BAD_REQUEST,
        ProviderError::Yanked => StatusCode::FORBIDDEN,
        #[cfg(feature = "client")]
        ProviderError::ProxyError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ProviderError::Other(_) | ProviderError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
