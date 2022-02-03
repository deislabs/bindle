use bindle::VerificationStrategy;
use clap::Parser;
use std::path::PathBuf;

const DESCRIPTION: &str = r#"
The Bindle Client

Bindle is a technology for storing and retrieving aggregate applications.
This program provides tools for working with Bindle servers.
"#;

#[derive(Parser)]
#[clap(name = "bindle", version = clap::crate_version!(), author = "DeisLabs at Microsoft Azure", about = DESCRIPTION)]
pub struct Opts {
    #[clap(
        short = 's',
        long = "server",
        env = "BINDLE_URL",
        help = "The address of the bindle server, including the top level subpath (e.g. https://my.bindle.com/v1/",
        default_value = "http://localhost:8080/v1/"
    )]
    pub server_url: String,
    #[clap(
        short = 'd',
        long = "bindle-dir",
        env = "BINDLE_DIRECTORY",
        help = "The directory where bindles are stored/cached, defaults to $XDG_CACHE_HOME"
    )]
    pub bindle_dir: Option<PathBuf>,

    #[clap(
        short = 'r',
        long = "keyring",
        env = "BINDLE_KEYRING",
        help = "The path to the keyring file. Defaults to $XDG_CONFIG/bindle/keyring.toml"
    )]
    pub keyring: Option<PathBuf>,

    #[clap(
        short = 't',
        long = "token-file",
        env = "BINDLE_TOKEN_FILE",
        help = "The path to the login token file. If running `bindle login` this is where the file will be saved to. Defaults to $XDG_CONFIG/bindle/.token"
    )]
    pub token_file: Option<PathBuf>,

    #[clap(
        long = "http-user",
        help = "Username for HTTP Basic auth",
        requires = "http-password",
        env = "BINDLE_HTTP_PASSWORD",
        hide_env_values = true
    )]
    pub http_user: Option<String>,

    #[clap(
        long = "http-password",
        help = "Password for HTTP Basic auth",
        requires = "http-user",
        env = "BINDLE_HTTP_USER"
    )]
    pub http_password: Option<String>,

    #[clap(
        short = 'k',
        long = "insecure",
        help = "If set, ignore server certificate errors",
        takes_value = false
    )]
    pub insecure: bool,

    #[clap(
        long = "strategy",
        help = "The verification strategy to use when pulling the bindle",
        default_value = "MultipleAttestationGreedy[Host]"
    )]
    pub strategy: VerificationStrategy,

    #[clap(subcommand)]
    pub subcmd: SubCommand,
}

#[derive(Parser)]
pub enum SubCommand {
    #[clap(name = "info", about = "Get the bindle invoice and display it")]
    Info(Info),
    #[clap(
        name = "push",
        about = "Push a bindle and all its parcels to the server"
    )]
    Push(Push),
    #[clap(name = "push-invoice", about = "Push an invoice file to the server")]
    PushInvoice(PushInvoice),
    #[clap(
        name = "push-file",
        about = "Push an arbitrary file as a parcel to the server"
    )]
    PushFile(PushFile),
    #[clap(name = "get", about = "Download the given bindle and all its parcels")]
    Get(Get),
    #[clap(name = "yank", about = "Yank an existing bindle")]
    Yank(Yank),
    #[clap(name = "search", about = "Search for bindles")]
    Search(Search),
    #[clap(
        name = "get-parcel",
        about = "Get an individual parcel by SHA and store it to a specific location"
    )]
    GetParcel(GetParcel),
    #[clap(
        name = "get-invoice",
        about = "Get only the specified invoice (does not download parcels) and store it to a specific location"
    )]
    GetInvoice(GetInvoice),
    #[clap(
        name = "generate-label",
        about = "Generates a label for the given file and prints it to stdout. This can be used to generate the label and add it to an invoice"
    )]
    GenerateLabel(GenerateLabel),
    #[clap(
        name = "sign-invoice",
        about = "Sign an invoice with one of your secret keys"
    )]
    SignInvoice(SignInvoice),
    #[clap(
        name = "login",
        about = "Logs in to a bindle server, saving the token locally"
    )]
    Login(Login),
    #[clap(
        name = "package",
        about = "Packages up a standalone bindle directory into a tarball"
    )]
    Package(Package),

    #[clap(subcommand)]
    Keys(Keys),
}

#[derive(Parser)]
#[clap(about = "Operations for creating and managing signing keys and keychains")]
pub enum Keys {
    #[clap(
        name = "create",
        about = "Creates a new signing key and places it in the local secret keys. Also adds the public key to your keychain. If no secret file is provided, this will store in the default config directory for Bindle. The LABEL is typically a name and email address, of the form 'name <email>'."
    )]
    Create(CreateKey),
    #[clap(name = "add", about = "Adds a public key to your keychain")]
    Add(AddKey),
    #[clap(
        name = "print",
        about = "Print the public key entries for keys from the secret key file. If no '--label' is supplied, public keys for all secret keys are returned."
    )]
    Print(PrintKey),
    // TODO(thomastaylor312): We should probably add an endpoint to bindle servers that allow you to download a host key and add a subcommand to help with it
}

#[derive(Parser)]
pub struct Info {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        help = "Whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
    #[clap(
        short = 'f',
        long = "output-format",
        help = "choose an output format",
        possible_values = &["json", "toml"],
    )]
    pub output: Option<String>,
}

#[derive(Parser)]
pub struct Push {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. foo/bar/baz/1.2.3. The sha of this ID should correspond to the standalone tarball or directory you wish to push"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'p',
        long = "path",
        default_value = "./",
        help = "A path where the standalone bindle directory or tarball is located"
    )]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct Get {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'y',
        long = "yanked",
        help = "Whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
    #[clap(
        short = 'e',
        long = "export",
        help = "If specified, export the bindle as a standlone bindle tarball in the given directory"
    )]
    pub export: Option<PathBuf>,
}

#[derive(Parser)]
pub struct Yank {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
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

#[derive(Parser, Clone)]
pub struct Search {
    // TODO: Figure out output format (like tables)
    #[clap(
        short = 'q',
        long = "query",
        help = "Filter bindles by this query. Typically, the query is a bindle name or part of a name"
    )]
    pub query: Option<String>,
    #[clap(short = 'b', long = "bindle-version", help = "version constraint of the bindle to search for", long_help = VERSION_QUERY)]
    pub version: Option<String>,
    #[clap(
        long = "offset",
        help = "The offset where to start the next page of results"
    )]
    pub offset: Option<u64>,
    #[clap(long = "limit", help = "the limit of results per page")]
    pub limit: Option<u8>,
    #[clap(
        long = "strict",
        help = "Whether or not to use strict mode",
        long_help = "Whether or not to use strict mode. Please note that bindle servers must implement a strict mode per the specification, a non-strict (standard) mode is optional"
    )]
    pub strict: Option<bool>,
    #[clap(
        long = "yanked",
        help = "Whether or not to include yanked bindles in the search result"
    )]
    pub yanked: Option<bool>,
    #[clap(
        short = 'f',
        long = "output-format",
        help = "choose an output format",
        possible_values = &["json", "toml", "table"],
    )]
    pub output: Option<String>,
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

#[derive(Parser)]
pub struct GetParcel {
    #[clap(
        index = 1,
        value_name = "BINDLE_ID",
        help = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
    pub bindle_id: bindle::Id,
    #[clap(
        index = 2,
        value_name = "PARCEL_SHA",
        help = "The SHA256 of the parcel"
    )]
    pub sha: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./parcel.dat",
        help = "The location where to output the parcel to"
    )]
    pub output: PathBuf,
}

#[derive(Parser)]
pub struct GetInvoice {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. example.com/mybindle/1.2.3"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'o',
        long = "output",
        default_value = "./invoice.toml",
        help = "The location where to output the invoice to"
    )]
    pub output: PathBuf,
    #[clap(
        short = 'y',
        long = "yanked",
        help = "Whether or not to fetch a yanked bindle. If you attempt to fetch a yanked bindle without this set, it will error"
    )]
    pub yanked: bool,
}

#[derive(Parser)]
pub struct GenerateLabel {
    #[clap(
        index = 1,
        value_name = "FILE",
        help = "The path to a file. This will generate a label for the file at that path"
    )]
    pub path: PathBuf,
    #[clap(
        short = 'n',
        long = "name",
        help = "The name of the parcel, defaults to the name + extension of the file"
    )]
    pub name: Option<String>,
    #[clap(
        short = 'm',
        long = "media-type",
        help = "The media (mime) type of the file. If not provided, the tool will attempt to guess the mime type. If guessing fails, the default is `application/octet-stream`"
    )]
    pub media_type: Option<String>,
}

#[derive(Parser)]
pub struct CreateKey {
    #[clap(
        index = 1,
        value_name = "LABEL",
        help = "The name of the key, such as 'Matt <me@example.com>'"
    )]
    pub label: String,

    #[clap(
        long = "roles",
        help = "The roles for this key. If multiple roles are specified, they should be comma-delimited with no spaces (e.g. creator,approver)",
        default_value = "creator"
    )]
    pub roles: String,
    #[clap(
        short = 'f',
        long = "secrets-file",
        help = "The path to the file where secrets should be stored. If it does not exist, it will be created. If it does exist, the key will be appended."
    )]
    pub secret_file: Option<PathBuf>,
    #[clap(
        long = "skip-keyring",
        help = "Skip writing the public key to the keychain"
    )]
    pub skip_keyring: bool,
}

#[derive(Parser)]
pub struct AddKey {
    #[clap(
        index = 1,
        value_name = "LABEL",
        help = "The label to use for the key. This should be of the form 'Boaty McBoatface <boaty@example.com>'"
    )]
    pub label: String,
    #[clap(
        index = 2,
        value_name = "ROLES",
        help = "The roles this key should be used for when verifying. If multiple roles are specified, they should be comma-delimited with no spaces (e.g. creator,approver)"
    )]
    pub roles: String,
    #[clap(
        index = 3,
        value_name = "KEY",
        help = "The base64 encoded public key to add"
    )]
    pub key: String,
}

#[derive(Parser)]
pub struct PrintKey {
    #[clap(
        short = 'f',
        long = "secrets-file",
        value_name = "KEYFILE_PATH",
        help = "The path to the private key file. If not set, the default location will be checked."
    )]
    pub secret_file: Option<PathBuf>,
    #[clap(
        short = 'l',
        long = "label",
        value_name = "LABEL",
        help = "The label to search for. If supplied, this will return each key that contains this string in its label. For example, '--label=ample' will match 'label: Examples'."
    )]
    pub label: Option<String>,
}

#[derive(Parser)]
pub struct SignInvoice {
    #[clap(
        index = 1,
        value_name = "INVOICE",
        help = "the path to the invoice to sign"
    )]
    pub invoice: String,
    #[clap(
        short = 'f',
        long = "secrets-file",
        help = "the path to the file where secret keys are stored. Use 'create-key' to create a new key"
    )]
    pub secret_file: Option<PathBuf>,
    #[clap(
        short = 'r',
        long = "role",
        help = "the role to sign with. Values are: c[reator], a[pprover], h[ost], p[roxy]. If no role is specified, 'creator' is used"
    )]
    pub role: Option<String>,
    #[clap(
        short = 'o',
        long = "out",
        help = "the location to write the modified invoice. By default, it will write to invoice-HASH.toml, where HASH is computed on name and version"
    )]
    pub destination: Option<String>,
}

#[derive(Parser)]
pub struct PushInvoice {
    #[clap(
        index = 1,
        value_name = "FILE",
        default_value = "./invoice.toml",
        help = "The path to the invoice TOML file"
    )]
    pub path: PathBuf,
}

#[derive(Parser)]
pub struct PushFile {
    #[clap(index = 1, value_name = "BINDLE_ID")]
    pub bindle_id: bindle::Id,
    #[clap(
        index = 2,
        value_name = "FILE",
        help = "The path to the file that should be pushed as a parcel"
    )]
    pub path: PathBuf,
    #[clap(
        short = 'n',
        long = "name",
        help = "the name of the parcel, defaults to the name + extension of the file"
    )]
    pub name: Option<String>,
    #[clap(
        short = 'm',
        long = "media-type",
        help = "the media (mime) type of the file. If not provided, the tool will attempt to guess the mime type. If guessing fails, the default is `application/octet-stream`"
    )]
    pub media_type: Option<String>,
}

#[derive(Parser)]
pub struct Login {}

#[derive(Parser)]
pub struct Package {
    #[clap(
        index = 1,
        value_name = "BINDLE",
        help = "The name of the bindle, e.g. foo/bar/baz/1.2.3. The sha of this ID should correspond to the standalone bindle directory you wish to package"
    )]
    pub bindle_id: String,
    #[clap(
        short = 'p',
        long = "path",
        default_value = "./",
        help = "A path where the standalone bindle directory is located"
    )]
    pub path: PathBuf,

    #[clap(
        short = 'e',
        long = "export-dir",
        default_value = "./",
        help = "The directory where the packaged standalone bindle should be written"
    )]
    pub export_dir: PathBuf,
}
