use clap::{App, Arg};
use serde::Deserialize;
use sha2::Digest;
use tokio::fs;
use tokio::io::AsyncSeekExt;

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::Path;

const DESCRIPTION: &str = r#"
Transform an AssemblyScript project to a bindle.

This reads a package.json file and the build/ directory to construct a
Bindle from the data.

By default, it attempts to read the local package.json and build/ directory,
and then write the results to a bindir/ directory as a standalone bindle.
"#;

#[tokio::main]
async fn main() {
    let app = App::new("as2bindle")
        .version(clap::crate_version!())
        .author("DeisLabs at Microsoft Azure")
        .about(DESCRIPTION)
        .arg(
            Arg::new("src")
                .help("path to directory with package.json")
                .short('s')
                .long("src")
                .takes_value(true),
        )
        .arg(
            Arg::new("bindir")
                .help("path to bindle directory")
                .short('b')
                .long("bindle")
                .takes_value(true),
        )
        .get_matches();
    let src_dir = app.value_of("src").unwrap_or("./");
    let bindle_dir = app.value_of("bindir").unwrap_or("./");

    let src_path = Path::new(src_dir);
    let bindle_path = Path::new(bindle_dir);

    // Find and read package.json
    let package_json = fs::read(src_path.join("package.json"))
        .await
        .expect("failed to read package.json");
    let package: Package =
        serde_json::from_slice(&package_json).expect("failed to parse package.json");
    // Find target wasm
    let path = src_path.join("build/optimized.wasm");
    if !path.is_file() {
        panic!("no optimized.wasm found in build directory");
    }

    // Calculate the hash of the WASM file
    let mut file = tokio::fs::File::open(&path)
        .await
        .expect("file cannot be opened");
    let mut hasher = bindle::async_util::AsyncSha256::new();
    tokio::io::copy(&mut file, &mut hasher)
        .await
        .expect("hashing file failed");
    let sha = format!("{:x}", hasher.into_inner().unwrap().finalize());
    file.seek(SeekFrom::Start(0))
        .await
        .expect("failed to seek to begining of WASM file");

    let mut parcels = HashMap::new();
    parcels.insert(sha.clone(), file);

    // Create the label object
    let md = tokio::fs::metadata(&path).await.expect("failed to stat");
    let label = bindle::Label {
        name: format!("{}", path.file_name().unwrap().to_string_lossy()),
        media_type: "application/wasm".to_owned(),
        sha256: sha,
        size: md.len() as u64,
        ..bindle::Label::default()
    };

    // Return the parcel section to be added to the invoice.
    // Create invoice
    let mut invoice = bindle::Invoice {
        bindle_version: bindle::BINDLE_VERSION_1.to_owned(),
        yanked: None,
        yanked_signature: None,
        bindle: bindle::BindleSpec {
            id: format!("{}/{}", package.name, package.version)
                .parse()
                .expect("Missing name or version information"),
            authors: None,
            description: package.description,
        },
        parcel: Some(vec![bindle::Parcel {
            label,
            conditions: None,
        }]),
        annotations: None,
        group: None,
        signature: None,
    };

    if let Some(auth) = package.author {
        invoice.bindle.authors = Some(vec![auth])
    }

    // Write invoice
    let standalone =
        bindle::standalone::StandaloneWrite::new(bindle_path, &invoice.bindle.id).unwrap();
    standalone
        .write(invoice, parcels)
        .await
        .expect("unable to write bindle");
    println!("Wrote bindle to {}", standalone.path().display());
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    author: Option<String>,
    description: Option<String>,
}
