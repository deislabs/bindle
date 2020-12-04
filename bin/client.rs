use std::path::PathBuf;

use bindle::client::{Client, ClientError, Result};
use bindle::storage::StorageError;
use bindle::{
    cache::{Cache, DumbCache},
    storage::Storage,
};
use clap::Clap;
use log::warn;
use tokio::io::AsyncWriteExt;

const DESCRIPTION: &str = r#"
The Bindle Client

Bindle is a technology for storing and retrieving aggregate applications.
This program provides tools for working with Bindle servers.
"#;

#[derive(Clap)]
#[clap(name = "bindle", version = "0.1.0", author = "DeisLabs at Microsoft Azure", about = DESCRIPTION)]
struct Opts {
    #[clap(
        short = 's',
        long = "server",
        env = "BINDLE_SERVER_URL",
        about = "The address of the bindle server"
    )]
    server_url: String,
    #[clap(
        short = 'd',
        long = "bindle-dir",
        env = "BINDLE_DIR",
        about = "The directory where bindles are stored/cached, defaults to $HOME/.bindle/bindles"
    )]
    bindle_dir: Option<PathBuf>,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    #[clap(name = "info", about = "get the bindle invoice and display it")]
    Info(Info),
    #[clap(
        name = "push",
        about = "push a bindle and all its parcels to the server"
    )]
    Push(Push),
    #[clap(name = "get", about = "download the given bindle and all its parcels")]
    Get(Get),
    #[clap(name = "yank", about = "yank an existing bindle")]
    Yank(Yank),
    #[clap(name = "search", about = "search for bindles")]
    Search(Search),
    #[clap(
        name = "get-parcel",
        about = "get an individual parcel by SHA and store it to a specific location"
    )]
    GetParcel(GetParcel),
    #[clap(
        name = "get-invoice",
        about = "get only the specified invoice (does not download parcels) and store it to a specific location"
    )]
    GetInvoice(GetInvoice),
}

#[derive(Clap)]
struct Info {
    #[clap(index = 1, value_name = "BINDLE")]
    bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    yanked: bool,
}

#[derive(Clap)]
struct Push {
    // TODO: Do we want to take a path to an invoice and a directory containing parcels?
}

#[derive(Clap)]
struct Get {
    #[clap(index = 1, value_name = "BINDLE")]
    bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    yanked: bool,
}

#[derive(Clap)]
struct Yank {
    #[clap(index = 1, value_name = "BINDLE")]
    bindle_id: String,
}

const VERSION_QUERY: &str = r#"version constraint of the bindle to search for. This is a semver range modifier that can either denote an exact version, or a range of versions.

For example, the range modifier `v=1.0.0-beta.1` indicates that a version MUST match version `1.0.0-beta.1`. Version `1.0.0-beta.12` does NOT match this modifier. 

The range modifiers will include the following modifiers, all based on the Node.js de facto behaviors (found at https://www.npmjs.com/package/semver):

- `<`, `>`, `<=`, `>=`, `=` -- all approximately their mathematical equivalents
- `-` (`1.2.3 - 1.5.6`) -- range declaration
- `^` -- patch/minor updates allow (`^1.2.3` would accept `1.2.4` and `1.3.0`)
- `~` -- at least the given version
"#;

#[derive(Clap)]
struct Search {
    // TODO: Figure out output format (like tables)
    #[clap(
        short = 'q',
        long = "query",
        about = "name of the bindle to search for, an empty query means all bindles"
    )]
    query: Option<String>,
    #[clap(short = 'b', long = "bindle-version", about = "version constraint of the bindle to search for", long_about = VERSION_QUERY)]
    version: Option<String>,
    #[clap(
        long = "offset",
        about = "the offset where to start the next page of results"
    )]
    offset: Option<u64>,
    #[clap(long = "limit", about = "the limit of results per page")]
    limit: Option<u8>,
    #[clap(
        long = "strict",
        about = "whether or not to use strict mode",
        long_about = "whether or not to use strict mode. Please note that bindle servers must implement a strict mode per the specification, a non-strict (standard) mode is optional"
    )]
    strict: Option<bool>,
    #[clap(
        long = "yanked",
        about = "whether or not to include yanked bindles in the search result"
    )]
    yanked: Option<bool>,
}

impl From<Search> for bindle::QueryOptions {
    fn from(s: Search) -> Self {
        bindle::QueryOptions {
            query: s.query,
            version: s.version,
            offset: s.offset,
            limit: s.limit,
            strict: s.strict,
            yanked: s.yanked,
        }
    }
}

#[derive(Clap)]
struct GetParcel {
    #[clap(index = 1, value_name = "PARCEL_SHA")]
    sha: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./parcel.dat",
        about = "The location where to output the parcel to"
    )]
    output: PathBuf,
}

#[derive(Clap)]
struct GetInvoice {
    #[clap(index = 1, value_name = "BINDLE")]
    bindle_id: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./invoice.toml",
        about = "The location where to output the invoice to"
    )]
    output: PathBuf,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    yanked: bool,
}

#[tokio::main]
async fn main() -> std::result::Result<(), ClientError> {
    let opts = Opts::parse();
    // TODO: Allow log level setting
    env_logger::init();

    let bindle_client = Client::new(&opts.server_url)?;
    let bindle_dir = opts
        .bindle_dir
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".bindle/bindles"));
    tokio::fs::create_dir_all(&bindle_dir).await?;
    let store =
        bindle::storage::file::FileStorage::new(bindle_dir, bindle::search::NoopEngine::default())
            .await;
    let cache = DumbCache::new(bindle_client.clone(), store);

    match opts.subcmd {
        SubCommand::Info(info_opts) => {
            let inv = match info_opts.yanked {
                true => cache.get_invoice(info_opts.bindle_id),
                false => cache.get_yanked_invoice(info_opts.bindle_id),
            }
            .await
            .map_err(map_storage_error)?;
            tokio::io::stdout().write_all(&toml::to_vec(&inv)?).await?;
        }
        SubCommand::GetInvoice(gi_opts) => {
            let inv = match gi_opts.yanked {
                true => cache.get_invoice(&gi_opts.bindle_id),
                false => cache.get_yanked_invoice(&gi_opts.bindle_id),
            }
            .await
            .map_err(map_storage_error)?;
            tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // Make sure we aren't overwriting
                .open(&gi_opts.output)
                .await?
                .write_all(&toml::to_vec(&inv)?)
                .await?;
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
            let matches = bindle_client.query_invoices(search_opts.into()).await?;
            tokio::io::stdout()
                .write_all(&toml::to_vec(&matches)?)
                .await?;
        }
        SubCommand::Get(get_opts) => get_all(cache, get_opts).await?,
        SubCommand::Push(_) => return Err(ClientError::Other("Command unimplemented".to_string())),
    }

    Ok(())
}

async fn get_parcel<C: Cache + Send + Sync + Clone>(cache: C, opts: GetParcel) -> Result<()> {
    let mut parcel = cache
        .get_parcel(&opts.sha)
        .await
        .map_err(map_storage_error)?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // Make sure we aren't overwriting
        .open(&opts.output)
        .await?;
    tokio::io::copy(&mut parcel, &mut file).await?;
    println!("Wrote parcel {} to {}", opts.sha, opts.output.display());
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
    let parcel_fetch = inv
        .parcel
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.label.sha256, cache.clone()))
        .map(|(sha, c)| async move {
            if let Err(e) = c.get_parcel(&sha).await {
                match e {
                    StorageError::NotFound => warn!("Parcel {} does not exist", sha),
                    StorageError::CacheError(err) if matches!(err, ClientError::ParcelNotFound) => {
                        warn!("Parcel {} does not exist", sha)
                    }
                    // Only return an error if it isn't a not found error. By design, an invoice
                    // can contain parcels that don't yet exist
                    StorageError::CacheError(inner) => return Err(inner),
                    _ => {
                        return Err(ClientError::Other(format!(
                            "Unable to get parcel {}: {:?}",
                            sha, e
                        )))
                    }
                }
            } else {
                println!("Fetched parcel {}", sha);
            }
            Ok(())
        });
    futures::future::join_all(parcel_fetch)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    Ok(())
}

fn map_storage_error(e: StorageError) -> ClientError {
    match e {
        StorageError::Io(e) => ClientError::Io(e),
        StorageError::CacheError(inner) => inner,
        _ => ClientError::Other(format!("{:?}", e)),
    }
}
