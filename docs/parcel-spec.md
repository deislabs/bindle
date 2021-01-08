# The Parcel Specification

A parcel consists of a single `parcel.dat`, which contains opaque parcel data. This file is not inspected by Bindle

A parcel is immutable. Once it is written to a Bindle server, it cannot be altered at all.

The `parcel.dat` files SHOULD be stored in read-only mode.
