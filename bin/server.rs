use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Clap;

use bindle::{
    provider, search,
    server::{server, TlsConfig},
};

const DESCRIPTION: &str = r#"
The Bindle Server

Bindle is a technology for storing and retrieving aggregate applications.
This program runs an HTTP frontend for a Bindle repository.
"#;

#[derive(Clap)]
#[clap(name = "bindle-server", version = clap::crate_version!(), author = "DeisLabs at Microsoft Azure", about = DESCRIPTION)]
struct Opts {
    #[clap(
        short = 'i',
        long = "address",
        env = "BINDLE_IP_ADDRESS_PORT",
        default_value = "127.0.0.1:8080",
        about = "the IP address and port to listen on"
    )]
    address: String,
    #[clap(
        name = "bindle_directory",
        short = 'd',
        long = "directory",
        env = "BINDLE_DIRECTORY",
        default_value = "/tmp",
        about = "the path to the directory in which bindles will be stored"
    )]
    bindle_directory: PathBuf,
    #[clap(
        name = "cert_path",
        short = 'c',
        long = "cert-path",
        env = "BINDLE_CERT_PATH",
        requires = "key_path",
        about = "the path to the TLS certificate to use. If set, --key-path must be set as well. If not set, the server will use HTTP"
    )]
    cert_path: Option<PathBuf>,
    #[clap(
        name = "key_path",
        short = 'k',
        long = "key-path",
        env = "BINDLE_KEY_PATH",
        requires = "cert_path",
        about = "the path to the TLS certificate key to use. If set, --cert-path must be set as well. If not set, the server will use HTTP"
    )]
    key_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    env_logger::init();

    let addr: SocketAddr = opts.address.parse()?;
    let index = search::StrictEngine::default();
    let store = provider::file::FileProvider::new(&opts.bindle_directory, index.clone()).await;

    log::info!(
        "Starting server at {}, and serving bindles from {}",
        addr.to_string(),
        opts.bindle_directory.display()
    );

    // Map doesn't work here because we've already moved data out of opts
    let tls = match opts.cert_path {
        None => None,
        Some(p) => Some(TlsConfig {
            cert_path: p,
            key_path: opts
                .key_path
                .expect("--key-path should be set if --cert-path was set"),
        }),
    };
    server(
        store,
        index,
        bindle::authn::always::AlwaysAuthenticate,
        bindle::authz::always::AlwaysAuthorize,
        addr,
        tls,
    )
    .await
}
