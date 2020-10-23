mod routes;
mod handlers;

use std::net::SocketAddr;

use super::storage::Storage;

use warp::Filter;

async fn server<S: Storage + Clone + Send + Sync + 'static>(store: S, addr: impl Into<SocketAddr> + 'static) -> anyhow::Result<()> {
    // V1 API paths, currently the only version
    let api = routes::v1::list(store.clone()).or(routes::v1::create(store.clone())).or(routes::v1::get(store.clone())).or(routes::v1::yank(store));

    // TODO: We'll have to change this to serve_incoming_with_graceful_shutdown when we setup TLS
    let (_, serv) = warp::serve(api).try_bind_with_graceful_shutdown(addr, shutdown_signal())?;

    serv.await;
    Ok(())
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}
