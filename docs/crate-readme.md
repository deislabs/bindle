# Bindle - Aggregate Object Storage System

Bindle is an aggregate object storage system used for storing aggregate applications. For more information and examples, see the [README](https://github.com/deislabs/bindle/blob/master/README.md) in the Bindle repo. This README is primarily for those consuming Bindle as an SDK. The main repo README contains information on the specification and server/client binaries.

## Using the crate

Add the following to your `Cargo.toml` dependencies:

```toml
bindle = "0.1"
```

### Features

By default, all of the following features are enabled. To use specific features (for example, just using the client component):

```toml
bindle = { version = "0.1", default-features = false, features = ["client"]}
```

- `client`: The client component of Bindle. This includes a fully featured client SDK.
- `caching` (also enables `client`): An optional caching component for Bindle. Currently, these are just used to keep a local cache of bindles
- `server`: The server side components necessary to run a bindle server
- `test-tools`: A helpful set of testing tools for loading and managing bindles

## Compatibility

While this crate is pre-1.0, we make no guarantees about API stability. However, any breaking API changes will be clearly communicated in release notes in the repo.
