#![macro_use]
extern crate serde;

fn main() -> anyhow::Result<()> {
    println!("Hello, world!");
    server()
}

fn server() -> anyhow::Result<()> {
    /*
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
    config.set_application_protos(quiche::h3::APPLICATION_PROTOCOL)?;
    let h3_config = quiche::h3::Config::new()?;
    let conn = quiche::accept([1], None, config)?;
    let h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
    loop {
        match h3_conn.poll(&mut conn) {
            Ok((stream_id, quiche::h3::Event::Headers { list, has_body })) => {
                let mut headers = list.into_iter();

                // Look for the request's method.
                let method = headers.find(|h| h.name() == ":method").unwrap();

                // Look for the request's path.
                let path = headers.find(|h| h.name() == ":path").unwrap();

                if method.value() == "GET" && path.value() == "/" {
                    let resp = vec![
                        quiche::h3::Header::new(":status", &200.to_string()),
                        quiche::h3::Header::new("server", "quiche"),
                    ];

                    h3_conn.send_response(&mut conn, stream_id, &resp, false)?;
                    h3_conn.send_body(&mut conn, stream_id, b"Hello World!", true)?;
                }
            }

            Ok((stream_id, quiche::h3::Event::Data)) => {
                // Request body data, handle it.
            }

            Ok((stream_id, quiche::h3::Event::Finished)) => {
                // Peer terminated stream, handle it.
            }

            Err(quiche::h3::Error::Done) => {
                // Done reading.
                break;
            }

            Err(e) => {
                // An error occurred, handle it.
                break;
            }
        }
    }
    */
    Ok(())
}
