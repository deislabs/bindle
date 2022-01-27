use std::convert::TryInto;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bindle::client::{
    tokens::{HttpBasic, NoToken, OidcToken, TokenManager},
    Client, ClientBuilder, ClientError, Result,
};
use bindle::invoice::signature::{
    KeyRing, SecretKeyEntry, SecretKeyFile, SecretKeyStorage, SignatureRole,
};
use bindle::invoice::Invoice;
use bindle::provider::ProviderError;
use bindle::signature::KeyEntry;
use bindle::standalone::{StandaloneRead, StandaloneWrite};
use bindle::{
    cache::{Cache, DumbCache},
    provider::Provider,
};

use clap::Parser;
use sha2::Digest;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;
use tracing::log::{info, warn};

mod opts;

use opts::*;

#[tokio::main]
async fn main() {
    // Trap and format error messages using the proper value
    if let Err(e) = run().await.map_err(anyhow::Error::new) {
        eprintln!("{}", e);
        for (i, cause) in e.chain().enumerate() {
            // Skip the first message because it is printed above.
            if i > 0 {
                if i == 1 {
                    eprintln!("\nError trace:");
                }
                eprintln!("\t{}: {}", i, cause);
            }
        }
        std::process::exit(1);
    }
}

/// An internal token type so we dynamically choose between various token types. We can't box dynner
/// them for some reason, so we need to basically do our own fake dynamic dispatch. If this could be
/// useful for other consumers of the bindle crate, we could add this in the future
#[derive(Clone)]
enum PickYourAuth {
    None(NoToken),
    Http(HttpBasic),
    Oidc(OidcToken),
}

#[async_trait::async_trait]
impl TokenManager for PickYourAuth {
    async fn apply_auth_header(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder> {
        match &self {
            PickYourAuth::None(nt) => nt.apply_auth_header(builder).await,
            PickYourAuth::Http(h) => h.apply_auth_header(builder).await,
            PickYourAuth::Oidc(oidc) => oidc.apply_auth_header(builder).await,
        }
    }
}

async fn run() -> std::result::Result<(), ClientError> {
    let opts = opts::Opts::parse();
    // TODO: Allow log level setting outside of RUST_LOG (this is easier with this subscriber)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_ansi(atty::is(atty::Stream::Stderr))
        .init();
    let bindle_dir = opts.bindle_dir.unwrap_or_else(|| {
        dirs::cache_dir()
            .expect("Unable to infer cache directory")
            .join("bindle")
    });
    tokio::fs::create_dir_all(&bindle_dir).await?;
    let token_file = opts.token_file.unwrap_or_else(|| {
        dirs::config_dir()
            .expect("Unable to infer cache directory")
            .join("bindle/.token")
    });
    // Theoretically, someone could create the token file at /, which would mean this function
    // wouldn't return anything. This isn't an error, so do not attempt to create the directory if
    // so
    if let Some(p) = token_file.parent() {
        tokio::fs::create_dir_all(p).await?;
    }

    let token = if !matches!(opts.subcmd, SubCommand::Login(_)) {
        match OidcToken::new_from_file(&token_file).await {
            Ok(t) => {
                tracing::debug!("Found and loaded token file");
                PickYourAuth::Oidc(t)
            }
            // Token doesn't exist on disk, so assume they don't want token auth
            Err(ClientError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                tracing::debug!("No token file located, no token authentication will be used");
                // If we have basic auth set, and no token was found, use that
                if let Some(user) = opts.http_user {
                    PickYourAuth::Http(HttpBasic::new(
                        &user,
                        &opts.http_password.unwrap_or_default(),
                    ))
                } else {
                    PickYourAuth::None(NoToken)
                }
            }
            Err(e) => {
                let message = format!("Error loading token file {:?}: {}", &token_file, e);
                return Err(ClientError::InvalidConfig(message));
            }
        }
    } else {
        PickYourAuth::None(NoToken)
    };

    let bindle_client = ClientBuilder::default()
        .danger_accept_invalid_certs(opts.insecure)
        .build(&opts.server_url, token)?;

    let local = bindle::provider::file::FileProvider::new(
        bindle_dir,
        bindle::search::NoopEngine::default(),
    )
    .await;
    let cache = DumbCache::new(bindle_client.clone(), local);

    // We don't verify locally yet, but we will need the keyring to do so
    let _keyring = load_keyring(opts.keyring)
        .await
        .unwrap_or_else(|_| KeyRing::default());

    match opts.subcmd {
        SubCommand::Info(info_opts) => {
            let inv = match info_opts.yanked {
                true => cache.get_invoice(info_opts.bindle_id),
                false => cache.get_yanked_invoice(info_opts.bindle_id),
            }
            .await
            .map_err(map_storage_error)?;

            match info_opts.output {
                Some(format) if &format == "toml" => {
                    tokio::io::stdout().write_all(&toml::to_vec(&inv)?).await?
                }
                Some(format) if &format == "json" => {
                    tokio::io::stdout()
                        .write_all(&serde_json::to_vec_pretty(&inv)?)
                        .await?
                }
                Some(format) => {
                    return Err(ClientError::Other(format!("Unknown format: {}", format)))
                }
                None => tokio::io::stdout().write_all(&toml::to_vec(&inv)?).await?,
            }
        }
        SubCommand::GetInvoice(gi_opts) => {
            let inv = match gi_opts.yanked {
                true => cache.get_invoice(&gi_opts.bindle_id),
                false => cache.get_yanked_invoice(&gi_opts.bindle_id),
            }
            .await
            .map_err(map_storage_error)?;
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // Make sure we aren't overwriting
                .open(&gi_opts.output)
                .await?;
            file.write_all(&toml::to_vec(&inv)?).await?;
            file.flush().await?;
            println!(
                "Wrote invoice {} to {}",
                gi_opts.bindle_id,
                gi_opts.output.display()
            );
        }
        SubCommand::GetParcel(gp_opts) => get_parcel(cache, gp_opts).await?,
        SubCommand::Yank(yank_opts) => {
            bindle_client.yank_invoice(&yank_opts.bindle_id).await?;
            println!("Bindle {} yanked", yank_opts.bindle_id);
        }
        SubCommand::Search(search_opts) => {
            // TODO: Do we want to use the cache for searching?
            let matches = bindle_client
                .query_invoices(search_opts.clone().into())
                .await?;

            match search_opts.output {
                Some(format) if &format == "toml" => {
                    tokio::io::stdout()
                        .write_all(&toml::to_vec(&matches)?)
                        .await?
                }
                Some(format) if &format == "json" => {
                    tokio::io::stdout()
                        .write_all(&serde_json::to_vec_pretty(&matches)?)
                        .await?
                }
                Some(format) if &format == "table" => tablify(&matches),
                Some(format) => {
                    return Err(ClientError::Other(format!("Unknown format: {}", format)))
                }
                None => tablify(&matches),
            }
        }
        SubCommand::Get(get_opts) => get_all(cache, get_opts).await?,
        SubCommand::Push(push_opts) => push_all(bindle_client, push_opts).await?,
        SubCommand::PushInvoice(push_opts) => {
            let resp = bindle_client
                .create_invoice_from_file(push_opts.path)
                .await?;
            println!("Invoice {} created", resp.invoice.bindle.id);
        }
        SubCommand::SignInvoice(sign_opts) => {
            // Role
            let role = if let Some(r) = sign_opts.role {
                role_from_name(r)?
            } else {
                SignatureRole::Creator
            };
            // Keyfile
            let keyfile = match sign_opts.secret_file {
                Some(dir) => dir,
                None => ensure_config_dir().await?.join("secret_keys.toml"),
            };

            // Signing key
            let key = first_matching_key(keyfile, &role).await?;

            // Load the invoice and sign it.
            let mut inv: Invoice = bindle::client::load::toml(sign_opts.invoice.as_str()).await?;
            inv.sign(role.clone(), &key)?;

            // Write the signed invoice to a file.
            let outfile = sign_opts
                .destination
                .unwrap_or_else(|| format!("./invoice-{}.toml", inv.canonical_name()));

            println!(
                "Signed as {} with role {} and wrote to {}",
                sign_opts.invoice, role, outfile
            );
            tokio::fs::write(outfile, toml::to_string(&inv)?).await?;
        }
        SubCommand::PushFile(push_opts) => {
            let label =
                generate_label(&push_opts.path, push_opts.name, push_opts.media_type).await?;
            println!("Uploading file {} to server", push_opts.path.display());
            bindle_client
                .create_parcel_from_file(push_opts.bindle_id, &label.sha256, push_opts.path)
                .await?;
            println!("File successfully uploaded");
        }
        SubCommand::GenerateLabel(generate_opts) => {
            let label = generate_label(
                generate_opts.path,
                generate_opts.name,
                generate_opts.media_type,
            )
            .await?;
            println!("{}", toml::to_string_pretty(&label)?);
        }
        SubCommand::PrintKey(print_key_opts) => {
            let dir = match print_key_opts.secret_file {
                Some(dir) => dir,
                None => ensure_config_dir().await?.join("secret_keys.toml"),
            };
            let keyfile = SecretKeyFile::load_file(dir)
                .await
                .map_err(|e| ClientError::Other(e.to_string()))?;

            let matches: Vec<KeyEntry> = match print_key_opts.label {
                Some(name) => keyfile
                    .key
                    .iter()
                    .filter_map(|k| {
                        if !k.label.contains(&name) {
                            return None;
                        }
                        match k.try_into() {
                            //Skip malformed keys.
                            Err(e) => {
                                eprintln!("Warning: Malformed key: {} (skipping)", e);
                                None
                            }
                            Ok(ke) => Some(ke),
                        }
                    })
                    .collect(),
                None => keyfile
                    .key
                    .iter()
                    .filter_map(|k| match k.try_into() {
                        //Skip malformed keys.
                        Err(e) => {
                            eprintln!("Warning: Malformed key: {} (skipping)", e);
                            None
                        }
                        Ok(ke) => Some(ke),
                    })
                    .collect(),
            };

            let keyring = KeyRing::new(matches);
            let out = toml::to_string(&keyring).map_err(|e| ClientError::Other(e.to_string()))?;
            println!("{}", out);
        }
        SubCommand::CreateKey(create_opts) => {
            let dir = match create_opts.secret_file {
                Some(dir) => dir,
                None => ensure_config_dir().await?.join("secret_keys.toml"),
            };
            println!("Writing keys to {}", dir.display());

            match tokio::fs::metadata(&dir).await {
                Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                    println!("File {} does not exist. Creating it.", dir.display());
                    let mut keyfile = SecretKeyFile::default();
                    let newkey = SecretKeyEntry::new(
                        create_opts.label,
                        vec![bindle::SignatureRole::Creator],
                    );
                    keyfile.key.push(newkey);
                    keyfile
                        .save_file(dir)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;
                }
                Ok(info) => {
                    if !info.is_file() {
                        eprint!("Path must point to a file.");
                        return Err(ClientError::Other(
                            "Keyfile cannot be directory or symlink".to_owned(),
                        ));
                    }
                    let mut keyfile = SecretKeyFile::load_file(&dir)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;
                    let newkey = SecretKeyEntry::new(
                        create_opts.label,
                        vec![bindle::SignatureRole::Creator],
                    );
                    keyfile.key.push(newkey);
                    keyfile
                        .save_file(dir)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        SubCommand::Login(_login_opts) => {
            // TODO: We'll use login opts when we enable additional login providers
            OidcToken::login(&opts.server_url, token_file).await?;
            println!("Login successful");
        }
    }

    Ok(())
}

async fn generate_label(
    file_path: impl AsRef<Path>,
    name: Option<String>,
    media_type: Option<String>,
) -> Result<bindle::Label> {
    let path = file_path.as_ref().to_owned();
    let mut file = tokio::fs::File::open(&path).await?;
    let media_type = media_type.unwrap_or_else(|| {
        mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string()
    });
    info!("Using media type {}", media_type);
    // Note: Should be able to unwrap here because the file opening step would have
    // failed in conditions where this returns `None`
    let name = name.unwrap_or_else(|| path.file_name().unwrap().to_string_lossy().to_string());
    info!("Using name {}", name);
    let size = file.metadata().await?.len();
    let mut sha = bindle::async_util::AsyncSha256::new();
    tokio::io::copy(&mut file, &mut sha).await?;
    let result = sha.into_inner().expect("data lock error").finalize();

    Ok(bindle::Label {
        sha256: format!("{:x}", result),
        media_type,
        size,
        name,
        annotations: None, // TODO: allow annotations from command line
        ..bindle::Label::default()
    })
}

async fn get_parcel<C: Cache + Send + Sync + Clone>(cache: C, opts: GetParcel) -> Result<()> {
    let parcel = cache
        .get_parcel(opts.bindle_id, &opts.sha)
        .await
        .map_err(map_storage_error)?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // Make sure we aren't overwriting
        .open(&opts.output)
        .await?;

    tokio::io::copy(
        &mut StreamReader::new(
            parcel.map(|res| res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
        ),
        &mut file,
    )
    .await?;
    println!("Wrote parcel {} to {}", opts.sha, opts.output.display());
    Ok(())
}

async fn push_all<T: TokenManager + Send + Sync + Clone + 'static>(
    client: Client<T>,
    opts: Push,
) -> Result<()> {
    let standalone = StandaloneRead::new(opts.path, &opts.bindle_id).await?;
    standalone.push(&client).await?;
    println!("Pushed bindle {}", opts.bindle_id);
    Ok(())
}

async fn get_all<C: Cache + Send + Sync + Clone>(cache: C, opts: Get) -> Result<()> {
    let inv = match opts.yanked {
        true => cache.get_invoice(opts.bindle_id),
        false => cache.get_yanked_invoice(opts.bindle_id),
    }
    .await
    .map_err(map_storage_error)?;

    println!("Fetched invoice. Starting fetch of parcels");

    let parcels = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let zero_vec = Vec::with_capacity(0);
    let is_export = opts.export.is_some();
    let parcel_fetch = inv
        .parcel
        .as_ref()
        .unwrap_or(&zero_vec)
        .iter()
        .map(|p| {
            (
                p.label.sha256.clone(),
                inv.bindle.id.clone(),
                cache.clone(),
                parcels.clone(),
            )
        })
        .map(|(sha, bindle_id, c, parcels)| async move {
            match c.get_parcel(bindle_id, &sha).await {
                Ok(p) => {
                    println!("Fetched parcel {}", sha);
                    if is_export {
                        parcels.lock().await.insert(
                            sha,
                            StreamReader::new(p.map(|res| {
                                res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                            })),
                        );
                    }
                }
                Err(e) => {
                    match e {
                        ProviderError::NotFound => warn!("Parcel {} does not exist", sha),
                        ProviderError::ProxyError(err)
                            if matches!(err, ClientError::ParcelNotFound) =>
                        {
                            warn!("Parcel {} does not exist", sha)
                        }
                        // Only return an error if it isn't a not found error. By design, an invoice
                        // can contain parcels that don't yet exist
                        ProviderError::ProxyError(inner) => return Err(inner),
                        _ => {
                            return Err(ClientError::Other(format!(
                                "Unable to get parcel {}: {:?}",
                                sha, e
                            )))
                        }
                    }
                }
            }
            Ok(())
        });
    futures::future::join_all(parcel_fetch)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    if let Some(p) = opts.export {
        let standalone = StandaloneWrite::new(p, &inv.bindle.id).await?;
        standalone
            .write(
                inv,
                // All locks should be done at this point (as all futures exited), so panicing feels
                // right here as it is an unrecoverable condition
                Arc::try_unwrap(parcels)
                    .map_err(|_| ClientError::Other("Unexpected lock error".to_string()))
                    .unwrap()
                    .into_inner(),
            )
            .await?;
    }

    Ok(())
}

async fn load_keyring(keyring: Option<PathBuf>) -> anyhow::Result<KeyRing> {
    // This takes an Option<PathBuf> because we want to wrap all of the flag handling in this
    // function, including setting the default if the kyering is None.
    let dir = keyring
        .unwrap_or_else(default_config_dir)
        .join("keyring.toml");
    let kr = bindle::client::load::toml(dir).await?;
    Ok(kr)
}

fn map_storage_error(e: ProviderError) -> ClientError {
    match e {
        ProviderError::Io(e) => ClientError::Io(e),
        ProviderError::ProxyError(inner) => inner,
        ProviderError::InvalidId(parse_err) => ClientError::InvalidId(parse_err),
        _ => ClientError::Other(format!("{}", e)),
    }
}

fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|v| v.join("bindle/"))
        .unwrap_or_else(|| "./bindle".into())
}

/// Get the config dir, ensuring that it exists.
///
/// This will return the default config directory. If that directory does not
/// exist, it will be created before the path is returned.
///
/// If the system does not have a configuration directory, this will create a directory named
/// `bindle/` in the local working directory.
///
/// This will return an error
async fn ensure_config_dir() -> Result<PathBuf> {
    let dir = default_config_dir();
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

fn role_from_name(name: String) -> Result<SignatureRole> {
    match name.as_str() {
        "c" | "creator" => Ok(SignatureRole::Creator),
        "h" | "host" => Ok(SignatureRole::Host),
        "a" | "approver" => Ok(SignatureRole::Approver),
        "p" | "proxy" => Ok(SignatureRole::Proxy),
        _ => Err(ClientError::Other("Unknown role".to_owned())),
    }
}

async fn first_matching_key(fpath: PathBuf, role: &SignatureRole) -> Result<SecretKeyEntry> {
    let keys = SecretKeyFile::load_file(&fpath).await.map_err(|e| {
        ClientError::Other(format!("Error loading file {}: {}", fpath.display(), e))
    })?;

    keys.get_first_matching(role)
        .map(|k| k.to_owned())
        .ok_or_else(|| ClientError::Other("No satisfactory key found".to_owned()))
}

fn tablify(matches: &bindle::search::Matches) {
    let last = matches.offset + matches.invoices.len() as u64;
    let trailer = if matches.more {
        format!(" - More results are available with --offset={}", last)
    } else {
        "".to_owned()
    };

    for i in matches.invoices.iter() {
        println!(
            "{}:\t{}",
            &i.bindle.id,
            &i.bindle
                .description
                .clone()
                .unwrap_or_else(|| "[no description available]".to_string())
        )
    }
    if matches.total > 0 {
        println!(
            "=== Showing results {} to {} of {} (limit: {}){}",
            matches.offset + 1,
            last,
            matches.total,
            matches.limit,
            trailer,
        );
    } else {
        println!("No matching bindles were found");
    }
}
