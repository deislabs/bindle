# Standalone Bindle Specification

A standalone Bindle is an artifact containing all the information necessary to represent a complete or partial (meaning missing parcels) bindle. It is meant to be used in both exporting and importing/publishing Bindles, which also means it could be used in airgapped situations.

## Directory Layout

```
INVOICE_SHA/
  |- invoice.toml
  |- parcels/
      |- PARCEL_SHA.dat
```

- `INVOICE_SHA` is a directory named with the SHA-256 hash of the invoice. It MUST be the SHA-256 hash created by hashing the canonical invoice name (not the bindle name): NAME/VERSION
  - `NAME` is the Bindle name in the invoice’s bindle name field. This can be arbitrarily pathy (e.g. `example.com/foo/bar`)
  - `/` is the literal slash character. This is not OS-dependent (e.g. Windows does not use the `\` character instead).
  - `VERSION` is the Bindle version in the invoice’s bindle version field.
- `invoice.toml` MUST exist and MUST contain a valid TOML [invoice specification](invoice-spec.md).
- `PARCEL_SHA` is the SHA-256 hash of the parcel file, represented as a hex string. For example, if you had a text file containing `a red one`, it would be named in the `parcels` directory as `23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5.dat`. 

The `parcels` directory MUST exist, but MAY be empty or contain multiple parcels. Each parcel file MUST be named according to the specification described above.

### Tarballs

A standalone Bindle MAY be compressed into a `.tar.gz` file (i.e. tarball). However, it MUST expand into the same directory structure as described in the previous section. Implementations MAY, but are not required to, support the tarball format.

## Sending a Standalone Bindle

Items in a standalone Bindle MAY be sent to a Bindle server. Implementations SHOULD first create the invoice and use the returned list of missing parcels (if there are any) to selectively send only the needed parcels to the Bindle server. This is recommended to avoid consuming bandwidth while possibly sending large amounts of data to the bindle server that isn't needed.

In cases where the invoice already exists, implementations SHOULD take advantage of the relationships (`/_r`) endpoint to check if the parcel already exists before uploading. This is recommended to avoid consuming bandwidth while possibly sending large amounts of data to the bindle server that isn't needed.

## Additional Notes

A standalone Bindle need not contain all (or any) of the parcels specified in the `invoice.toml` file. Tooling MUST NOT assume that all the parcels will exist. Bindle implementations MAY handle missing parcels differently (e.g. one may return an error while another allows for fetching the missing parcel from a Bindle server).
