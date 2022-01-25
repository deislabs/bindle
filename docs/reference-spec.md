# Reference and Naming Specification

This section of the Bindle specification defines how naming and referencing works.

## Bindle Name (and Version)

A bindle has a name. For example, if we were to write a `hello world` bindle, it may have the name `hello_world`.

When a bindle is concretized into an invoice, it MUST have a name and a version. Version MUST be a valid [SemVer 2 version](https://semver.org).

A bindle name can be one or more characters from the set of unicode alphabetic and numeric characters, as well as the characters `_` and `/`.

TODO: Are there other characters we should allow? `-` causes problems with algorithms that autoconvert between `_` and `-`, because of ambiguity. `+` and `%` are legal in URLs, and % might be useful as an escape character, per URLs. A bindle name MUST NOT start with the string `bindle:` (the `:` is not a legal character in bindle names).

Example bindle names:

- `a`
- `1_b`
- `käse`
- `example.com/hello/world`

Bindle creators SHOULD use a domain syntax for naming, where the domain name is associated with the organization of the creators. Examples:

- `example.com/hello_world`
- `github.com/deislabs/example_bindle`

## Invoice Name

An invoice name is the concatenation of the bindle name and version. The `/` character is used to combine these two pieces of information.

Examples:

- `example.com/hello_world/1.2.3`
- `github.com/deislabs/example_bindle/123.234.34567-alpha.9999+hellothere`

Note that invoice names MUST NOT use a "shortened version" where minor or patch releases are elided out. The following is _illegal_: `example.com/hello/1.0` because the string `1.0` is not a valid SemVer.

NOTE: In the above examples, the domain is _not_ intended as an _authority section_ (using RFC 3986's terminology).

## The URI Syntax

The following URI syntax is supported for describing an _invoice name_:

```
bindle:INVOICE_NAME
```

- The scheme for a Bindle URI is always `bindle:`.
- The INVOICE_NAME is the complete invoice name, encoded according to the rules in [RFC 3986](https://tools.ietf.org/html/rfc3986).

Note that because no part of a bindle name is an _authority section_, the URI MUST NOT follow the schema section with an authority marker (`//`). The following is illegal: `bindle://example.com/foo/1.2.3` because it includes a `//` following the scheme.

Examples of Bindle URIs:

- `bindle:example.com/hello_world/1.2.3`
- `bindle:github.com/deislabs/example_bindle/123.234.34567-alpha.9999+hellothere`

Resolving an invoice name to an actual invoice is up to the Bindle resolver. It may consult local storage or access remote storage.

## Bindle Media Types

When a bindle is transmitted over the network, standard content types should be used. Optionally a `+bindle` may be added.

For example, an invoice TOML file may use the media type `text/toml` or `text/toml+bindle`.
