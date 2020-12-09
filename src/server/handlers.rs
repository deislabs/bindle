use std::convert::Infallible;

use log::trace;
use warp::Reply;

use super::filters::InvoiceQuery;
use super::reply;
use crate::search::Search;
use crate::storage::Storage;

pub mod v1 {
    use super::*;

    use std::io::Read;

    use crate::QueryOptions;
    use bytes::buf::BufExt;
    use tokio::stream::StreamExt;
    use tokio_util::codec::{BytesCodec, FramedRead};

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

    pub async fn create_invoice<S: Storage>(
        store: S,
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

    pub async fn get_invoice<S: Storage + Sync>(
        tail: warp::path::Tail,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        let id = tail.as_str();
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
                return Ok(reply::into_reply(e));
            }
        };
        Ok(warp::reply::with_status(
            reply::toml(&inv),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn yank_invoice<S: Storage>(
        tail: warp::path::Tail,
        store: S,
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

    pub async fn head_invoice<S: Storage + Sync>(
        tail: warp::path::Tail,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        trace!("Head invoice request for {}", tail.as_str());
        let inv = get_invoice(tail, query, store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok(super::HeadResponse {
            headers: parts.headers,
        })
    }

    //////////// Parcel Functions ////////////

    pub async fn create_parcel<S: Storage>(
        store: S,
        mut data: warp::multipart::FormData,
    ) -> Result<impl warp::Reply, Infallible> {
        trace!("Create parcel request, beginning parse of multipart data");
        let label_part = match form_data_unwrapper(data.next().await) {
            Ok(p) => p,
            Err(e) => {
                trace!("Got error while parsing label data from multipart: {:?}", e);
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
        };

        let label = match parse_label(label_part).await {
            Ok(l) => l,
            Err(e) => {
                trace!("Got error while parsing label data from multipart: {:?}", e);
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
        };

        trace!("Got SHA {} from label", label.sha256);

        let file_part = match form_data_unwrapper(data.next().await) {
            Ok(p) => p,
            Err(e) => {
                trace!(
                    "Got error while parsing parcel data from multipart: {:?}",
                    e
                );
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
        };

        if data.next().await.is_some() {
            return Ok(reply::reply_from_error(
                "Found extra file data in stream. Only one parcel should be uploaded with its associated label",
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }

        if let Err(e) = store
            .create_parcel(
                &label,
                &mut crate::async_util::BodyReadBuffer(file_part.stream()),
            )
            .await
        {
            return Ok(reply::into_reply(e));
        }

        Ok(warp::reply::with_status(
            reply::toml(&label),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn get_parcel<S: Storage>(
        id: String,
        store: S,
    ) -> Result<Box<dyn warp::Reply>, Infallible> {
        trace!("Get parcel request for {}", id);
        // Get parcel label to ascertain content type and length, then get the actual data
        let label = match store.get_label(&id).await {
            Ok(l) => l,
            Err(e) => {
                trace!("Error while fetching label for {}", id);
                return Ok(Box::new(reply::into_reply(e)));
            }
        };

        let data = match store.get_parcel(&id).await {
            Ok(reader) => reader,
            Err(e) => {
                return Ok(Box::new(reply::into_reply(e)));
            }
        };

        let stream = FramedRead::new(data, BytesCodec::new());

        // TODO: If we start to use compression on the body, we'll need a new custom header for
        // _actual_ size of the parcel, so the client can reconstruct the label data from headers
        // without needing to read the whole (possibly large) file
        let resp = warp::http::Response::builder()
            .header(warp::http::header::CONTENT_TYPE, label.media_type)
            .header(warp::http::header::CONTENT_LENGTH, label.size)
            .body(hyper::Body::wrap_stream(stream))
            .unwrap();

        // Gotta box because this is not a toml reply type (which we use for sending error messages to the user)
        Ok(Box::new(warp::reply::with_status(
            resp,
            warp::http::StatusCode::OK,
        )))
    }

    pub async fn head_parcel<S: Storage>(
        id: String,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        trace!("Head parcel request for {}", id);
        let inv = get_parcel(id, store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok(super::HeadResponse {
            headers: parts.headers,
        })
    }

    //////////// Relationship Functions ////////////

    pub async fn get_missing<S: Storage + Sync + Clone>(
        tail: warp::path::Tail,
        store: S,
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
            .map(|p| (p, store.clone()))
            .map(|(p, store)| async move {
                // We can't use a filter_map with async, so we need to map first, then collect things with a filter
                match store.get_label(&p.label.sha256).await {
                    // If it exists, don't include it
                    Ok(_) => Ok(None),
                    Err(e) if matches!(e, crate::storage::StorageError::NotFound) => {
                        Ok(Some(p.label))
                    }
                    Err(e) => Err(e),
                }
            });

        let missing = match futures::future::join_all(missing_futures)
            .await
            .into_iter()
            .collect::<Result<Vec<Option<crate::Label>>, crate::storage::StorageError>>()
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

    /// unwraps the option result from a form data stream to avoid double unwrapping littered all
    /// over the code
    fn form_data_unwrapper(
        item: Option<Result<warp::multipart::Part, warp::Error>>,
    ) -> anyhow::Result<warp::multipart::Part> {
        let res = match item {
            Some(r) => r,
            None => return Err(anyhow::anyhow!("Unexpected end of data body")),
        };

        res.map_err(anyhow::Error::new)
    }

    async fn parse_label(part: warp::multipart::Part) -> anyhow::Result<crate::Label> {
        if part.content_type().unwrap_or_default() != crate::server::TOML_MIME_TYPE {
            return Err(anyhow::anyhow!(
                "Expected label of content type {}, found content type {}",
                crate::server::TOML_MIME_TYPE,
                part.content_type().unwrap_or_default()
            ));
        }
        let mut raw = Vec::new();

        // Read all the parts into a buffer
        let mut stream = part.stream();
        while let Some(buf) = stream.next().await {
            buf?.reader().read_to_end(&mut raw)?;
        }
        let label = toml::from_slice(&raw)?;

        Ok(label)
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
