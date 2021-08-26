use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Clap;
use tracing::{info, warn};

use bindle::{
    invoice::signature::{KeyRing, SignatureRole},
    provider, search,
    server::{server, TlsConfig},
    signature::SecretKeyFile,
    SecretKeyEntry,
};

enum AuthType {
    /// Use GitHub with the given Client Id and Secret (in that order)
    Github(String, String),
    /// Use an HTPassword file at the given path
    HttpBasic(PathBuf),
    /// Do not perform auth.
    None,
}

const DESCRIPTION: &str = r#"
The Bindle Server

Bindle is a technology for storing and retrieving aggregate applications.
This program runs an HTTP frontend for a Bindle repository.
"#;

#[derive(Clap, serde::Deserialize, Default)]
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
        long = "tls-cert",
        env = "BINDLE_TLS_CERT",
        requires = "key_path",
        about = "the path to the TLS certificate to use. If set, --key-path must be set as well. If not set, the server will use HTTP"
    )]
    cert_path: Option<PathBuf>,
    #[clap(
        name = "key_path",
        short = 'k',
        long = "tls-key",
        env = "BINDLE_TLS_KEY",
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
        about = "the path to the public keyring file used for verifying signatures"
    )]
    keyring_file: Option<PathBuf>,

    #[clap(
        name = "signing_keys",
        long = "signing-keys",
        env = "BINDLE_SIGNING_KEYS",
        about = "location of the TOML file that holds the signing keys used for creating signatures"
    )]
    signing_file: Option<PathBuf>,

    #[clap(
        name = "verification_strategy",
        long = "strategy",
        env = "BINDLE_VERIFICATION_STRATEGY",
        about = "The verification strategy to use on the server. Must be one of: CreativeIntegrity, AuthoritativeIntegrity, GreedyVerification, ExhaustiveVerification, MultipleAttestation, MultipleAttestationGreedy. For either of the multiple attestation strategies, you can specify the roles using the following syntax: `MultipleAttestation[Creator, Approver]`"
    )]
    verification_strategy: Option<bindle::VerificationStrategy>,

    #[clap(
        name = "use_embedded_db",
        long = "use-embedded-db",
        short = 'e',
        env = "BINDLE_USE_EMBEDDED_DB",
        about = "Use the new embedded database provider. This is currently experimental, but fairly stable and more efficient. In the future, this will be the default"
    )]
    use_embedded_db: bool,

    #[clap(
        name = "github-client-id",
        long = "github-client-id",
        env = "BINDLE_GITHUB_CLIENT_ID",
        requires = "github-client-secret",
        about = "The Github client ID to use for Oauth2 token authentication. --github-client-secret should be set as well. Please note this flag is likely to be changed or removed in future releases"
    )]
    github_client_id: Option<String>,

    #[clap(
        name = "github-client-secret",
        long = "github-client-secret",
        env = "BINDLE_GITHUB_CLIENT_SECRET",
        requires = "github-client-id",
        about = "The Github client secret to use for Oauth2 token authentication. --github-client-id should be set as well. Please note this flag is likely to be changed or removed in future releases"
    )]
    github_client_secret: Option<String>,

    #[clap(
        name = "htpasswd-file",
        long = "htpasswd-file",
        env = "BINDLE_HTPASSWD_FILE",
        about = "If set, this will turn on HTTP Basic Auth for Bindle and load the given htpasswd file. Use 'htpasswd -Bc' to create one."
    )]
    htpasswd_file: Option<PathBuf>,
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

    let config: Opts = load_toml(config_file_path).await.unwrap_or_else(|e| {
        warn!(error = %e, "No server.toml file loaded");
        Opts::default()
    });

    // find socket address
    //   1. cli options if set
    //   2. config file if set
    //   3. default
    let addr: SocketAddr = opts
        .address
        .or(config.address)
        .unwrap_or_else(|| String::from("127.0.0.1:8080"))
        .parse()?;

    // find bindle directory
    //   1. cli options if set
    //   2. config file if set
    //   3. default
    let bindle_directory: PathBuf = opts
        .bindle_directory
        .or(config.bindle_directory)
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
        .or(config.keyring_file)
        .unwrap_or_else(|| default_config_dir().join("keyring.toml"));

    // We might want to do something different in the future. But what we do here is
    // load the file if we can find it. If the file just doesn't exist, we print a
    // warning and load a placeholder. This prevents the program from failing when
    // a keyring does not exist.
    //
    // All other cases are considered errors worthy of failing.
    let keyring: KeyRing = match std::fs::metadata(&keyring_file) {
        Ok(md) if md.is_file() => load_toml(keyring_file).await?,
        Ok(_) => {
            anyhow::bail!("Expected {} to be a regular file", keyring_file.display());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("No keyring.toml found. Using default keyring.");
            KeyRing::default()
        }
        Err(e) => anyhow::bail!("failed to read file {}: {}", keyring_file.display(), e),
    };

    // Load the signing keys from...
    // - --signing-keys filename
    // - or config file signing-keys entry
    // - or $XDG_DATA/bindle/signing-keys.toml
    let signing_keys_config: Option<PathBuf> = opts.signing_file.or(config.signing_file);

    let signing_keys = match signing_keys_config {
        Some(keypath) => keypath,
        None => ensure_signing_keys().await?,
    };

    let cert_path = opts.cert_path.or(config.cert_path);

    let key_path = opts.key_path.or(config.key_path);

    // Map doesn't work here because we've already moved data out of opts
    #[allow(clippy::manual_map)]
    let tls = match cert_path {
        None => None,
        Some(p) => Some(TlsConfig {
            cert_path: p,
            key_path: key_path.expect("--key-path should be set if --cert-path was set"),
        }),
    };

    let strategy = opts
        .verification_strategy
        .or(config.verification_strategy)
        .unwrap_or_default();

    tracing::info!("Using verification strategy of {:?}", strategy);

    let index = search::StrictEngine::default();
    let secret_store = SecretKeyFile::load_file(&signing_keys).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to load secret key file from {}: {} HINT: Try the flag --signing-keys",
            signing_keys.display(),
            e
        )
    })?;

    tracing::log::info!(
        "Starting server at {}, and serving bindles from {}",
        addr.to_string(),
        bindle_directory.display()
    );

    let auth_method = if opts.github_client_id.is_some() {
        AuthType::Github(
            opts.github_client_id.unwrap(),
            opts.github_client_secret.unwrap(),
        )
    } else if let Some(htpasswd) = opts.htpasswd_file {
        AuthType::HttpBasic(htpasswd)
    } else {
        AuthType::None
    };

    // TODO: This is really gnarly, but the associated type on `Authenticator` makes turning it into
    // a Boxed dynner really difficult. I also tried rolling our own type erasure and ran into
    // similar issues (though I think it could be fixed, it would be a lot of code). So we might
    // have to resort to some sort of dependency injection here. The same goes for providers as the
    // methods have generic parameters
    match (opts.use_embedded_db, auth_method) {
        // Embedded DB and GH auth
        (true, AuthType::Github(id, secret)) => {
            warn!("Using EmbeddedProvider. This is currently experimental");
            info!("Using Github Oauth token authentication");
            let store =
                provider::embedded::EmbeddedProvider::new(&bindle_directory, index.clone()).await?;
            // We can unwrap safely here because Clap checks that both args exist and we already
            // checked that one of them exists
            let authn = bindle::authn::github::GithubAuthenticator::new(&id, &secret)?;
            server(
                store,
                index,
                authn,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // Embedded DB and no GH auth
        (true, AuthType::None) => {
            warn!("Using EmbeddedProvider. This is currently experimental");
            let store =
                provider::embedded::EmbeddedProvider::new(&bindle_directory, index.clone()).await?;
            server(
                store,
                index,
                bindle::authn::always::AlwaysAuthenticate,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // File system and GH auth
        (false, AuthType::Github(id, secret)) => {
            info!("Using FileProvider");
            info!("Using Github Oauth token authentication");
            let store = provider::file::FileProvider::new(&bindle_directory, index.clone()).await;
            // We can unwrap safely here because Clap checks that both args exist and we already
            // checked that one of them exists
            let authn = bindle::authn::github::GithubAuthenticator::new(&id, &secret)?;
            server(
                store,
                index,
                authn,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // File system and no GH auth
        (false, AuthType::None) => {
            info!("Using FileProvider");
            let store = provider::file::FileProvider::new(&bindle_directory, index.clone()).await;
            server(
                store,
                index,
                bindle::authn::always::AlwaysAuthenticate,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // DB with HttpBasic
        (true, AuthType::HttpBasic(filename)) => {
            warn!("Using EmbeddedProvider. This is currently experimental");
            info!("Auth mode: HTTP Basic Auth");
            let store =
                provider::embedded::EmbeddedProvider::new(&bindle_directory, index.clone()).await?;
            let authn = bindle::authn::http_basic::HttpBasic::from_file(filename).await?;
            server(
                store,
                index,
                authn,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // File system with HttpBasic
        (false, AuthType::HttpBasic(filename)) => {
            info!("Using FileProvider");
            info!("Auth mode: HTTP Basic Auth");
            let authn = bindle::authn::http_basic::HttpBasic::from_file(filename).await?;
            let store = provider::file::FileProvider::new(&bindle_directory, index.clone()).await;
            server(
                store,
                index,
                authn,
                bindle::authz::always::AlwaysAuthorize,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
    }
}

fn default_config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|v| v.join("bindle/server.toml"))
}
fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|v| v.join("bindle/"))
        .unwrap_or_else(|| "./bindle".into())
}

async fn ensure_config_dir() -> anyhow::Result<PathBuf> {
    let dir = default_config_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| anyhow::anyhow!("Unable to create config dir at {}: {}", dir.display(), e))?;
    Ok(dir)
}

async fn ensure_signing_keys() -> anyhow::Result<PathBuf> {
    let base = ensure_config_dir().await?;
    let signing_keyfile = base.join("signing-keys.toml");

    // Stat it, and if it exists we are good.
    match tokio::fs::metadata(&signing_keyfile).await {
        Ok(info) if info.is_file() => Ok(signing_keyfile),
        Ok(_info) => Err(anyhow::anyhow!("Not a file: {}", signing_keyfile.display())),
        // If the file is not found, then drop through and create a default instance
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut default_keyfile = SecretKeyFile::default();
            warn!(
                "Creating a default host signing key and storing it in {}",
                signing_keyfile.display()
            );
            let key = SecretKeyEntry::new("Default host key".to_owned(), vec![SignatureRole::Host]);
            default_keyfile.key.push(key);
            default_keyfile
                .save_file(&signing_keyfile)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Unable to save newly created key to {}: {}",
                        signing_keyfile.display(),
                        e
                    )
                })?;
            Ok(signing_keyfile)
        }
        Err(e) => Err(anyhow::anyhow!(
            "Failed to load singing keys at {}: {}",
            signing_keyfile.display(),
            e
        )),
    }
}

async fn load_toml<T>(file: PathBuf) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    // MPB: The original version did an unwrap_or_default() on the read_to_string.
    // I removed this because I think we want an error to propogate if the file
    // cannot be read.
    let raw_data = tokio::fs::read(&file)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read TOML file {}: {}", file.display(), e))?;
    let res = toml::from_slice::<T>(&raw_data)?;
    Ok(res)
}
