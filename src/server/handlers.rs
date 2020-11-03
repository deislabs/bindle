use std::convert::Infallible;

use warp::Reply;

use super::filters::InvoiceQuery;
use super::reply;
use crate::storage::Storage;

pub mod v1 {
    use super::*;

    pub async fn list_invoices<S: Storage>(store: S) -> Result<impl warp::Reply, Infallible> {
        Ok(reply::toml(&"yay".to_string()))
    }

    pub async fn create_invoice<S: Storage>(
        id: String,
        store: S,
        inv: crate::Invoice,
    ) -> Result<impl warp::Reply, Infallible> {
        let labels = match store.create_invoice(&inv).await {
            Ok(l) => l,
            // TODO: Actually inspect the error to return the right code
            Err(e) => {
                return Ok(warp::reply::with_status(
                    reply::toml(&format!("error = \"{}\"", e.to_string())),
                    warp::http::StatusCode::NOT_FOUND,
                ));
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
        id: String,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        let res = if query.yanked {
            store.get_yanked_invoice(id)
        } else {
            store.get_invoice(id)
        };
        let inv = match res.await {
            Ok(i) => i,
            // TODO: Actually inspect the error to return the right code
            Err(e) => {
                return Ok(warp::reply::with_status(
                    reply::toml(&format!("error = \"{}\"", e.to_string())),
                    warp::http::StatusCode::NOT_FOUND,
                ));
            }
        };
        Ok(warp::reply::with_status(
            reply::toml(&inv),
            warp::http::StatusCode::OK,
        ))
    }

    pub async fn yank_invoice<S: Storage>(
        id: String,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        // Do this once we figure out what we actually need for the yank_invoice method on storage
        Ok(reply::toml(&"yay".to_string()))
    }

    pub async fn head_invoice<S: Storage>(
        id: String,
        query: InvoiceQuery,
        store: S,
    ) -> Result<impl warp::Reply, Infallible> {
        let inv = get_invoice(id, query, store).await?;

        // Consume the response to we can take the headers
        let (parts, _) = inv.into_response().into_parts();

        Ok(super::HeadResponse {
            headers: parts.headers,
        })
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
