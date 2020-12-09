//! Tests for the client. These are integration style tests and, as such, they run through
//! entire user workflows

use bindle::client::Client;

const DEFAULT_SERVER: &str = "http://127.0.0.1:8080/v1/";

struct TestController {
    pub client: Client,
    server_handle: std::process::Child,
    // Keep a handle to the tempdir so it doesn't drop until the controller drops
    _tempdir: tempfile::TempDir,
}

impl TestController {
    fn new() -> TestController {
        let build_result = std::process::Command::new("cargo")
            .args(&["build", "--all-features"])
            .output()
            .expect("unable to run build command");

        assert!(
            build_result.status.success(),
            "Error trying to build server {}",
            String::from_utf8(build_result.stderr).unwrap()
        );

        let tempdir = tempfile::tempdir().expect("unable to create tempdir");

        let server_handle = std::process::Command::new("cargo")
            .args(&[
                "run",
                "--features",
                "cli",
                "--bin",
                "bindle-server",
                "--",
                "-d",
                tempdir.path().to_string_lossy().to_string().as_str(),
            ])
            .spawn()
            .expect("unable to start bindle server");

        let client = Client::new(DEFAULT_SERVER).expect("unable to setup bindle client");
        TestController {
            client,
            server_handle,
            _tempdir: tempdir,
        }
    }
}

impl Drop for TestController {
    fn drop(&mut self) {
        // Not much we can do here if we error, so just ignore
        let _ = self.server_handle.kill();
    }
}

// TODO: client tests
