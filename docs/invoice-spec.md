# The Invoice

This specification describes the invoice (`invoice.toml`).

An invoice is the top-level descriptor of a bindle. Every bindle has exactly one invoice.

```toml
[bindle]
name = "mybindle"
version = "v0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[annotations]
myname = "myvalue"

[boxes]
e1706ab0a39ac88094b6d54a3f5cdba41fe5a901 = {}
098fa798779ac88094b6d54a3f5cdba41fe5a901 = {}
```

The above bindle declares its metadata, and then declares a manifest containing two boxes.

## `bindle` Fields

- name: Alpha-numeric name of the bindle, designed for humans
- version: SemVer version, with the `v` prefix recommended, but not required
- authors: Optional list of authors, where each field is a string conventionally containing a name and email address
- description: A one-line description

## `annotations` Fields

The `annotations` section contains arbitrary name/value pairs. Implementors of the Bindle
system may use this section to store custom configuration.

Implementations MUST NOT add fields anywhere else in the invoice except here and in the
`annotations` field of a bundle label.

## `boxes` Fields

Currently, `boxes` contains a map of objects, where the name is the SHA256 of the box, and
the object contains additional information.

At the time of this writing, no additional information is defined. In the future, this
is likely to hold the contents of the box's `label.toml` object