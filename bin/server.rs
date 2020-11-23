use std::net::SocketAddr;

use clap::{App, Arg};

use bindle::{search, server::server, storage};

const DESCRIPTION: &str = r#"
The Bindle Server

Bindle is a technology for storing and retrieving aggregate applications.
This program runs an HTTP frontend for a Bindle repository.
"#;

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let app = App::new("bindle-server")
        .version("0.1.0")
        .author("DeisLabs at Microsoft Azure")
        .about(DESCRIPTION)
        .arg(
            Arg::new("address")
                .short('i')
                .long("address")
                .value_name("IP_ADDRESS_PORT")
                .about("the IP address and port to listen on")
                .takes_value(true),
        )
        .arg(
            Arg::new("dir")
                .short('d')
                .long("directory")
                .value_name("PATH")
                .about("the path to the directory in which bindles will be stored")
                .takes_value(true),
        )
        .get_matches();

    let raw_addr = app.value_of("addr").unwrap_or("127.0.0.1:8080");
    let dir = app.value_of("dir").unwrap_or("/tmp");
    let addr: SocketAddr = raw_addr.parse()?;
    let index = search::StrictEngine::default();
    let store = storage::file::FileStorage::new(dir, index.clone()).await;

    log::info!(
        "Starting server at {}, and serving bindles from {}",
        raw_addr,
        dir
    );
    server(store, index, addr).await
}
