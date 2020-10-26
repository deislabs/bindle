use std::net::SocketAddr;

use bindle::{server, storage};

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    let storage = storage::FileStorage::new("/tmp");
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    server(storage, addr).await
}
