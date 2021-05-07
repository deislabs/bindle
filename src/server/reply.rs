use serde::Serialize;
use warp::http::header::HeaderValue;
use warp::http::status::StatusCode;
use warp::reply::Response;
use warp::Reply;

use tracing::debug;

use super::{JSON_MIME_TYPE, TOML_MIME_TYPE};
use crate::provider::ProviderError;

const SUPPORTED_SERIALIZERS: &[&str] = &[TOML_MIME_TYPE, JSON_MIME_TYPE, "text/json"];

/// Use an accept header to determine how to serialize content.
///
/// This will examine the Accept header, looking for the best match, and then it will
/// use the appropriate serializer to serialize the data.
///
/// The current implementation ignores `q=` annotations, assigning preference based on
/// the first MIME type to match.
///
/// For example, `Accept: text/json, application/toml;q=0.9` will cause encoding to be in JSON.
/// If no suitable content type is found, this will encode in application/toml, as that
/// is the behavior described in the spec.
pub fn serialized_data<T>(val: &T, accept: String) -> SerializedData
where
    T: Serialize,
{
    let default_mime = "*/*".to_owned();
    let accept_items = parse_accept(accept.as_str());
    debug!(
        %accept,
        ?accept_items,
        "Parsed accept header into list",
    );
    let best_fit = accept_items
        .iter()
        .find(|i| SUPPORTED_SERIALIZERS.contains(&i.as_str()))
        .unwrap_or(&default_mime);
    debug!(%best_fit, "Selected a best-fit MIME");
    let mut final_mime = TOML_MIME_TYPE;
    let inner = match (*best_fit).as_str() {
        JSON_MIME_TYPE | "text/json" => {
            final_mime = JSON_MIME_TYPE;
            serde_json::to_vec(val).map_err(|e| {
                tracing::log::error!("Error while serializing TOML: {:?}", e);
            })
        }
        // TOML is default
        _ => toml::to_vec(val).map_err(|e| {
            tracing::log::error!("Error while serializing TOML: {:?}", e);
        }),
    };
    debug!(%final_mime, "negotiated MIME");
    SerializedData {
        inner,
        mime: final_mime.to_owned(),
    }
}

fn parse_accept(header: &str) -> Vec<String> {
    header
        .split(',')
        .map(|h| {
            let normalized = h.trim().to_lowercase();
            let parts: Vec<&str> = normalized.split(";").collect();
            let mime = parts[0].clone();
            mime.to_owned()
        })
        .collect()
}

/// A serialized body.
///
/// Currently, this may be JSON or TOML.
pub struct SerializedData {
    inner: Result<Vec<u8>, ()>,
    mime: String,
}

impl Reply for SerializedData {
    #[inline]
    fn into_response(self) -> Response {
        match self.inner {
            Ok(body) => {
                let mut res = Response::new(body.into());
                res.headers_mut().insert(
                    warp::http::header::CONTENT_TYPE,
                    HeaderValue::from_str(self.mime.as_str())
                        .unwrap_or(HeaderValue::from_static(TOML_MIME_TYPE)),
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
pub fn into_reply(error: ProviderError) -> warp::reply::WithStatus<SerializedData> {
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
) -> warp::reply::WithStatus<SerializedData> {
    warp::reply::with_status(
        serialized_data(
            &crate::ErrorResponse {
                error: error.to_string(),
            },
            TOML_MIME_TYPE.to_owned(),
        ),
        status_code,
    )
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_parse_accept() {
        assert_eq!(vec!["application/toml"], parse_accept("application/toml"));

        assert_eq!(vec!["application/toml"], parse_accept("application/TOML"));

        assert_eq!(
            vec!["text/json", "application/json"],
            parse_accept("text/json, application/json;q=0.9")
        );
    }
}
