use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Clap;

use bindle::{
    invoice::signature::KeyRing,
    provider, search,
    server::{server, TlsConfig},
    signature::SecretKeyFile,
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
        about = "the path to the directory in which bindles will be stored [default: $XDG_DATA_HOME/bindle]"
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
    #[clap(
        name = "keyring",
        short = 'r',
        long = "keyring",
        about = "the path to the keyring file"
    )]
    keyring_file: Option<PathBuf>,

    #[clap(
        name = "signing_keys",
        long = "signing-keys",
        env = "BINDLE_SIGNING_KEYS",
        about = "location of the TOML file that holds the signing keys"
    )]
    signing_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    // TODO: Allow log level setting outside of RUST_LOG (this is easier with this subscriber)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // load config file if it exists
    let config_file_path = match opts.config_file {
        Some(c) => c,
        None => default_config_file()
            .ok_or_else(|| anyhow::anyhow!("could not find a default config path"))?,
    };

    let config: toml::Value = load_toml(config_file_path).await.unwrap_or_else(|_| {
        println!("No server.toml file loaded");
        toml::Value::Table(toml::value::Table::new())
    });

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
        .unwrap_or_else(|| {
            dirs::data_dir()
                .expect("Unable to infer data directory")
                .join("bindle")
        });

    // find bindle directory
    //   1. cli options if set
    //   2. config file if set
    //   3. default
    //   4. hardcoded `./.bindle/keyring.toml`
    // TODO: Should we ensure a keyring?
    let keyring_file: PathBuf = opts
        .keyring_file
        .or_else(|| {
            config
                .get("keyring")
                .map(|v| v.as_str().unwrap().parse().unwrap())
        })
        .unwrap_or(PathBuf::from(
            default_config_dir()
                .unwrap_or_else(|| PathBuf::from("./bindle"))
                .join("keyring.toml"),
        ));

    // We might want to do something different in the future. But what we do here is
    // load the file if we can find it. If the file just doesn't exist, we print a
    // warning and load a placeholder. This prevents the program from failing when
    // a keyring does not exist.
    //
    // All other cases are considered errors worthy of failing.
    let keyring: KeyRing = match std::fs::metadata(&keyring_file) {
        Ok(md) if md.is_file() => load_toml(keyring_file).await?,
        Ok(_) => Err(anyhow::anyhow!(
            "Expected {} to be a regular file",
            keyring_file.display()
        ))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("No keyring.toml found.");
            KeyRing::default()
        }
        Err(e) => anyhow::bail!("failed to read file {}: {}", keyring_file.display(), e),
    };

    let signing_keys: PathBuf = opts
        .signing_file
        .or_else(|| {
            config
                .get("signing-keys")
                .map(|v| v.to_string().parse().unwrap())
        })
        .unwrap_or(PathBuf::from(
            default_config_dir()
                .unwrap_or_else(|| PathBuf::from("./bindle"))
                .join("signing-keys.toml"),
        ));

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
    let store = provider::file::FileProvider::new(&bindle_directory, index.clone(), keyring).await;
    let secret_store = SecretKeyFile::load_file(signing_keys.clone())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to load secret key file from {}: {}",
                signing_keys.display(),
                e
            )
        })?;

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
        secret_store,
    )
    .await
}

fn default_config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|v| v.join("bindle/server.toml"))
}
fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|v| v.join("bindle/"))
}

async fn load_toml<T>(file: PathBuf) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    // MPB: The original version did an unwrap_or_default() on the read_to_string.
    // I removed this because I think we want an error to propogate if the file
    // cannot be read.
    let raw_data = tokio::fs::read_to_string(&file)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read TOML file {}: {}", file.display(), e))?;
    let res = toml::from_str::<T>(&raw_data)?;
    Ok(res)
}
