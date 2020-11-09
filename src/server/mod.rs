mod filters;
mod handlers;
mod reply;
mod routes;

use std::net::SocketAddr;
use std::sync::Arc;

use super::storage::Storage;
use crate::search::Search;

use tokio::sync::RwLock;
use warp::Filter;

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
    let api = warp::path("v1").and(
        routes::v1::query(index.clone())
            .or(routes::v1::create(store.clone()))
            .or(routes::v1::get(store.clone()))
            .or(routes::v1::head(store.clone()))
            .or(routes::v1::yank(store)),
    );

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
