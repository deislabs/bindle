# File System Layout of a Bindle

Currently, this section is non-normative. It describes the format that the Bindle server uses for filesystem representation of a Bindle.

## Layout

The structure of the Bindle filesystem layout is:

```
BINDIR/
  |
  |- invoices/
  |   |- INVOICE_SHA
  |       |- invoice.toml
  |- parcels/
      |- PARCEL_SHA
         |- label.toml
         |- parcel.dat
```

- `BINDIR` is an arbitrarily named directory for storing bindles
- `INVOICE_SHA` is the hex representation of a SHA-256 hash created by using the canonical invoice name (not the bindle name): NAME/VERSION
  - `NAME` is the Bindle name in the invoice's `bindle` `name` field.
  - `/` is the literal `slash` character. This is not OS-dependent (e.g. Windows does not use the `\` character instead).
  - `VERSION` is the Bindle version in the invoice's `bindle` `version` field.
- `PARCEL_SHA` is the SHA-256 hash of the `parcel.dat` file, represented as a hex string.
