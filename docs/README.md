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
1. Unpack it (`tar -xzf bindle-v0.5.0-linux-amd64.tar.gz`)
1. Move the bindle and bindle CLIs to their desired
   destinations somewhere in your `$PATH` (e.g. `mv bindle bindle-server /usr/local/bin/` on Unix-like
   systems or `mv *.exe C:\Windows\system32\` on Windows)

From there, you should be able to run the client in your terminal emulator. If your terminal cannot
find Bindle, check to make sure that your `$PATH` environment variable is set correctly.

#### Validating

If you'd like to validate the download, checksums can be downloaded from
https://bindle.blob.core.windows.net/releases/checksums-v0.4.1.txt

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

### Configuring Authentication

There are currently two authentication types supported by Bindle:

- GitHub OAuth2
- HTTP Basic Auth (username/password)

To use GitHub OAuth2, you must supply `--github-client-id` and `--github-client-secret` at startup.

To use HTTP Basic authentication, you must generate an `htpasswd` file using Bcrypt:

```console
$ htpasswd -Bc htpasswd admin
New password: 
Re-type new password: 
Adding password for user admin
```

(The `htpasswd` program comes on many Linux/Unix distros. Officially it is part of the Apache Web Server project.)

Then you need to supply the `--htpasswd-file` option at `bindle-server` startup:

```console
$ bindle-server --htpasswd-file test/data/htpasswd
```

> Currently, only bcrypt is supported in htpasswd files. At the time of this writing, bcrypt is the most secure algorithm supported by htpasswd.

### Configuring Signing

Keys are used for signing and verification.
Keys are stored as base64-encoded Ed25519 keys inside of specially formatted TOML files.

#### Verification Keys

Verification keyrings are used to verify signatures.
They contain annotated _public keys_.
You should add the keys from trusted creators, hosts, proxies, and verifiers to this file.

The file exists in your system's `XDG_DATA_DIR` in the `bindle` subdirectory.
(On a mac, this will be something like `$HOME/Library/Application\ Support/bindle/`.
On Linux, it will be `$HOME/.local/share`.)

The file's content looks something like this:

```toml
version = "1.0"

[[key]]
label = "Matt <example@example.com>"
key = "SOME_PUBLIC_KEY"
roles = ["creator"]

[[key]]
label = "bindle.example.com"
key = "SOME_PUBLIC_KEY"
roles = ["host"]
```

At the bare minimum, a key file must have the `version = "1.0"` line.
Each entry represents a key that you trust.
The `label` is a user-friendly string that tells about the key.
The `roles` lists all of the roles that you trust this key to perform (`creator`, `host`, `proxy`, and `verifier`).
The `key` is a base64-encoded Ed25519 public key.
Some keys have a signed piece of metadata called `label_signature`.
This field contains a base64-encoded signature of the label, ensuring that the label has not been changed.
The signature is generated using the private key that corresponds to this public key,
thus ensuring that (a) only the keyholder can create this signature, and (b) anyone with
the `key` entry can verify the signature.
This adds additional trust, because it ensures that the label is the label that the keyholder desired.
However, in many cases this additional level of trust may not be necessary or desired.

#### Signing Keys

A _signing key_ is used when Bindle needs to sign something.
The file `secret_keys.toml` is created in your system's `XDG_DATA_DIR`, in the `bindle/` subdirectory.
(On a mac, this will be something like `$HOME/Library/Application\ Support/bindle/`.
On Linux, it will be `$HOME/.local/share`.)

The file's content will look something like this:

```toml
version = "1.0"

[[key]]
label = "Matt <example@example.com>"
keypair = "KEYDATA_GOES_IN_HERE"
roles = ["creator"]
```

A user only needs on such keypair (though a user is free to have more).
This file can be moved from system to system, just like OpenPGP or SSH key sets.

- To create a signing key for a client, use `bindle create-key`
- By default, if Bindle does not find an existing keyring, it creates one of these when it first starts.

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
