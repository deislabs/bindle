# Parcel scaffolds

This directory contains various scaffolds for use in testing. It has a specific structure that must
be adhered to so the test helpers can load it properly. Each directory contains all the data for a
complete bindle. That directory is structured like so:

```
<DIRNAME>
├── invoice.toml
└── parcels
    ├── first.dat
    └── second.dat
```

The `invoice.toml` file should contain the invoice, and the parcels directory should contain all of
the parcels you want to create that are connected to that invoice. Each parcel should have an opaque
`<parcel_name>.dat` file that contains the actual data to be uploaded for the parcel. If the
`parcels` directory is non-existent, it will assume there are no parcels to upload.

## Key management

As Bindle requires signing and keyrings, you will need to sign your scaffolding invoices. A
`KeyRing` will be automatically loaded if the `keys` directory exists in the `scaffolds` directory
and that directory contains a `keyring.toml` file. Likewise, secret keys for signing will be loaded
automatically if the `keys` directory exists in the `scaffolds` directory and that directory
contains a `secret_keys.toml` file
