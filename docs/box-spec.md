# The Box Specification

A box consists of two parts:

- a `label.md` which contains information about what is in the box
- a `box.dat` which contains opaque box data. This file is not inspected by Bindle

A box is immutable. Once it is written to a Bindle server, it cannot be altered at all.
The Bindle server MAY modify the `label.md` at write-time if incomplete or incorrect data
is supplied. For example, the file size of the `box.dat` may be overwritten.

The box files SHOULD be stored in read-only mode.