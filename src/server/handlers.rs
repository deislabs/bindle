use std::convert::Infallible;

use log::trace;
use warp::Reply;

use super::filters::InvoiceQuery;
use super::reply;
use crate::provider::Provider;
use crate::search::Search;

pub mod v1 {
    use super::*;

    use crate::QueryOptions;
    use tokio_stream::{self as stream, StreamExt};

    //////////// Invoice Functions ////////////
    pub async fn query_invoices<S: Search>(
        options: QueryOptions,
        index: S,
    ) -> Result<impl warp::Reply, Infallible> {
        trace!("Query invoice request with options: {:?}", options);
        let term = options.query.clone().unwrap_or_default();
        let version = options.version.clone().unwrap_or_default();
        let matches = match index.query(term, version, options.into()).await {
            Ok(m) => m,
            Err(e) => {
                trace!("Got bad query request: {:?}", e);
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
        };

        Ok(warp::reply::with_status(
            reply::toml(&matches),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn create_invoice<P: Provider>(
        store: P,
        inv: crate::Invoice,
    ) -> Result<impl warp::Reply, Infallible> {
        trace!("Create invoice request with invoice: {:?}", inv);
        let labels = match store.create_invoice(&inv).await {
            Ok(l) => l,
            Err(e) => {
                return Ok(reply::into_reply(e));
            }
        };
        // If there are missing parcels that still need to be created, return a 202 to indicate that
        // things were accepted, but will not be fetchable until further action is taken
        if !labels.is_empty() {
            trace!(
                "Newly created invoice {:?} is missing {} parcels",
                inv.bindle.id,
                labels.len()
            );
            Ok(warp::reply::with_status(
                reply::toml(&crate::InvoiceCreateResponse {
                    invoice: inv,
                    missing: Some(labels),
                }),
                warp::http::StatusCode::ACCEPTED,
            ))
        } else {
            trace!(
                "Newly created invoice {:?} has all existing parcels",
                inv.bindle.id
            );
            Ok(warp::reply::with_status(
                reply::toml(&crate::InvoiceCreateResponse {
                    invoice: inv,
                    missing: None,
                }),
                warp::http::StatusCode::CREATED,
            ))
        }
    }

    pub async fn get_invoice<P: Provider + Sync>(
        id: String,
        query: InvoiceQuery,
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!(
            "Get invoice request for {} with yanked = {}",
            id,
            query.yanked.unwrap_or_default()
        );
        let res = if query.yanked.unwrap_or_default() {
            store.get_yanked_invoice(id)
        } else {
            store.get_invoice(id)
        };
        let inv = match res.await {
            Ok(i) => i,
            Err(e) => {
                trace!("Got error during get invoice request: {:?}", e);
                return Ok(Box::new(reply::into_reply(e)));
            }
        };
        Ok(Box::new(warp::reply::with_status(
            reply::toml(&inv),
            warp::http::StatusCode::OK,
        )))
    }

    pub async fn yank_invoice<P: Provider>(
        tail: warp::path::Tail,
        store: P,
    ) -> Result<impl warp::Reply, Infallible> {
        let id = tail.as_str();
        trace!("Yank invoice request for {}", id);
        if let Err(e) = store.yank_invoice(id).await {
            trace!("Got error during yank invoice request: {:?}", e);
            return Ok(reply::into_reply(e));
        }

        let mut resp = std::collections::HashMap::new();
        resp.insert("message", "invoice yanked");
        Ok(warp::reply::with_status(
            reply::toml(&resp),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn head_invoice<P: Provider + Sync>(
        id: String,
        query: InvoiceQuery,
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Head invoice request for {}", id);
        let inv = get_invoice(id, query, store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok(Box::new(super::HeadResponse {
            headers: parts.headers,
        }))
    }

    //////////// Parcel Functions ////////////

    pub async fn create_parcel<P, B, D>(
        (bindle_id, sha): (String, String),
        body: B,
        store: P,
    ) -> Result<impl warp::Reply, Infallible>
    where
        P: Provider + Sync,
        B: stream::Stream<Item = Result<D, warp::Error>> + Send + Sync + Unpin + 'static,
        D: bytes::Buf + Send,
    {
        trace!("Create parcel request, checking if parcel exists in bindle");

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
            return Ok(reply::into_reply(e));
        }

        let mut resp = std::collections::HashMap::new();
        resp.insert("message", "parcel created");
        Ok(warp::reply::with_status(
            reply::toml(&resp),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn get_parcel<P: Provider + Sync>(
        (bindle_id, id): (String, String),
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Get parcel request for {}", id);
        // Get parcel label to ascertain content type and length, and validate that it does exist
        let label = match parcel_in_bindle(&store, &bindle_id, &id).await {
            Ok(l) => l,
            Err(e) => return Ok(Box::new(e)),
        };

        let data = match store.get_parcel(bindle_id, &id).await {
            Ok(reader) => reader,
            Err(e) => {
                return Ok(Box::new(reply::into_reply(e)));
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
        Ok(Box::new(warp::reply::with_status(
            resp,
            warp::http::StatusCode::OK,
        )))
    }

    pub async fn head_parcel<P: Provider + Sync>(
        (bindle_id, id): (String, String),
        store: P,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Head parcel request for {}", id);
        let inv = get_parcel((bindle_id, id), store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok(Box::new(super::HeadResponse {
            headers: parts.headers,
        }))
    }

    //////////// Relationship Functions ////////////

    pub async fn get_missing<P: Provider + Sync + Clone>(
        tail: warp::path::Tail,
        store: P,
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
            reply::toml(&crate::MissingParcelsResponse { missing }),
            warp::http::StatusCode::OK,
        ))
    }

    //////////// Helper Functions ////////////

    /// Fetches an invoice from the given store and checks that the given SHA exists within that
    /// invoice. Returns a result where the Error variant is a warp reply containing the error
    async fn parcel_in_bindle<P: Provider + Sync>(
        store: &P,
        bindle_id: &str,
        sha: &str,
    ) -> std::result::Result<crate::Label, warp::reply::WithStatus<crate::server::reply::Toml>>
    {
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
