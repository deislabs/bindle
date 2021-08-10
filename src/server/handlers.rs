use std::convert::Infallible;

use tracing::{debug, instrument, trace, trace_span};
use warp::Reply;

use super::filters::InvoiceQuery;
use super::reply;
use crate::invoice::{SignatureRole, VerificationStrategy};
use crate::provider::{Provider, ProviderError};
use crate::search::Search;

pub mod v1 {
    use super::*;

    use crate::{
        signature::{KeyRing, SecretKeyStorage},
        LoginParams, LoginProvider, QueryOptions, SignatureError,
    };

    use oauth2::reqwest::async_http_client;
    use oauth2::{basic::BasicClient, devicecode::StandardDeviceAuthorizationResponse};
    use oauth2::{AuthUrl, ClientId, DeviceAuthorizationUrl, Scope};
    use tokio_stream::{self as stream, StreamExt};
    use tracing::Instrument;
    use warp::http::StatusCode;

    const GITHUB_AUTH_URL: &str = "https://github.com/login/oauth/authorize";
    const GITHUB_DEVICE_AUTH_URL: &str = "https://github.com/login/device/code";

    //////////// Invoice Functions ////////////
    #[instrument(level = "trace", skip(index))]
    pub async fn query_invoices<S: Search>(
        options: QueryOptions,
        index: S,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible> {
        let term = options.query.clone().unwrap_or_default();
        let version = options.version.clone().unwrap_or_default();
        debug!(
            %term,
            %version,
            "Querying invoice index",
        );
        let matches = match index.query(&term, &version, options.into()).await {
            Ok(m) => m,
            Err(e) => {
                debug!(error = %e, "Got bad query request");
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
        };

        trace!(?matches, "Index query successful");

        Ok(warp::reply::with_status(
            reply::serialized_data(&matches, accept_header.unwrap_or_default()),
            warp::http::StatusCode::OK,
        ))
    }

    #[instrument(level = "trace", skip(store, secret_store))]
    pub async fn create_invoice<P: Provider, S: SecretKeyStorage>(
        store: P,
        secret_store: S,
        strategy: VerificationStrategy,
        keyring: std::sync::Arc<KeyRing>,
        inv: crate::Invoice,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible> {
        let accept = accept_header.unwrap_or_default();
        trace!("Create invoice request with invoice: {:?}", inv);

        // Right here, I need to load one secret key and a ring of public keys.
        // Then I need to validate the invoice against the public keys, sign the invoice
        // with my private key, and THEN go on to store.create_invoice()

        let role = SignatureRole::Host;
        let sk = match secret_store.get_first_matching(&role) {
            None => {
                return Ok(reply::into_reply(ProviderError::FailedSigning(
                    SignatureError::NoSuitableKey,
                )))
            }
            Some(k) => k,
        };

        let verified = match strategy.verify(inv, &keyring) {
            Ok(v) => v,
            Err(e) => return Ok(reply::into_reply(ProviderError::FailedSigning(e))),
        };
        let signed = match crate::sign(verified, vec![(role, sk)]) {
            Ok(s) => s,
            Err(e) => return Ok(reply::into_reply(ProviderError::FailedSigning(e))),
        };

        let (invoice, labels) = match store.create_invoice(signed).await {
            Ok(l) => l,
            Err(e) => {
                return Ok(reply::into_reply(e));
            }
        };
        // If there are missing parcels that still need to be created, return a 202 to indicate that
        // things were accepted, but will not be fetchable until further action is taken
        if !labels.is_empty() {
            trace!(
                invoice_id = %invoice.bindle.id,
                missing = labels.len(),
                "Newly created invoice is missing parcels",
            );
            Ok(warp::reply::with_status(
                reply::serialized_data(
                    &crate::InvoiceCreateResponse {
                        invoice,
                        missing: Some(labels),
                    },
                    accept,
                ),
                warp::http::StatusCode::ACCEPTED,
            ))
        } else {
            trace!(
                invoice_id = %invoice.bindle.id,
                "Newly created invoice has all existing parcels",
            );
            Ok(warp::reply::with_status(
                reply::serialized_data(
                    &crate::InvoiceCreateResponse {
                        invoice,
                        missing: None,
                    },
                    accept,
                ),
                warp::http::StatusCode::CREATED,
            ))
        }
    }

    #[instrument(level = "trace", skip(store), fields(id = %id, yanked = query.yanked.unwrap_or_default()))]
    pub async fn get_invoice<P: Provider + Sync>(
        id: String,
        query: InvoiceQuery,
        store: P,
        accept_header: Option<String>,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        let accept = accept_header.unwrap_or_default();

        let res = if query.yanked.unwrap_or_default() {
            store.get_yanked_invoice(id)
        } else {
            store.get_invoice(id)
        };
        let inv = match res.await {
            Ok(i) => i,
            Err(e) => {
                debug!(error = %e, "Got error during get invoice request");
                return Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(reply::into_reply(e)));
            }
        };
        let res = Box::new(warp::reply::with_status(
            reply::serialized_data(&inv, accept),
            warp::http::StatusCode::OK,
        ));
        Ok::<Box<dyn warp::Reply>, Infallible>(res)
    }

    #[instrument(level = "trace", skip(store), fields(id = tail.as_str()))]
    pub async fn yank_invoice<P: Provider>(
        tail: warp::path::Tail,
        store: P,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible> {
        let id = tail.as_str();
        if let Err(e) = store.yank_invoice(id).await {
            debug!(error = %e, "Got error during yank invoice request");
            return Ok(reply::into_reply(e));
        }

        let mut resp = std::collections::HashMap::new();
        resp.insert("message", "invoice yanked");
        Ok(warp::reply::with_status(
            reply::serialized_data(&resp, accept_header.unwrap_or_default()),
            warp::http::StatusCode::OK,
        ))
    }

    #[instrument(level = "trace", skip(store))]
    pub async fn head_invoice<P: Provider + Sync>(
        id: String,
        query: InvoiceQuery,
        store: P,
        accept_header: Option<String>,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Getting invoice data");
        let inv = get_invoice(id, query, store, accept_header).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(super::HeadResponse {
            headers: parts.headers,
        }))
    }

    //////////// Parcel Functions ////////////
    #[instrument(level = "trace", skip(store, body))]
    pub async fn create_parcel<P, B, D>(
        (bindle_id, sha): (String, String),
        body: B,
        store: P,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible>
    where
        P: Provider + Sync,
        B: stream::Stream<Item = Result<D, warp::Error>> + Send + Sync + Unpin + 'static,
        D: bytes::Buf + Send,
    {
        trace!("Checking if parcel exists in bindle");

        // Validate that this sha belongs
        if let Err(e) = parcel_in_bindle(&store, &bindle_id, &sha).await {
            return Ok(e);
        }

        if let Err(e) = store
            .create_parcel(
                bindle_id,
                &sha,
                body.map(|res| {
                    res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                }),
            )
            .await
        {
            debug!(error = %e, "Got error while creating parcel in store");
            return Ok(reply::into_reply(e));
        }

        let mut resp = std::collections::HashMap::new();
        resp.insert("message", "parcel created");
        Ok(warp::reply::with_status(
            reply::serialized_data(&resp, accept_header.unwrap_or_default()),
            warp::http::StatusCode::OK,
        ))
    }

    #[instrument(level = "trace", skip(store))]
    pub async fn get_parcel<P: Provider + Sync>(
        (bindle_id, id): (String, String),
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        // Get parcel label to ascertain content type and length, and validate that it does exist
        let label = match parcel_in_bindle(&store, &bindle_id, &id).await {
            Ok(l) => l,
            Err(e) => return Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(e)),
        };

        let data = match store.get_parcel(bindle_id, &id).await {
            Ok(reader) => reader,
            Err(e) => {
                debug!(error = %e, "Got error while getting parcel from store");
                return Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(reply::into_reply(e)));
            }
        };

        // TODO: If we start to use compression on the body, we'll need a new custom header for
        // _actual_ size of the parcel, so the client can reconstruct the label data from headers
        // without needing to read the whole (possibly large) file
        let resp = warp::http::Response::builder()
            .header(warp::http::header::CONTENT_TYPE, label.media_type)
            .header(warp::http::header::CONTENT_LENGTH, label.size)
            .body(hyper::Body::wrap_stream(data))
            .unwrap();

        // Gotta box because this is not a toml reply type (which we use for sending error messages to the user)
        Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(warp::reply::with_status(
            resp,
            warp::http::StatusCode::OK,
        )))
    }

    #[instrument(level = "trace", skip(store))]
    pub async fn head_parcel<P: Provider + Sync>(
        (bindle_id, id): (String, String),
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Getting parcel data");
        let inv = get_parcel((bindle_id, id), store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok::<Box<dyn warp::Reply>, Infallible>(Box::new(super::HeadResponse {
            headers: parts.headers,
        }))
    }

    //////////// Relationship Functions ////////////
    #[instrument(level = "trace", skip(store), fields(id = tail.as_str()))]
    pub async fn get_missing<P: Provider + Sync + Clone>(
        tail: warp::path::Tail,
        store: P,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible> {
        let id = tail.as_str();

        let inv = match store.get_invoice(id).await {
            Ok(i) => i,
            Err(e) => {
                trace!("Got error during get missing request: {:?}", e);
                return Ok(reply::into_reply(e));
            }
        };

        let missing_futures = inv
            .parcel
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p, id.to_owned(), store.clone()))
            .map(|(p, bindle_id, store)| async move {
                // We can't use a filter_map with async, so we need to map first, then collect things with a filter
                match store.parcel_exists(bindle_id, &p.label.sha256).await {
                    Ok(b) => {
                        // For some reason, guard blocks on bools don't indicate to the compiler
                        // that they are exhaustive, so hence the nested if block
                        if b {
                            // If it exists, don't include it
                            Ok(None)
                        } else {
                            Ok(Some(p.label))
                        }
                    }
                    Err(e) => Err(e),
                }
            });

        let missing = match futures::future::join_all(missing_futures)
            .instrument(trace_span!("find_missing_parcels"))
            .await
            .into_iter()
            .collect::<Result<Vec<Option<crate::Label>>, crate::provider::ProviderError>>()
        {
            Ok(m) => m.into_iter().flatten().collect::<Vec<crate::Label>>(),
            Err(e) => {
                trace!("Got error during get missing request: {:?}", e);
                return Ok(reply::into_reply(e));
            }
        };
        Ok(warp::reply::with_status(
            reply::serialized_data(
                &crate::MissingParcelsResponse { missing },
                accept_header.unwrap_or_default(),
            ),
            warp::http::StatusCode::OK,
        ))
    }

    //////////// Login Functions ////////////

    /// Redirects to a login request
    #[instrument(level = "trace")]
    pub(crate) async fn login(
        p: LoginParams,
        client_id: String,
        accept_header: Option<String>,
    ) -> Result<impl warp::Reply, Infallible> {
        match p.provider {
            LoginProvider::Github => {
                let client = BasicClient::new(
                    ClientId::new(client_id.clone()),
                    None,
                    // I don't think we actually need this, but including it anyway
                    AuthUrl::new(GITHUB_AUTH_URL.to_owned())
                        .expect("The github auth URL is invalid, this is programmer error"),
                    None,
                )
                .set_device_authorization_url(
                    DeviceAuthorizationUrl::new(GITHUB_DEVICE_AUTH_URL.to_owned())
                        .expect("Incorrect device auth url. This is programmer error"),
                );
                let device_auth_req = match client.exchange_device_code() {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = ?e, "Unable to create request for device auth");
                        return Ok(reply::reply_from_error(
                            "Error with Github auth request",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        ));
                    }
                };
                let details: StandardDeviceAuthorizationResponse = match device_auth_req
                    .add_scope(Scope::new("user:email".into()))
                    .request_async(async_http_client)
                    .await
                {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!(error = ?e, "Unable to perform device auth code request to Github");
                        // NOTE: We could probably inspect the error a bit to see if this is our
                        // fault or if we are getting an error from the GH API which would be an
                        // internal server error and bad gateway respectively
                        return Ok(reply::reply_from_error(
                            "Error performing Github auth request",
                            StatusCode::BAD_GATEWAY,
                        ));
                    }
                };

                // Inject in the additional client_id parameter after serializing to a Value
                let mut intermediate = match serde_json::to_value(details) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(error = %e, "Unable to serialize device auth response to intermediate value, this shouldn't happen");
                        return Ok(reply::reply_from_error(
                            "Error with Github auth request",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        ));
                    }
                };
                intermediate["client_id"] = client_id.into();

                Ok(warp::reply::with_status(
                    reply::serialized_data(&intermediate, accept_header.unwrap_or_default()),
                    warp::http::StatusCode::OK,
                ))
            }
        }
    }

    //////////// Helper Functions ////////////

    /// Fetches an invoice from the given store and checks that the given SHA exists within that
    /// invoice. Returns a result where the Error variant is a warp reply containing the error
    #[instrument(level = "trace", skip(store))]
    async fn parcel_in_bindle<P: Provider + Sync>(
        store: &P,
        bindle_id: &str,
        sha: &str,
    ) -> std::result::Result<
        crate::Label,
        warp::reply::WithStatus<crate::server::reply::SerializedData>,
    > {
        trace!("fetching invoice data");
        let inv = match store.get_invoice(bindle_id).await {
            Ok(i) => i,
            Err(e) => return Err(reply::into_reply(e)),
        };

        // Make sure the sha exists in the list
        let label = inv
            .parcel
            .map(|parcels| parcels.into_iter().find(|p| p.label.sha256 == sha))
            .flatten()
            .map(|p| p.label);

        match label {
            Some(l) => Ok(l),
            None => Err(reply::reply_from_error(
                format!("Parcel SHA {} does not exist in invoice {}", sha, bindle_id),
                warp::http::StatusCode::BAD_REQUEST,
            )),
        }
    }
}

// A helper struct for HEAD responses that takes the raw headers from a GET request and puts them
// onto an empty body
struct HeadResponse {
    headers: warp::http::HeaderMap,
}

impl Reply for HeadResponse {
    fn into_response(self) -> warp::reply::Response {
        let mut resp = warp::http::Response::new(warp::hyper::Body::empty());
        let headers = resp.headers_mut();
        *headers = self.headers;
        resp
    }
}
