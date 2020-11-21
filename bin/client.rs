use std::path::PathBuf;

use bindle::client::{Client, ClientError, Result};
use clap::Clap;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::stream::{Stream, StreamExt};

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
    #[clap(name = "get", about = "download the bindle and all its parcels")]
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
}

#[derive(Clap)]
struct Push {
    // TODO: Do we want to take a path to an invoice and a directory containing parcels?
}

#[derive(Clap)]
struct Get {
    #[clap(index = 1, value_name = "BINDLE")]
    bindle_id: String,
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
}

#[tokio::main]
async fn main() -> std::result::Result<(), ClientError> {
    let opts = Opts::parse();

    let bindle_client = Client::new(&opts.server_url)?;

    match opts.subcmd {
        SubCommand::Info(info_opts) => {
            let inv = bindle_client.get_invoice(info_opts.bindle_id).await?;
            tokio::io::stdout().write_all(&toml::to_vec(&inv)?).await?;
        }
        SubCommand::GetInvoice(gi_opts) => {
            let inv = bindle_client.get_invoice(&gi_opts.bindle_id).await?;
            tokio::fs::write(&gi_opts.output, &toml::to_vec(&inv)?).await?;
            println!(
                "Wrote invoice {} to {}",
                gi_opts.bindle_id,
                gi_opts.output.display()
            );
        }
        SubCommand::GetParcel(gp_opts) => get_parcel(bindle_client, gp_opts).await?,
        SubCommand::Yank(yank_opts) => {
            bindle_client.yank_invoice(&yank_opts.bindle_id).await?;
            println!("Bindle {} yanked", yank_opts.bindle_id);
        }
        SubCommand::Search(search_opts) => {
            let matches = bindle_client.query_invoices(search_opts.into()).await?;
            tokio::io::stdout()
                .write_all(&toml::to_vec(&matches)?)
                .await?;
        }
        _ => return Err(ClientError::Other("Command unimplemented".to_string())),
    }

    Ok(())
}

async fn get_parcel(bindle_client: Client, opts: GetParcel) -> Result<()> {
    let stream = bindle_client.get_parcel_stream(&opts.sha).await?;
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&opts.output)
        .await?;
    write_stream(stream, file).await?;
    println!("Wrote parcel {} to {}", opts.sha, opts.output.display());
    Ok(())
}

async fn write_stream<S, W>(mut stream: S, mut writer: W) -> Result<()>
where
    S: Stream<Item = Result<bytes::Bytes>> + Unpin,
    W: AsyncWrite + Unpin,
{
    while let Some(b) = stream.next().await {
        let b = b?;
        writer.write_all(&b).await?;
    }
    writer.flush().await?;
    Ok(())
}
