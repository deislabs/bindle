use bindle::client;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("BINDLE_SERVER_URL")?;
    let root = std::env::var("CARGO_MANIFEST_DIR")?;
    let root_path = std::path::PathBuf::from(root);

    let bindle_client = client::Client::new(&url)?;

    // Load an invoice manually and send it to the server
    println!("Creating invoice 1");
    let inv = toml::from_slice(
        &tokio::fs::read(root_path.join("tests/scaffolds/valid_v1/invoice.toml")).await?,
    )?;
    let inv = bindle_client.create_invoice(inv).await?;
    println!("{:?}", inv);

    // Load an invoice from file and stream it to the API
    println!("Creating invoice 2");
    let inv = bindle_client
        .create_invoice_from_file(root_path.join("tests/scaffolds/valid_v2/invoice.toml"))
        .await?;
    println!("{:?}", inv);

    // Get one of the created invoices
    println!("Getting invoice 1");
    let inv = bindle_client
        .get_invoice("enterprise.com/warpcore/1.0.0")
        .await?;
    println!("{:?}", inv);

    // Query the API for a specific version
    println!("Querying for invoice 1");
    let matches = bindle_client
        .query_invoices(bindle::QueryOptions {
            query: Some("enterprise.com/warpcore".to_string()),
            version: Some("1.0.0".to_string()),
            ..Default::default()
        })
        .await?;
    println!("{:?}", matches);

    // Upload a parcel by loading the file into memory
    println!("Creating parcel 1");
    let label = toml::from_slice(
        &tokio::fs::read(root_path.join("tests/scaffolds/valid_v1/parcels/parcel.toml")).await?,
    )?;
    let data =
        tokio::fs::read(root_path.join("tests/scaffolds/valid_v1/parcels/parcel.dat")).await?;
    let label = bindle_client.create_parcel(label, data).await?;
    println!("{:?}", label);
    let first_sha = label.sha256;

    // Upload a parcel using a stream instead of loading into memory
    println!("Creating parcel 2");
    let label = toml::from_slice(
        &tokio::fs::read(root_path.join("tests/scaffolds/valid_v2/parcels/parcel.toml")).await?,
    )?;
    let label = bindle_client
        .create_parcel_from_file(
            label,
            root_path.join("tests/scaffolds/valid_v2/parcels/parcel.dat"),
        )
        .await?;
    println!("{:?}", label);
    let second_sha = label.sha256;

    // Get a parcel and load its bytes into memory
    println!("Loading parcel 1");
    let data = bindle_client.get_parcel(&first_sha).await?;
    println!("{}", data.len());

    // Get a parcel as a stream, and write it into a file somewhere
    println!("Loading parcel 2 as stream");
    let temp = tempdir()?;
    let mut stream = bindle_client.get_parcel_stream(&second_sha).await?;

    let file_path = temp.path().join("foo");
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&file_path)
        .await?;

    while let Some(data) = stream.next().await {
        let data = data?;
        file.write_all(&data).await?;
    }
    file.flush().await?;

    // Read the whole file and make sure we got it
    assert_eq!(tokio::fs::read(file_path).await?, b"a green one");

    Ok(())
}
