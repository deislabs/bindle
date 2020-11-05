use std::net::SocketAddr;

use bindle::{search, server, storage::file::FileStorage};

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    let store = FileStorage::new("/tmp", search::StrictEngine::default());
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    server(store, addr).await
}
