use std::error::Error;
use std::fmt;
use std::io::Read;

use either::Either;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{debug, instrument, trace, warn};
use tracing_futures::Instrument;
use warp::mtls::Certificates;
use warp::reject::{custom, Reject, Rejection};
use warp::Filter;

use super::TOML_MIME_TYPE;
use crate::authn::{Authenticator, AuthData};
use crate::authz::always::Anonymous;
use crate::authz::Authorizer;

pub(crate) const PARCEL_ID_SEPARATOR: char = '@';

/// Query string options for the invoice endpoint
#[derive(Debug, Deserialize)]
pub struct InvoiceQuery {
    pub yanked: Option<bool>,
}

/// A warp filter that returns the invoice ID if the path is for an invoice and rejects it otherwise
pub fn invoice() -> impl Filter<Extract = (String,), Error = Rejection> + Copy {
    warp::path("_i")
        .and(warp::path::tail())
        .and_then(|tail: warp::path::Tail| {
            async move {
                let (inv, parcel) = match handle_tail(tail.as_str()) {
                    Ok(i) => i,
                    // The try operator doesn't work because I can't implement `From` for the sealed
                    // CombinedRejection type
                    Err(e) => return Err(e),
                };
                if parcel.is_some() {
                    return Err(custom(InvalidRequestPath));
                }
                Ok(inv)
            }
            .instrument(tracing::debug_span!("invoice_filter"))
        })
}

/// A warp filter that returns the invoice ID and parcel ID as a tuple if the path is for a parcel
/// and rejects it otherwise
pub fn parcel() -> impl Filter<Extract = ((String, String),), Error = Rejection> + Copy {
    warp::path("_i")
        .and(warp::path::tail())
        .and_then(|tail: warp::path::Tail| {
            async move {
                let (inv, parcel) = match handle_tail(tail.as_str()) {
                    Ok(i) => i,
                    // The try operator doesn't work because I can't implement `From` for the sealed
                    // CombinedRejection type
                    Err(e) => return Err(e),
                };
                let parcel = match parcel {
                    None => return Err(custom(InvalidRequestPath)),
                    Some(p) => p,
                };
                Ok((inv, parcel))
            }
            .instrument(tracing::debug_span!("parcel_filter"))
        })
}

#[instrument(level = "trace")]
fn handle_tail(tail: &str) -> Result<(String, Option<String>), Rejection> {
    let mut split: Vec<String> = tail
        .split(PARCEL_ID_SEPARATOR)
        .map(|s| s.to_owned())
        .collect();

    // The unwraps here are safe because we are checking length
    match split.len() {
        1 => {
            trace!(bindle_id = %split[0], "Matched only bindle ID");
            Ok((split.pop().unwrap(), None))
        }
        2 => {
            trace!(
                bindle_id = %split[0],
                sha = %split[1],
                "Matched bindle ID and sha"
            );
            let parcel = split.pop().unwrap();
            let inv = split.pop().unwrap();
            Ok((inv, Some(parcel)))
        }
        _ => Err(custom(InvalidRequestPath)),
    }
}

/// A warp filter for adding an authenticator
fn authenticate<Authn: Authenticator + Clone + Send + Sync>(
    authn: Authn,
) -> impl Filter<Extract = (Either<Anonymous, Authn::Item>,), Error = Rejection> + Clone {
    // We get the header optionally as anonymous auth could be enabled
    warp::any()
        .map(move || authn.clone())
        .and(warp::header::optional::<String>("Authorization"))
        .and(warp::mtls::peer_certificates())
        .and_then(_authenticate)
}

#[instrument(level = "trace", skip(authn, auth_header, peer_certs), name = "authentication")]
async fn _authenticate<A: Authenticator + Clone + Send>(
    authn: A,
    auth_header: Option<String>,
    peer_certs: Option<Certificates>
) -> Result<Either<Anonymous, A::Item>, Rejection> {
    if let (None, None) = (&auth_header, &peer_certs) {
        // If we had no auth data, that means this is anonymous
        return Ok(Either::Left(Anonymous));
    }
    let auth_data = AuthData{ auth_header, peer_certs };
    match authn.authenticate(&auth_data).await {
        Ok(a) => Ok(Either::Right(a)),
        Err(e) => {
            debug!(error = %e, "Authentication error");
            Err(warp::reject::custom(AuthnFail))
        }
    }
}

#[derive(Debug)]
struct AuthnFail;

impl warp::reject::Reject for AuthnFail {}

#[instrument(level = "trace", skip(err))]
pub(crate) async fn handle_authn_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, warp::Rejection> {
    if err.find::<AuthnFail>().is_some() {
        debug!("Handling rejection as authn rejection");
        Ok(crate::server::reply::reply_from_error(
            "unauthorized",
            warp::http::StatusCode::UNAUTHORIZED,
        ))
    } else {
        Err(err)
    }
}

/// A warp filter for adding an authorizer
pub(crate) fn authenticate_and_authorize<
    Authn: Authenticator + Clone + Send + Sync,
    Authz: Authorizer + Clone + Send + Sync,
>(
    authn: Authn,
    authz: Authz,
) -> impl Filter<Extract = ((),), Error = Rejection> + Clone {
    authenticate(authn)
        .and(warp::path::full())
        .and(warp::method())
        .and(warp::any().map(move || authz.clone()))
        .and_then(
            |item: Either<Anonymous, Authn::Item>,
             path: warp::path::FullPath,
             method: warp::http::Method,
             authz: Authz| {
                async move {
                    trace!(path = path.as_str(), %method, "Authorizing request");
                    if let Err(e) = item.either(
                        |anon| authz.authorize(anon, path.as_str(), &method),
                        |i| authz.authorize(i, path.as_str(), &method),
                    ) {
                        debug!(error = %e, "Authorization error");
                        return Err(warp::reject::custom(AuthzFail));
                    }
                    Ok(())
                }
                .instrument(tracing::trace_span!("authorization"))
            },
        )
}

#[derive(Debug)]
struct AuthzFail;

impl warp::reject::Reject for AuthzFail {}

#[instrument(level = "trace", skip(err))]
pub(crate) async fn handle_authz_rejection(
    err: warp::Rejection,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    if err.find::<AuthzFail>().is_some() {
        debug!("Handling rejection as authz rejection");
        Ok(crate::server::reply::reply_from_error(
            "access denied",
            warp::http::StatusCode::FORBIDDEN,
        ))
    } else {
        Err(err)
    }
}

/// A warp filter that parses the body of a request from TOML to the specified type
// Lovingly borrowed from https://docs.rs/warp/0.2.5/src/warp/filters/body.rs.html
pub fn toml<T: DeserializeOwned + Send>() -> impl Filter<Extract = (T,), Error = Rejection> + Copy {
    // We can't use the http type constant here because clippy is warning about it having internal
    // mutability.
    warp::filters::header::header::<String>("Content-Type")
        .and(warp::body::aggregate())
        .and_then(parse_toml)
}

async fn parse_toml<T: DeserializeOwned + Send>(
    raw_header: String,
    buf: impl warp::Buf,
) -> Result<T, Rejection> {
    let mime: mime::Mime = raw_header
        .parse()
        .map_err(|err: mime::FromStrError| custom(BodyDeserializeError { cause: err.into() }))?;
    // As far as I can tell from the code, essence_str is lowercased, so we shouldn't need to
    // do it here
    if mime.essence_str() != TOML_MIME_TYPE {
        return Err(custom(BodyDeserializeError {
            cause: "content-type is not TOML".into(),
        }));
    }
    let mut raw = Vec::new();
    buf.reader()
        .read_to_end(&mut raw)
        .map_err(|err| custom(BodyDeserializeError { cause: err.into() }))?;
    toml::from_slice(&raw).map_err(|err| {
        warn!("Failed to deserialize TOML file: {}", err);
        custom(BodyDeserializeError { cause: err.into() })
    })
}

#[instrument(level = "trace", skip(err))]
pub(crate) async fn handle_deserialize_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(e) = err.find::<BodyDeserializeError>() {
        debug!("Handling rejection as deserialize rejection");
        Ok(crate::server::reply::reply_from_error(
            e,
            warp::http::StatusCode::BAD_REQUEST,
        ))
    } else if let Some(e) = err.find::<warp::body::BodyDeserializeError>() {
        // Because we are handling the built in JSON filter, we need to handle the error here as it
        // is different from ours
        debug!("Handling rejection as built in deserialize rejection");
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

#[instrument(level = "trace", skip(err))]
pub(crate) async fn handle_invalid_request_path(
    err: warp::Rejection,
) -> Result<impl warp::Reply, warp::Rejection> {
    if err.find::<InvalidRequestPath>().is_some() {
        debug!("Handling rejection as invalid request rejection");
        Ok(crate::server::reply::reply_from_error(
            "Invalid URL. Missing Bindle ID and/or parcel SHA",
            warp::http::StatusCode::BAD_REQUEST,
        ))
    } else {
        Err(err)
    }
}

#[derive(Debug)]
struct InvalidRequestPath;

impl Reject for InvalidRequestPath {}
