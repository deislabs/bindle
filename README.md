# Bindle: Aggregate Object Storage

This repository is a :100: experimental code created by the DeisLab team on a whim. We
really don't think you should use this in production.

## Manifesto

A specter is haunting the cloud-native ecosystem -- the specter of object storage. For too
long, we have stored our binary artifacts using technologies now decades old. Old
protocols, poor security, and complicated JSON have resulted in inferior performance and
dangerous information leaks.

Bindle is a next-generation object storage system based on HTTP/3, strong TLS encryption,
Merkel-tree hashing, and immutable storage. While inspired in part by OCI registries, it
remedies the common problems of that system. Hashing is not volatile, there are no "mutable
tags", SemVer is supported first-class, and access controls are hierarchical.

## Using Bindle

This is a Rust project. Use `cargo run` to execute Bindle.

## Concepts

In the Bindle system, the term _bindle_ refers to a _bundle of related data called parcels_.
A _bindle_ might be simple, containing only a single binary file. Or it may be complex, 
containing hundreds of discrete data objects (files, libraries, or whatnot). It can
represent a layer diagram, like Docker, or just a regular file download. With experimental
conditions, it can even represent packages containing mandatory, optional, and conditional
components.

A bindle is composed of several parts:

- The _invoice_ (`invoice.toml`) contains information about the bindle (`name`, `description`...)
  as well as a manifest of parcels (individual data items).
- A _parcel_ contains two parts:
  - The _label_ (`label.toml`) that contains data about the parcel
  - the _parcel contents_ (`box.dat`) that contains the opaque data ("what's in the box")

A _bindle hub_ is a service that manages storage and retrieval of bindles. It is available
via an HTTP/3 connection (almost always over TLS). A hub supports the following actions:

- GET: Get a bindle and any of its parcels that you don't currently have
- POST: Push a bindle and any of its parcels that the hub currently doesn't have
- DELTE: Remove a bindle

Note that you cannot modify any part of a bindle. Not the payload. Not the name. Not even
the description. Bindles are truly immutable. It's like the post office: Once you ship a
package, you can't go back and change it. This greatly increases the security of the
entire system.

### Bindle Names

There are many fancy naming conventions in the world. But Bindle eschews the fancy in
favor of the easy. Bindle names are _paths_. The following are all valid bindle names:

- `mybindle`
- `mybindle.txt`
- `example.com/stuff/mybindle`
- `mybindle/v1.2.3`

While all of the above are valid bindles, those that end with a version string (a SemVer)
have some special features. Thus, we recommend using versioned bindle names:

- `mybindle/v1.0.0`
- `mybindle/v1.0.1-beta.1+ab21321`
- `example.com/stuff/mybindle/v1.2.3`

### First-class Semver

One frequently used convention in the software world is _versioning_. And one standard for
version numbering is called [SemVer](https://semver.org). Bindle supports SemVer queries
as a way of locating "near relatives" of bindles.

For example, searching for `v1.2.3` of a bindle will return an exact version. Searching
for `v1.2` will return the latest patch release of the 1.2 version of the bindle (which
might be `v1.2.3` or perhaps `v1.2.4`, etc).

Version ranges must be explicitly requested _as queries_. A direct fetch against `v1.2`
will only return a bindle whose version string is an exact match to `v1.2`. But a version
query for `v1.2` will return what Bindle thinks is the most appropriate matching version.

## The Bindle Specification

The [docs](docs/) folder on this site contains the beginnings of a formal specification
for Bindle. The best place to start is with the [Bindle Specification](docs/bindle-spec.md).

## Okay, IRL what's a "bindle"

The word "bindle" means a cloth-wrapped parcel, typically of clothing. In popular U.S. 
culture, hobos were portrayed as carrying bindles represented as a stick with a
handkerchief-wrapped bindle at the end. Also, it sounds cute.