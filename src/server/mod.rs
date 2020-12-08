mod filters;
mod handlers;
mod reply;

pub mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;

use super::storage::Storage;
use crate::search::Search;

pub(crate) const TOML_MIME_TYPE: &str = "application/toml";

/// The configuration required for running with TLS enabled
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Returns a future that runs a server until it receives a SIGINT to stop. If optional TLS
/// configuration is given, the server will be configured to use TLS. Otherwise it will use plain
/// HTTP
pub async fn server<S, I>(
    store: S,
    index: I,
    addr: impl Into<SocketAddr> + 'static,
    tls: Option<TlsConfig>,
) -> anyhow::Result<()>
where
    S: Storage + Clone + Send + Sync + 'static,
    I: Search + Clone + Send + Sync + 'static,
{
    // V1 API paths, currently the only version
    let api = routes::api(store, index);

    let server = warp::serve(api);
    match tls {
        None => {
            server
                .try_bind_with_graceful_shutdown(addr, shutdown_signal())?
                .1
                .await
        }
        Some(config) => {
            server
                .tls()
                .key_path(config.key_path)
                .cert_path(config.cert_path)
                .bind_with_graceful_shutdown(addr, shutdown_signal())
                .1
                .await
        }
    };
    Ok(())
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to setup signal handler");
}
