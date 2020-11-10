use std::convert::Infallible;
use std::sync::Arc;

use tokio::sync::RwLock;
use warp::Reply;

use super::filters::InvoiceQuery;
use super::reply;
use crate::search::Search;
use crate::storage::Storage;

pub mod v1 {
    use super::*;

    use std::io::Read;

    use crate::server::filters::QueryOptions;
    use bytes::buf::BufExt;
    use tokio::stream::StreamExt;
    use tokio_util::codec::{BytesCodec, FramedRead};

    //////////// Invoice Functions ////////////
    pub async fn query_invoices<S: Search>(
        options: QueryOptions,
        index: Arc<RwLock<S>>,
    ) -> Result<impl warp::Reply, Infallible> {
        let term = options.query.clone().unwrap_or_default();
        let version = options.version.clone().unwrap_or_default();
        let locked_index = index.read().await;
        let matches = match locked_index.query(term, version, options.into()) {
            Ok(m) => m,
            Err(e) => {
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ))
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
        let labels = match store.create_invoice(&inv).await {
            Ok(l) => l,
            Err(e) => {
                return Ok(reply::into_reply(e));
            }
        };
        // If there are missing parcels that still need to be created, return a 202 to indicate that
        // things were accepted, but will not be fetchable until further action is taken
        if !labels.is_empty() {
            Ok(warp::reply::with_status(
                reply::toml(&reply::InvoiceCreateResponse {
                    invoice: inv,
                    missing: Some(labels),
                }),
                warp::http::StatusCode::ACCEPTED,
            ))
        } else {
            Ok(warp::reply::with_status(
                reply::toml(&reply::InvoiceCreateResponse {
                    invoice: inv,
                    missing: None,
                }),
                warp::http::StatusCode::CREATED,
            ))
        }
    }

    pub async fn get_invoice<S: Storage>(
        tail: warp::path::Tail,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        let id = tail.as_str();
        let res = if query.yanked.unwrap_or_default() {
            store.get_yanked_invoice(id)
        } else {
            store.get_invoice(id)
        };
        let inv = match res.await {
            Ok(i) => i,
            Err(e) => {
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
        if let Err(e) = store.yank_invoice(id).await {
            return Ok(reply::into_reply(e));
        }

        // Do this once we figure out what we actually need for the yank_invoice method on storage
        Ok(warp::reply::with_status(
            reply::toml(&String::from("message = \"invoice yanked\"")),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn head_invoice<S: Storage>(
        tail: warp::path::Tail,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
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
        let label_part = match form_data_unwrapper(data.next().await) {
            Ok(p) => p,
            Err(e) => {
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ))
            }
        };

        let label = match parse_label(label_part).await {
            Ok(l) => l,
            Err(e) => {
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ))
            }
        };

        let file_part = match form_data_unwrapper(data.next().await) {
            Ok(p) => p,
            Err(e) => {
                return Ok(reply::reply_from_error(
                    e,
                    warp::http::StatusCode::BAD_REQUEST,
                ))
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
                &mut crate::server::stream_util::BodyReadBuffer(file_part.stream()),
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
        // Get parcel label to ascertain content type, then get the actual data
        let label = match store.get_label(&id).await {
            Ok(l) => l,
            Err(e) => {
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

        let resp = warp::http::Response::builder()
            .header(warp::http::header::CONTENT_TYPE, label.media_type)
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
        let inv = get_parcel(id, store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        // TODO: This doesn't set content length properly (probably because of streams)
        Ok(super::HeadResponse {
            headers: parts.headers,
        })
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
