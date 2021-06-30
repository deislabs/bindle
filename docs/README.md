# Bindle Documentation

## Installing Bindle

### From Source

Prerequisites:

- A recent version of Rust and Cargo

Clone the repository at https://github.com/deislabs/bindle, and then use `make` to build
the binaries:

```console
$ git clone https://github.com/deislabs/bindle.git
$ cd bindle
$ make build
```

You will now have binaries built in `target/debug/bindle` and `target/debug/bindle-server`.

### From the Binary Releases

Every release of Bindle provides compiled releases for a variety of operating systems. These
compiled releases can be manually downloaded and installed. Please note these instructions will work
on Linux, MacOS, and Windows (in PowerShell):

1. Download your desired version from [the releases
   page](https://github.com/deislabs/bindles/releases)
1. Unpack it (`tar -xzf bindle-v0.3.1-linux-amd64.tar.gz`)
1. Move the bindle and bindle CLIs to their desired
   destinations somewhere in your `$PATH` (e.g. `mv bindle bindle-server /usr/local/bin/` on Unix-like
   systems or `mv *.exe C:\Windows\system32\` on Windows)

From there, you should be able to run the client in your terminal emulator. If your terminal cannot
find Bindle, check to make sure that your `$PATH` environment variable is set correctly.

#### Validating

If you'd like to validate the download, checksums can be downloaded from
https://bindle.blob.core.windows.net/releases/checksums-v0.3.1.txt

### From Canary Builds

“Canary” builds are versions of Bindle that are built from `master`. They are not official
releases, and may not be stable. However, they offer the opportunity to test the cutting edge
features before they are released.

Here are links to the common builds:

- [checksum file](https://bindle.blob.core.windows.net/releases/checksums-canary.txt)
- [64-bit Linux (AMD
  architecture)](https://bindle.blob.core.windows.net/releases/bindle-canary-linux-amd64.tar.gz)
- [64-bit macOS (AMD
  architecture)](https://bindle.blob.core.windows.net/releases/bindle-canary-macos-amd64.tar.gz)
- [64-bit Windows](https://bindle.blob.core.windows.net/releases/bindle-canary-windows-amd64.tar.gz)


## Using Bindle

The `bindle` program is the client. The `bindle-server` program is the HTTP server.

Before using the `bindle` client, set the `BINDLE_URL` environment variable:

```console
$ export BINDLE_URL="http://localhost:8080/v1/" 
```

To bootstrap a new bindle instance:

1. Create a directory to store your bindles. We recommend `${HOME}/.bindle/bindles`. You can also
   skip this step and bindles will be stored in the `/tmp` directory
2. Start your `bindle-server`, pointing it to your bindle directory.
3. Create an `invoice.toml`
4. Use the `bindle` client to push the invoice to the server

Here's a concrete version of the above for UNIX-like systems:
```console
$ mkdir -p $HOME/.bindle/bindles
# This step is optional, but will give you a bit more log output if you are curious
$ export RUST_LOG=error,warp=info,bindle=debug
$ bindle-server --directory ${HOME}/.bindle/bindles

# In another terminal window
$ cat <<EOF > invoice.toml
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[annotations]
myname = "myvalue"
EOF
$ export BINDLE_URL="http://localhost:8080/v1/" 
$ bindle push-invoice invoice.toml
Invoice mybindle/0.1.0 created
```

You can verify that this is working by fetching the invoice:

```console
$ bindle info mybindle/0.1.0
# request for mybindle/0.1.0
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
description = "My first bindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]

[annotations]
myname = "myvalue"
```

To learn more about the Bindle command, run `bindle --help`.

## Specification

1. The specification for the Bindle format and design begins with the [Bindle Specification](bindle-spec.md).
2. The [invoice spec](invoice-spec.md) describes the invoice format.
3. The [parcel spec](parcel-spec.md) defines the parcel format.
4. The [label spec](label-spec.md) describes the parcel label format.
5. The [protocol specification](protocol-spec.md) describes the HTTP protocol for storing and retrieving Bindles.
6. Finally, the [Standalone Bindle Specification](standalone-bindle-spec.md) describes a format for packaging up a Bindle into a portable artifact

## Additional Docs

- The [filesystem layout](file-layout.md) for Bindle is not a specification, but is described here
- We have some [special use cases](webassembly.md) for WebAssembly. Many of these concepts apply to other languages/platforms as well.
