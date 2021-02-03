# Bindle Documentation

## Installing Bindle

### From the Binary Releases

Every release of Bindle provides compiled releases for a variety of operating systems. These
compiled releases can be manually downloaded and installed. Please note these instructions will work
on Linux, MacOS, and Windows (in PowerShell):

1. Download your desired version from [the releases
   page](https://github.com/deislabs/bindles/releases)
1. Unpack it (`tar -xzf bindle-v0.3.0-linux-amd64.tar.gz`)
1. Move the bindle and bindle CLIs to their desired
   destinations somewhere in your `$PATH` (e.g. `mv bindle bindle-server /usr/local/bin/` on Unix-like
   systems or `mv *.exe C:\Windows\system32\` on Windows)

From there, you should be able to run the client in your terminal emulator. If your terminal cannot
find Bindle, check to make sure that your `$PATH` environment variable is set correctly.

#### Validating

If you'd like to validate the download, checksums can be downloaded from
https://bindle.blob.core.windows.net/releases/checksums-v0.3.0.txt

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

To bootstrap a new bindle instance:

- Create a Bindle invoice, as described in the specification. See the test data in this repo for examples.
- Create a directory to store you bindle, for example `mkdir -p _scratch/bindir/invoices`
- Use the `bindle invoice-name` command to generate a SHA of the bindle
- Create a directory in the `invoices` folder whose name is the SHA generated above
- Copy your invoice into that directory, naming the file `invoice.toml`
- Start your `bindle-server`, pointing it to your bindle directory.

Here's a compact version of the above:
```console
$ edit invoice.toml
$ cat invoice.toml
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[annotations]
myname = "myvalue"
$ mkdir -p _scratch/bindir/invoices/$(bindle invoice-name ./invoice.toml)
$ mv invoice.toml _scratch/bindir/invoices/$(bindle invoice-name ./invoice.toml)
$ bindle-server-d _scratch/bindir
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