use clap::{App, Arg, SubCommand};

const DESCRIPTION: &str = r#"
The Bindle Client

Bindle is a technology for storing and retrieving aggregate applications.
This program provides tools for working with Bindle servers.
"#;

#[tokio::main]
async fn main() {
    let app = App::new("bindle")
        .version("0.1.0")
        .author("DeisLabs at Microsoft Azure")
        .about(DESCRIPTION)
        .subcommand(
            SubCommand::with_name("info")
                .about("get the bindle invoice and display it")
                .arg(
                    Arg::with_name("BINDLE")
                        .help("the bindle to examine")
                        .required(true),
                ),
        )
        .subcommand(SubCommand::with_name("push").about("push a bindle to the server"))
        .subcommand(SubCommand::with_name("get").about("download the bindle and all its parcels"))
        .subcommand(SubCommand::with_name("yank").about("yank an existing bindle"))
        .subcommand(SubCommand::with_name("search").about("search for bindles"))
        .subcommand(
            SubCommand::with_name("invoice-name")
                .about("display the hashed invoice name for a bindle invoice")
                .arg(
                    Arg::with_name("invoice")
                        .required(true)
                        .help("the path to the invoice to parse"),
                ),
        )
        .get_matches();

    match app.subcommand() {
        ("info", info) => {
            let inv = info
                .map(|i| i.value_of("BINDLE").unwrap_or("NONE"))
                .unwrap_or("hello");
            get_invoice(inv).await
        }
        ("invoice-name", iname) => {
            let inv = iname
                .map(|i| i.value_of("invoice").unwrap_or("./invoice.toml"))
                .unwrap_or("./invoice.toml");

            let invoice_path = std::path::Path::new(inv);
            let raw = std::fs::read_to_string(invoice_path).expect("File not found");
            let invoice = toml::from_str::<bindle::Invoice>(raw.as_str()).unwrap();
            println!(
                "{}",
                bindle::storage::file::canonical_invoice_name(&invoice)
            )
        }
        _ => eprintln!("Not implemented"),
    }
}

async fn get_invoice(name: &str) {
    match reqwest::get(to_url(name).as_str()).await {
        Ok(res) => match res.status() {
            reqwest::StatusCode::NOT_FOUND => exit(format!("'{}' not found", name)),
            reqwest::StatusCode::OK => println!(
                "# request for {}\n{}",
                name,
                res.text().await.unwrap_or_else(|_| "ERROR".to_owned())
            ),
            _ => exit("Unsupported status"),
        },
        Err(err) => exit(err),
    }
}

fn exit<T: std::fmt::Display>(msg: T) {
    eprintln!("Error: {}", msg);
    std::process::exit(1)
}

fn to_url(bindle: &str) -> String {
    // TODO: How do we want to do this part?
    format!("http://localhost:8080/v1/_i/{}", bindle)
}
