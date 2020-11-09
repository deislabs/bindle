# Bindle Documentation

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

The specification for the Bindle format and design begins with the [Bindle Specification](bindle-spec.md).
The [invoice spec](invoice-spec.md) describes the invoice format.
The [parcel spec](parcel-spec.md) defines the parcel format.
The [label spec](label-spec.md) describes the parcel label format.
Finally, the [protocol specification](protocol-spec.md) describes the HTTP protocol for storing and retrieving Bindles.