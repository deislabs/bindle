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
        about = "the IP address and port to listen on [default: 127.0.0.1:8080]"
    )]
    address: Option<String>,
    #[clap(
        name = "bindle_directory",
        short = 'd',
        long = "directory",
        env = "BINDLE_DIRECTORY",
        about = "the path to the directory in which bindles will be stored [default: /tmp]"
    )]
    bindle_directory: Option<PathBuf>,
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
    #[clap(
        name = "config_file",
        long = "config-path",
        about = "the path to a configuration file"
    )]
    config_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    // TODO: Allow log level setting outside of RUST_LOG (this is easier with this subscriber)
    tracing_subscriber::fmt::init();

    // load config file if it exists
    let config_file_path = match opts.config_file {
        Some(c) => c,
        None => default_config_file()
            .ok_or_else(|| anyhow::anyhow!("could not find a default config path"))?,
    };

    let config_file = tokio::fs::read_to_string(config_file_path)
        .await
        .unwrap_or_default();
    let config: toml::Value = toml::from_str(&config_file)?;

    // find socket address
    //   1. cli options if set
    //   2. config file if set
    //   3. default
    let addr: SocketAddr = opts
        .address
        .or_else(|| {
            config
                .get("address")
                .map(|v| v.as_str().unwrap().to_string())
        })
        .unwrap_or_else(|| String::from("127.0.0.1:8080"))
        .parse()?;

    // find bindle directory
    //   1. cli options if set
    //   2. config file if set
    //   3. default
    let bindle_directory: PathBuf = opts
        .bindle_directory
        .or_else(|| {
            config
                .get("bindle-directory")
                .map(|v| v.as_str().unwrap().parse().unwrap())
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let cert_path = opts.cert_path.or_else(|| {
        config
            .get("cert-path")
            .map(|v| v.to_string().parse().unwrap())
    });

    let key_path = opts.key_path.or_else(|| {
        config
            .get("key-path")
            .map(|v| v.to_string().parse().unwrap())
    });

    // Map doesn't work here because we've already moved data out of opts
    let tls = match cert_path {
        None => None,
        Some(p) => Some(TlsConfig {
            cert_path: p,
            key_path: key_path.expect("--key-path should be set if --cert-path was set"),
        }),
    };

    let index = search::StrictEngine::default();
    let store = provider::file::FileProvider::new(&bindle_directory, index.clone()).await;

    tracing::log::info!(
        "Starting server at {}, and serving bindles from {}",
        addr.to_string(),
        bindle_directory.display()
    );

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

fn default_config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|v| v.join("bindle/server.toml"))
}
