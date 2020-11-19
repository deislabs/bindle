mod filters;
mod handlers;
mod reply;

pub mod routes;
pub use filters::QueryOptions;
pub use reply::InvoiceCreateResponse;

pub(crate) mod stream_util;

use std::net::SocketAddr;
use std::sync::Arc;

use super::storage::Storage;
use crate::search::Search;

use tokio::sync::RwLock;

pub(crate) const TOML_MIME_TYPE: &str = "application/toml";

pub async fn server<S, I>(
    store: S,
    index: Arc<RwLock<I>>,
    addr: impl Into<SocketAddr> + 'static,
) -> anyhow::Result<()>
where
    S: Storage + Clone + Send + Sync + 'static,
    I: Search + Send + Sync + 'static,
{
    // V1 API paths, currently the only version
    let api = routes::api(store, index);

    // TODO: We'll have to change this to serve_incoming_with_graceful_shutdown when we setup TLS
    let (_, serv) = warp::serve(api).try_bind_with_graceful_shutdown(addr, shutdown_signal())?;

    serv.await;
    Ok(())
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to setup signal handler");
}
