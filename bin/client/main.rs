use std::convert::TryInto;
use std::io::IsTerminal;
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
use bindle::signature::{KeyEntry, KeyRingLoader, KeyRingSaver, LabelMatch};
use bindle::standalone::{StandaloneRead, StandaloneWrite};
use bindle::SignatureError;
use bindle::{
    cache::{Cache, DumbCache},
    provider::Provider,
};

use base64::Engine;
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
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    let bindle_dir = opts.bindle_dir.unwrap_or_else(|| {
        dirs::cache_dir()
            .expect("Unable to infer cache directory")
            .join("bindle")
    });
    tokio::fs::create_dir_all(&bindle_dir).await?;
    let token_file = opts
        .token_file
        .unwrap_or_else(|| default_config_dir().join(".token"));
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

    let keyring_path: PathBuf = opts
        .keyring
        .unwrap_or_else(|| default_config_dir().join("keyring.toml"));

    if let Some(p) = keyring_path.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let keyring = Arc::new(
        keyring_path
            .load()
            .await
            .unwrap_or_else(|_| KeyRing::default()),
    );

    let bindle_client = ClientBuilder::default()
        .danger_accept_invalid_certs(opts.insecure)
        .verification_strategy(opts.strategy)
        .build(&opts.server_url, token, keyring)?;

    let local = bindle::provider::file::FileProvider::new(
        &bindle_dir,
        bindle::search::NoopEngine::default(),
    )
    .await;
    let cache = DumbCache::new(bindle_client.clone(), local);

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
                r.parse()
                    .map_err(|e: &str| ClientError::Other(e.to_owned()))?
            } else {
                SignatureRole::Creator
            };
            // Keyfile
            let keyfile = match sign_opts.secret_file {
                Some(dir) => dir,
                None => ensure_config_dir().await?.join("secret_keys.toml"),
            };

            let match_type = match (sign_opts.label, sign_opts.label_matching) {
                (Some(label), None) => Some(LabelMatch::FullMatch(label)),
                (None, Some(label_matching)) => Some(LabelMatch::PartialMatch(label_matching)),
                (None, None) => None,
                _ => {
                    unreachable!("both label and label-matching cannot be present at the same time")
                }
            };

            // Signing key
            let key = first_matching_key(keyfile, &role, match_type.as_ref()).await?;

            // Load the invoice and sign it.
            let mut inv: Invoice = bindle::client::load::toml(sign_opts.invoice.as_str()).await?;
            inv.sign(role.clone(), &key)?;

            // Write the signed invoice to a file.
            let outfile = sign_opts
                .destination
                .unwrap_or_else(|| format!("./invoice-{}.toml", inv.canonical_name()));

            println!(
                "Signed {} with role as '{}', label as '{}' and wrote to {}",
                sign_opts.invoice, role, key.label, outfile
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
        SubCommand::Login(_login_opts) => {
            // TODO: We'll use login opts when we enable additional login providers
            OidcToken::login(&opts.server_url, token_file).await?;
            println!("Login successful");
        }
        SubCommand::Package(opts) => {
            let standalone = StandaloneWrite::new(opts.path, opts.bindle_id).await?;
            let sha = standalone
                .path()
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            println!(
                "Packaging standalone bindle directory {}",
                standalone.path().display()
            );
            standalone.tarball(&opts.export_dir).await?;
            println!(
                "Wrote standalone bindle tarball to {}",
                opts.export_dir.join(format!("{}.tar.gz", sha)).display()
            )
        }
        SubCommand::Keys(keys) => {
            match keys {
                Keys::Print(print_key_opts) => {
                    let dir = match print_key_opts.secret_file {
                        Some(dir) => dir,
                        None => ensure_config_dir().await?.join("secret_keys.toml"),
                    };
                    let keyfile = SecretKeyFile::load_file(dir)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;

                    let matches: Vec<KeyEntry> = match print_key_opts.label_matching {
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
                    let out =
                        toml::to_string(&keyring).map_err(|e| ClientError::Other(e.to_string()))?;
                    println!("{}", out);
                }
                Keys::Create(create_opts) => {
                    let dir = match create_opts.secret_file {
                        Some(dir) => dir,
                        None => ensure_config_dir().await?.join("secret_keys.toml"),
                    };
                    println!("Writing keys to {}", dir.display());

                    let roles = parse_roles(create_opts.roles)?;

                    let key = match tokio::fs::metadata(&dir).await {
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                            println!("File {} does not exist. Creating it.", dir.display());
                            let mut keyfile = SecretKeyFile::default();
                            let newkey = SecretKeyEntry::new(&create_opts.label, roles);
                            keyfile.key.push(newkey.clone());
                            keyfile
                                .save_file(dir)
                                .await
                                .map_err(|e| ClientError::Other(e.to_string()))?;

                            newkey
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
                            let newkey = SecretKeyEntry::new(&create_opts.label, roles);
                            keyfile.key.push(newkey.clone());
                            keyfile
                                .save_file(dir)
                                .await
                                .map_err(|e| ClientError::Other(e.to_string()))?;

                            newkey
                        }
                        Err(e) => return Err(e.into()),
                    };

                    if !create_opts.skip_keyring {
                        let mut keyring = keyring_path
                            .load()
                            .await
                            .unwrap_or_else(|_| KeyRing::default());
                        keyring.add_entry(key.try_into()?);
                        keyring_path
                            .save(&keyring)
                            .await
                            .map_err(|e| ClientError::Other(e.to_string()))?;
                    }
                }
                Keys::Add(opts) => {
                    let mut keyring = keyring_path
                        .load()
                        .await
                        .unwrap_or_else(|_| KeyRing::default());
                    // First, check that the key is actually valid
                    let key = base64::engine::general_purpose::STANDARD
                        .decode(&opts.key)
                        .map_err(|_| SignatureError::CorruptKey(opts.key.clone()))?;
                    ed25519_dalek::VerifyingKey::try_from(key.as_slice())
                        .map_err(|_| SignatureError::CorruptKey(opts.key.clone()))?;
                    keyring.add_entry(KeyEntry {
                        label: opts.label,
                        roles: parse_roles(opts.roles)?,
                        key: opts.key,
                        label_signature: None,
                    });

                    keyring_path
                        .save(&keyring)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;
                    println!("Wrote key to keyring file at {}", keyring_path.display())
                }
                Keys::Fetch(opts) => {
                    let new_keys = match opts.key_server {
                        Some(url) if !opts.use_host => {
                            println!("Fetching host keys from {}", url);
                            get_host_keys(url).await?
                        }
                        _ => {
                            println!("Fetching host keys from bindle server");
                            bindle_client.get_host_keys().await?
                        }
                    };

                    let mut keyring = keyring_path
                        .load()
                        .await
                        .unwrap_or_else(|_| KeyRing::default());
                    let orig_len = keyring.key.len();
                    // Have to filter before extending so we finish with the borrow of the current keyring
                    let filtered_keys: Vec<KeyEntry> = new_keys
                        .key
                        .into_iter()
                        .filter(|k| !keyring.key.iter().any(|current| current.key == k.key))
                        .collect();
                    keyring.key.extend(filtered_keys);
                    keyring_path
                        .save(&keyring)
                        .await
                        .map_err(|e| ClientError::Other(e.to_string()))?;
                    println!(
                        "Wrote {} keys to keyring file at {}",
                        keyring.key.len() - orig_len,
                        keyring_path.display()
                    )
                }
            }
        }
        SubCommand::Clean(_clean_opts) => {
            // Cleans up the local bindles directory.
            println!("Cleaning up bindles in {}...", bindle_dir.display());

            // although remove_dir_all crate could default to std::fs::remove_dir_all for unix family,
            // we still prefer tokio::fs implementation for unix
            #[cfg(target_family = "windows")]
            tokio::task::spawn_blocking(|| remove_dir_all::remove_dir_all(bindle_dir))
                .await
                .map_err(|e| ClientError::Other(e.to_string()))??;

            #[cfg(target_family = "unix")]
            tokio::fs::remove_dir_all(bindle_dir.clone()).await?;
            println!("Clean up successful");
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
    let parsed_id = bindle::Id::try_from(opts.bindle_id).map_err(ClientError::from)?;
    let tarball_path = opts.path.join(format!("{}.tar.gz", parsed_id.sha()));

    let standalone = if tokio::fs::metadata(&tarball_path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        StandaloneRead::new_from_tarball(tarball_path).await?
    } else {
        StandaloneRead::new(opts.path, &parsed_id).await?
    };
    standalone.push(&client).await?;
    println!("Pushed bindle {}", parsed_id);
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
        // Create a tempdir for collecting the files
        let tempdir = tokio::task::spawn_blocking(tempfile::tempdir)
            .await
            .map_err(|e| ClientError::Other(e.to_string()))??;
        let standalone = StandaloneWrite::new(tempdir.path(), &inv.bindle.id).await?;
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
        standalone.tarball(p).await?;
    }

    Ok(())
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

async fn first_matching_key(
    fpath: PathBuf,
    role: &SignatureRole,
    label_match: Option<&LabelMatch>,
) -> Result<SecretKeyEntry> {
    let keys = SecretKeyFile::load_file(&fpath).await.map_err(|e| {
        ClientError::Other(format!("Error loading file {}: {}", fpath.display(), e))
    })?;

    keys.get_first_matching(role, label_match)
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

/// Parses a comma delimited list of roles and returns a Vec of those parsed roles
fn parse_roles(roles: String) -> Result<Vec<SignatureRole>> {
    roles
        .split(',')
        .map(|raw| {
            raw.parse()
                .map_err(|e: &str| ClientError::Other(e.to_owned()))
        })
        .collect()
}

async fn get_host_keys(url: url::Url) -> Result<KeyRing> {
    let resp = reqwest::get(url).await?;
    if resp.status() != reqwest::StatusCode::OK {
        return Err(ClientError::Other(format!(
            "Unable to fetch host keys. Got status code {} with body content:\n{}",
            resp.status(),
            String::from_utf8_lossy(&resp.bytes().await?)
        )));
    }

    toml::from_slice(&resp.bytes().await?).map_err(ClientError::from)
}
