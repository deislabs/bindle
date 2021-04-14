use clap::Clap;
use std::path::PathBuf;

const DESCRIPTION: &str = r#"
The Bindle Client

Bindle is a technology for storing and retrieving aggregate applications.
This program provides tools for working with Bindle servers.
"#;

#[derive(Clap)]
#[clap(name = "bindle", version = clap::crate_version!(), author = "DeisLabs at Microsoft Azure", about = DESCRIPTION)]
pub struct Opts {
    #[clap(
        short = 's',
        long = "server",
        env = "BINDLE_SERVER_URL",
        about = "The address of the bindle server"
    )]
    pub server_url: String,
    #[clap(
        short = 'd',
        long = "bindle-dir",
        env = "BINDLE_DIR",
        about = "The directory where bindles are stored/cached, defaults to $HOME/.bindle/bindles"
    )]
    pub bindle_dir: Option<PathBuf>,
    #[clap(subcommand)]
    pub subcmd: SubCommand,
}

#[derive(Clap)]
pub enum SubCommand {
    #[clap(name = "info", about = "get the bindle invoice and display it")]
    Info(Info),
    #[clap(
        name = "push",
        about = "push a bindle and all its parcels to the server"
    )]
    Push(Push),
    #[clap(name = "push-invoice", about = "push an invoice file to the server")]
    PushInvoice(PushInvoice),
    #[clap(
        name = "push-file",
        about = "push an arbitrary file as a parcel to the server"
    )]
    PushFile(PushFile),
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
    #[clap(
        name = "generate-label",
        about = "generates a label for the given file and prints it to stdout. This can be used to generate the label and add it to an invoice"
    )]
    GenerateLabel(GenerateLabel),
    #[clap(
        name = "create-key",
        about = "creates a new signing key and places it in the local secret keys. If no secret file is provided, this will store in the default config directory for Bindle. The LABEL is typically a name and email address, of the form 'name <email>'."
    )]
    CreateKey(CreateKey),
    #[clap(
        name = "sign-invoice",
        about = "sign an invoice with one of your secret keys"
    )]
    SignInvoice(SignInvoice),
}

#[derive(Clap)]
pub struct Info {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        about = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
}

#[derive(Clap)]
pub struct Push {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        about = "The name of the bindle, e.g. foo/bar/baz/1.2.3"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'p',
        long = "path",
        default_value = "./",
        about = "a path where the standalone bindle directory is located"
    )]
    pub path: PathBuf,
}

#[derive(Clap)]
pub struct Get {
    #[clap(index = 1, value_name = "BINDLE")]
    pub bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
    #[clap(
        short = 'e',
        long = "export",
        about = "if specified, export the bindle as a standlone bindle in the given directory"
    )]
    pub export: Option<PathBuf>,
}

#[derive(Clap)]
pub struct Yank {
    #[clap(index = 1, value_name = "BINDLE")]
    pub bindle_id: String,
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
pub struct Search {
    // TODO: Figure out output format (like tables)
    #[clap(
        short = 'q',
        long = "query",
        about = "name of the bindle to search for, an empty query means all bindles"
    )]
    pub query: Option<String>,
    #[clap(short = 'b', long = "bindle-version", about = "version constraint of the bindle to search for", long_about = VERSION_QUERY)]
    pub version: Option<String>,
    #[clap(
        long = "offset",
        about = "the offset where to start the next page of results"
    )]
    pub offset: Option<u64>,
    #[clap(long = "limit", about = "the limit of results per page")]
    pub limit: Option<u8>,
    #[clap(
        long = "strict",
        about = "whether or not to use strict mode",
        long_about = "whether or not to use strict mode. Please note that bindle servers must implement a strict mode per the specification, a non-strict (standard) mode is optional"
    )]
    pub strict: Option<bool>,
    #[clap(
        long = "yanked",
        about = "whether or not to include yanked bindles in the search result"
    )]
    pub yanked: Option<bool>,
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
pub struct GetParcel {
    #[clap(index = 1, value_name = "BINDLE_ID")]
    pub bindle_id: bindle::Id,
    #[clap(index = 2, value_name = "PARCEL_SHA")]
    pub sha: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./parcel.dat",
        about = "The location where to output the parcel to"
    )]
    pub output: PathBuf,
}

#[derive(Clap)]
pub struct GetInvoice {
    #[clap(index = 1, value_name = "BINDLE")]
    pub bindle_id: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./invoice.toml",
        about = "The location where to output the invoice to"
    )]
    pub output: PathBuf,
    #[clap(
        short = 'y',
        long = "yanked",
        about = "whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
}

#[derive(Clap)]
pub struct GenerateLabel {
    #[clap(index = 1, value_name = "FILE")]
    pub path: PathBuf,
    #[clap(
        short = 'n',
        long = "name",
        about = "the name of the parcel, defaults to the name + extension of the file"
    )]
    pub name: Option<String>,
    #[clap(
        short = 'm',
        long = "media-type",
        about = "the media (mime) type of the file. If not provided, the tool will attempt to guess the mime type. If guessing fails, the default is `application/octet-stream`"
    )]
    pub media_type: Option<String>,
}

#[derive(Clap)]
pub struct CreateKey {
    #[clap(
        index = 1,
        value_name = "LABEL",
        about = "The name of the key, such as 'Matt <me@example.com>'"
    )]
    pub label: String,
    #[clap(
        short = 'f',
        long = "secrets-file",
        about = "the path to the file where secrets should be stored. If it does not exist, it will be created. If it does exist, the key will be appended."
    )]
    pub secret_file: Option<PathBuf>,
}

#[derive(Clap)]
pub struct SignInvoice {
    #[clap(
        index = 1,
        value_name = "INVOICE",
        about = "the path to the invoice to sign"
    )]
    pub invoice: String,
    #[clap(
        short = 'f',
        long = "secrets-file",
        about = "the path to the file where secret keys are stored. Use 'create-key' to create a new key"
    )]
    pub secret_file: Option<PathBuf>,
    #[clap(
        short = 'r',
        long = "role",
        about = "the role to sign with. Values are: c[reator], a[pprover], h[ost], p[roxy]. If no role is specified, 'creator' is used"
    )]
    pub role: Option<String>,
    #[clap(
        short = 'o',
        long = "out",
        about = "the location to write the modified invoice. By default, it will write to invoice-HASH.toml, where HASH is computed on name and version"
    )]
    pub destination: Option<String>,
}

#[derive(Clap)]
pub struct PushInvoice {
    #[clap(
        index = 1,
        value_name = "FILE",
        default_value = "./invoice.toml",
        about = "The path to the invoice TOML file"
    )]
    pub path: PathBuf,
}

#[derive(Clap)]
pub struct PushFile {
    #[clap(index = 1, value_name = "BINDLE_ID")]
    pub bindle_id: bindle::Id,
    #[clap(index = 2, value_name = "FILE")]
    pub path: PathBuf,
    #[clap(
        short = 'n',
        long = "name",
        about = "the name of the parcel, defaults to the name + extension of the file"
    )]
    pub name: Option<String>,
    #[clap(
        short = 'm',
        long = "media-type",
        about = "the media (mime) type of the file. If not provided, the tool will attempt to guess the mime type. If guessing fails, the default is `application/octet-stream`"
    )]
    pub media_type: Option<String>,
}
