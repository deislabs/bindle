use std::path::PathBuf;
use std::{net::SocketAddr, path::Path};

use bindle::signature::KeyRingSaver;
use clap::Parser;
use tracing::{debug, info, warn};

use bindle::{
    invoice::signature::{KeyRing, SignatureRole},
    provider, search,
    server::{server, TlsConfig},
    signature::{KeyEntry, KeyRingLoader, SecretKeyFile},
    SecretKeyEntry,
};

enum AuthType {
    /// Use Oidc with the given Client Id, issuer URL, and device URL (in that order)
    Oidc(String, String, String),
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

#[derive(Parser, serde::Deserialize, Default)]
#[clap(name = "bindle-server", version = clap::crate_version!(), author = "DeisLabs at Microsoft Azure", about = DESCRIPTION)]
struct Opts {
    #[clap(
        short = 'i',
        long = "address",
        env = "BINDLE_IP_ADDRESS_PORT",
        help = "the IP address and port to listen on [default: 127.0.0.1:8080]"
    )]
    address: Option<String>,
    #[clap(
        name = "bindle_directory",
        short = 'd',
        long = "directory",
        env = "BINDLE_DIRECTORY",
        help = "the path to the directory in which bindles will be stored [default: $XDG_DATA_HOME/bindle]"
    )]
    bindle_directory: Option<PathBuf>,
    #[clap(
        name = "cert_path",
        short = 'c',
        long = "tls-cert",
        env = "BINDLE_TLS_CERT",
        requires = "key_path",
        help = "the path to the TLS certificate to use. If set, --key-path must be set as well. If not set, the server will use HTTP"
    )]
    cert_path: Option<PathBuf>,
    #[clap(
        name = "key_path",
        short = 'k',
        long = "tls-key",
        env = "BINDLE_TLS_KEY",
        requires = "cert_path",
        help = "the path to the TLS certificate key to use. If set, --cert-path must be set as well. If not set, the server will use HTTP"
    )]
    key_path: Option<PathBuf>,
    #[clap(
        name = "config_file",
        long = "config-path",
        help = "the path to a configuration file"
    )]
    config_file: Option<PathBuf>,
    #[clap(
        name = "keyring",
        short = 'r',
        long = "keyring",
        help = "the path to the public keyring file used for verifying signatures"
    )]
    keyring_file: Option<PathBuf>,

    #[clap(
        name = "signing_keys",
        long = "signing-keys",
        env = "BINDLE_SIGNING_KEYS",
        help = "location of the TOML file that holds the signing keys used for creating signatures"
    )]
    signing_file: Option<PathBuf>,

    #[clap(
        name = "verification_strategy",
        long = "strategy",
        env = "BINDLE_VERIFICATION_STRATEGY",
        help = "The verification strategy to use on the server. Must be one of: CreativeIntegrity, AuthoritativeIntegrity, GreedyVerification, ExhaustiveVerification, MultipleAttestation, MultipleAttestationGreedy. For either of the multiple attestation strategies, you can specify the roles using the following syntax: `MultipleAttestation[Creator, Approver]`"
    )]
    verification_strategy: Option<bindle::VerificationStrategy>,

    #[clap(
        name = "use_embedded_db",
        long = "use-embedded-db",
        short = 'e',
        env = "BINDLE_USE_EMBEDDED_DB",
        help = "Use the new embedded database provider. This is currently experimental, but fairly stable and more efficient. In the future, this will be the default"
    )]
    #[serde(default)]
    use_embedded_db: bool,

    #[clap(
        name = "htpasswd-file",
        long = "htpasswd-file",
        env = "BINDLE_HTPASSWD_FILE",
        help = "If set, this will turn on HTTP Basic Auth for Bindle and load the given htpasswd file. Use 'htpasswd -Bc' to create one."
    )]
    htpasswd_file: Option<PathBuf>,

    #[clap(
        name = "oidc-client-id",
        long = "oidc-client-id",
        env = "BINDLE_OIDC_CLIENT_ID",
        requires_all = &["oidc-device-url", "oidc-issuer-url"],
        help = "The OIDC client ID to use for Oauth2 token authentication"
    )]
    oidc_client_id: Option<String>,

    // TODO(thomastaylor312): This could be obtained purely by using the discovery endpoint, but
    // `device_authorization_endpoint` is not in the spec. If it seems like most major providers
    // support this, we could make this optional as a fallback
    #[clap(
        name = "oidc-device-url",
        long = "oidc-device-url",
        env = "BINDLE_OIDC_DEVICE_URL",
        requires_all = &["oidc-client-id", "oidc-issuer-url"],
        help = "The URL to the device code authentication for your OIDC provider"
    )]
    oidc_device_url: Option<String>,

    #[clap(
        name = "oidc-issuer-url",
        long = "oidc-issuer-url",
        env = "BINDLE_OIDC_ISSUER_URL",
        requires_all = &["oidc-device-url", "oidc-client-id"],
        help = "The URL of the OIDC issuer your tokens should be issued by. This is used for verification of the token and for OIDC discovery"
    )]
    oidc_issuer_url: Option<String>,

    #[clap(
        name = "unauthenticated",
        long = "unauthenticated",
        help = "Run server in development mode"
    )]
    #[serde(default)]
    unauthenticated: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO: Allow log level setting outside of RUST_LOG (this is easier with this subscriber)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_ansi(atty::is(atty::Stream::Stderr))
        .init();

    let config = merged_opts().await?;

    let addr: SocketAddr = config
        .address
        .unwrap_or_else(|| String::from("127.0.0.1:8080"))
        .parse()?;

    let bindle_directory: PathBuf = config.bindle_directory.unwrap_or_else(|| {
        dirs::data_dir()
            .expect("Unable to infer data directory")
            .join("bindle")
    });

    let keyring_file: PathBuf = config
        .keyring_file
        .unwrap_or_else(|| default_config_dir().join("keyring.toml"));

    // We might want to do something different in the future. But what we do here is
    // load the file if we can find it. If the file just doesn't exist, we print a
    // warning and load a placeholder. This prevents the program from failing when
    // a keyring does not exist.
    //
    // All other cases are considered errors worthy of failing.
    let mut keyring: KeyRing = match tokio::fs::metadata(&keyring_file).await {
        Ok(md) if md.is_file() => keyring_file.load().await?,
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
    let signing_keys: PathBuf = match config.signing_file {
        Some(keypath) => {
            debug!(path = %keypath.display(), "Signing keys file was set, loading...");
            keypath
        }
        None => {
            debug!("No signing key file set, attempting to load from default");
            ensure_signing_keys(&mut keyring, &keyring_file).await?
        }
    };

    // Map doesn't work here because we've already moved data out of opts
    #[allow(clippy::manual_map)]
    let tls = match config.cert_path {
        None => None,
        Some(p) => Some(TlsConfig {
            cert_path: p,
            key_path: config
                .key_path
                .expect("--key-path should be set if --cert-path was set"),
        }),
    };

    let strategy = config.verification_strategy.unwrap_or_default();

    tracing::info!("Using verification strategy of {:?}", strategy);

    let index = search::StrictEngine::default();
    let secret_store = SecretKeyFile::load_file(&signing_keys).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to load secret key file from {}: {} HINT: Try the flag --signing-keys",
            signing_keys.display(),
            e
        )
    })?;

    // If there are any keys we use for signing, we should trust them in our keychain
    keyring.key.extend(
        secret_store
            .key
            .iter()
            .map(|sk| KeyEntry::try_from(sk.clone()))
            .collect::<Result<Vec<_>, _>>()?,
    );

    tracing::log::info!(
        "Starting server at {}, and serving bindles from {}",
        addr.to_string(),
        bindle_directory.display()
    );

    let auth_method = if config.oidc_client_id.is_some() {
        // We can unwrap safely here because Clap checks that all args exist and we already
        // checked that one of them exists
        AuthType::Oidc(
            config.oidc_client_id.unwrap(),
            config.oidc_issuer_url.unwrap(),
            config.oidc_device_url.unwrap(),
        )
    } else if let Some(htpasswd) = config.htpasswd_file {
        AuthType::HttpBasic(htpasswd)
    } else if config.unauthenticated {
        AuthType::None
    } else {
        anyhow::bail!(
            "An authentication method must be specified.  Use --unauthenticated to run server without authentication"
        );
    };

    // TODO: This is really gnarly, but the associated type on `Authenticator` makes turning it into
    // a Boxed dynner really difficult. I also tried rolling our own type erasure and ran into
    // similar issues (though I think it could be fixed, it would be a lot of code). So we might
    // have to resort to some sort of dependency injection here. The same goes for providers as the
    // methods have generic parameters
    match (config.use_embedded_db, auth_method) {
        // Embedded DB and oidc auth
        (true, AuthType::Oidc(client_id, issuer, token_url)) => {
            warn!("Using EmbeddedProvider. This is currently experimental");
            info!("Using OIDC token authentication");
            let store =
                provider::embedded::EmbeddedProvider::new(&bindle_directory, index.clone()).await?;

            let authn =
                bindle::authn::oidc::OidcAuthenticator::new(&issuer, &token_url, &client_id)
                    .await?;
            server(
                store,
                index,
                authn,
                bindle::authz::anonymous_get::AnonymousGet,
                addr,
                tls,
                secret_store,
                strategy,
                keyring,
            )
            .await
        }
        // Embedded DB and no auth
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
        // File system and oidc auth
        (false, AuthType::Oidc(client_id, issuer, token_url)) => {
            info!("Using FileProvider");
            info!("Using OIDC token authentication");
            let store = provider::file::FileProvider::new(&bindle_directory, index.clone()).await;

            let authn =
                bindle::authn::oidc::OidcAuthenticator::new(&issuer, &token_url, &client_id)
                    .await?;
            server(
                store,
                index,
                authn,
                bindle::authz::anonymous_get::AnonymousGet,
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
                bindle::authz::anonymous_get::AnonymousGet,
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
                bindle::authz::anonymous_get::AnonymousGet,
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

/// Makes sure signing keys exist for the host. If it generates a key, it will add it to the current keyring and save it to the path
async fn ensure_signing_keys(
    keyring: &mut KeyRing,
    keyring_path: &Path,
) -> anyhow::Result<PathBuf> {
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
            let key = SecretKeyEntry::new("Default host key", vec![SignatureRole::Host]);
            default_keyfile.key.push(key.clone());
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
            keyring.add_entry(key.try_into()?);
            keyring_path.save(keyring).await.map_err(|e| {
                anyhow::anyhow!(
                    "Unable to save newly created key to keyring {}: {}",
                    keyring_path.display(),
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

async fn merged_opts() -> anyhow::Result<Opts> {
    let opts = Opts::parse();

    // load config file if it exists
    let config_file_path = match opts.config_file.clone() {
        Some(c) => c,
        None => default_config_file()
            .ok_or_else(|| anyhow::anyhow!("could not find a default config path"))?,
    };

    let config: Opts = load_toml(config_file_path).await.unwrap_or_else(|e| {
        warn!(error = %e, "No config file loaded");
        Opts::default()
    });

    Ok(Opts {
        address: opts.address.or(config.address),
        bindle_directory: opts.bindle_directory.or(config.bindle_directory),
        cert_path: opts.cert_path.or(config.cert_path),
        config_file: opts.config_file,
        htpasswd_file: opts.htpasswd_file.or(config.htpasswd_file),
        unauthenticated: opts.unauthenticated || config.unauthenticated,
        key_path: opts.key_path.or(config.key_path),
        keyring_file: opts.keyring_file.or(config.keyring_file),
        oidc_client_id: opts.oidc_client_id.or(config.oidc_client_id),
        oidc_device_url: opts.oidc_device_url.or(config.oidc_device_url),
        oidc_issuer_url: opts.oidc_issuer_url.or(config.oidc_issuer_url),
        signing_file: opts.signing_file.or(config.signing_file),
        use_embedded_db: opts.use_embedded_db || config.use_embedded_db,
        verification_strategy: opts.verification_strategy.or(config.verification_strategy),
    })
}

async fn load_toml<T>(file: PathBuf) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    // MPB: The original version did an unwrap_or_default() on the read_to_string.
    // I removed this because I think we want an error to propagate if the file
    // cannot be read.
    let raw_data = tokio::fs::read(&file)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read TOML file {}: {}", file.display(), e))?;
    let res = toml::from_slice::<T>(&raw_data)?;
    Ok(res)
}
