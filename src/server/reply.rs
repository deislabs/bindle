use serde::Serialize;
use warp::http::header::HeaderValue;
use warp::http::status::StatusCode;
use warp::reply::Response;
use warp::Reply;

use tracing::debug;

use super::{JSON_MIME_TYPE, TOML_MIME_TYPE};
use crate::provider::ProviderError;

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
    let best_fit = accept_best_fit(accept.as_str());
    let inner = match best_fit {
        JSON_MIME_TYPE => serde_json::to_vec(val).map_err(|e| {
            tracing::log::error!("Error while serializing TOML: {:?}", e);
        }),
        // TOML is default
        _ => toml::to_vec(val).map_err(|e| {
            tracing::log::error!("Error while serializing TOML: {:?}", e);
        }),
    };
    SerializedData {
        inner,
        mime: best_fit.to_owned(),
    }
}

/// Parse an Accept header and return the best possible handler.
///
/// This will always return one of the supported serializers, defaulting to
/// application/toml.
fn accept_best_fit(accept_value: &str) -> &str {
    let accept_items = parse_accept(accept_value);
    debug!(
        %accept_value,
        ?accept_items,
        "Parsed accept header into list",
    );

    // Basically, we're working around the issue that there are multiple MIME types
    // for JSON (application/json and text/json, as well as application/json+STUFF)
    let best_fit = accept_items
        .iter()
        .find_map(|m| match m.subtype().as_str() {
            "toml" => Some(TOML_MIME_TYPE),
            "json" => Some(JSON_MIME_TYPE),
            _ => None,
        })
        .unwrap_or(TOML_MIME_TYPE);

    debug!(%best_fit, "Selected a best-fit MIME");
    best_fit
}

fn parse_accept(header: &str) -> Vec<mime::Mime> {
    header
        .split(',')
        .filter_map(|h| match h.trim().parse::<mime::Mime>() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(
                    header,
                    %e,
                    "Accept header contains unparsable media type. Ignoring."
                );
                None
            }
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
                        .unwrap_or_else(|_| HeaderValue::from_static(TOML_MIME_TYPE)),
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
        | ProviderError::InvalidId(_)
        | ProviderError::SizeMismatch => StatusCode::BAD_REQUEST,
        ProviderError::Yanked => StatusCode::FORBIDDEN,
        #[cfg(feature = "client")]
        ProviderError::ProxyError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ProviderError::Other(_) | ProviderError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ProviderError::FailedSigning(_) => StatusCode::BAD_REQUEST,
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
        // In these cases, the `mime.to_string()` is fine for testing
        assert_eq!(vec!["application/toml"], parse_accept("application/toml"));

        assert_eq!(vec!["application/toml"], parse_accept("application/TOML"));

        // In this case, we want to test the essence_str
        assert_eq!(
            vec!["text/json", "application/json"],
            parse_accept("text/json, application/json;q=0.9")
                .iter()
                .map(|m| m.essence_str())
                .collect::<Vec<&str>>()
        );
    }

    #[test]
    fn test_accept_best_fit() {
        assert_eq!(TOML_MIME_TYPE, accept_best_fit("application/toml"));
        assert_eq!(JSON_MIME_TYPE, accept_best_fit("text/json"));
        assert_eq!(
            JSON_MIME_TYPE,
            accept_best_fit("text/plain,application/json")
        );
        assert_eq!(
            JSON_MIME_TYPE,
            accept_best_fit("text/plain, application/json, image/jpeg")
        );

        // use JSON for the oddball cases b/c TOML is the default if a parse fails
        assert_eq!(
            JSON_MIME_TYPE,
            accept_best_fit("application/json;hello=world")
        );
        assert_eq!(JSON_MIME_TYPE, accept_best_fit("application/json+bindle"));
        assert_eq!(
            JSON_MIME_TYPE,
            accept_best_fit("not-a-mime, text/json, also/not/a/mime")
        );

        // Default cases
        assert_eq!(TOML_MIME_TYPE, accept_best_fit(""));
        assert_eq!(TOML_MIME_TYPE, accept_best_fit("*"));
        assert_eq!(TOML_MIME_TYPE, accept_best_fit("*/*"));
        assert_eq!(TOML_MIME_TYPE, accept_best_fit("text/plain"));

        // Should go by order of appearance in the list
        // We don't support `p=`. It's not worth the effort.
        assert_eq!(
            JSON_MIME_TYPE,
            accept_best_fit("application/json, application/toml")
        );
        assert_eq!(
            TOML_MIME_TYPE,
            accept_best_fit("application/toml, application/json")
        );
    }
}
