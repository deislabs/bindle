# The Parcel Specification

A parcel consists of two parts:

- a `label.toml` which contains information about what is in the parcel
- a `parcel.dat` which contains opaque parcel data. This file is not inspected by Bindle

A parcel is immutable. Once it is written to a Bindle server, it cannot be altered at all.

The Bindle server MAY modify the `label.toml` at write-time if incomplete or incorrect data is supplied. For example, the file size of the `parcel.dat` may be overwritten. But the `parcel.dat` itself must be immutable.

The `parcel.dat` files SHOULD be stored in read-only mode.
