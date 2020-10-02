# The Invoice

This specification describes the invoice (`invoice.toml`).

An invoice is the top-level descriptor of a bindle. Every bindle has exactly one invoice.

```toml
bindleVersion = "v1.0.0"

[bindle]
name = "mybindle"
version = "v0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[annotations]
myname = "myvalue"

[[parcel]]
label.sha256 = "e1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
label.mediaType = "text/html"
label.name = "myparcel.html"

# Experimental support for conditional inclusions
conditions.memberOf = ["server"]

[[parcel]]
label.sha256 = "098fa798779ac88094b6d54a3f5cdba41fe5a901"
label.name = "style.css"
label.mediaType = "text/css"

[[parcel]]
label.sha256 = "5b992e90b71d5fadab3cd3777230ef370df75f5b"
label.mediaType = "application/x-javascript"
label.name = "foo.js"
label.size = 248098
```
(Source)[../test/data/simple-invoice.toml]

The above bindle declares its metadata, and then declares a manifest containing three parcels.

## Top-level Fields

- `bindleVersion` is required, and should be `v1.0.0` for this version of the specification.
- `yanked` is a boolean field that indicates whether a Bindle has been yanked. This field appears outside of the `bindle` because it is mutable, though it can only be toggled on. Once set to true, a Bindle MUST NOT be un-yanked. A yanked bundle should never be served in an index or search, but MAY be accessed directly.

## `bindle` Fields

- name: Alpha-numeric name of the bindle, designed for humans (REQUIRED)
- version: SemVer version, with the `v` prefix recommended, but not required (REQUIRED)
- authors: Optional list of authors, where each field is a string conventionally containing a name and email address (OPTIONAL)
- description: A one-line description (OPTIONAL)

## `annotations` Fields

The `annotations` section contains arbitrary name/value pairs. Implementors of the Bindle system may use this section to store custom configuration.

The annotations section is OPTIONAL.

Implementations MUST NOT add fields anywhere else in the invoice except here and in the `annotations` field of a bundle label.

## `parcel` List

In TOML, a list header (`[[parcel]]`) precedes each list item. Each parcel is a separate `[[parcel]]` entry.

Currently, each `[[parcel]]` contains `label` object (see [the label spec](label-spec.md)). Implementations SHOULD use the SHA-256 or SHA-512 on the label item to identify or validate the appropriate parcel.

### `group` Lists and `conditions` Fields

EXPERIMENTAL--may be completely removed.

It may be the case that not all of the parcels in a bindle are _required_. It may be the case that some are optional (based on undefined criteria) or that only one of N choices may be necessary.

To support such combinations, bindles provide the concept of a `group` and various `condition`s that can be attached to each parcel.

By default, all parcels are part of the global (unnamed) group and are required. Groups are only necessary when composing bindles that have optional or conditional parcels.

The `[[group]]` list is used to create a group. In the following example, three groups are defined: `server`, `cli`, and `utility`.

```toml
bindleVersion = "v1.0.0"

[bindle]
name = "mybindle"
version = "v0.1.0"

[[group]]
name = "server"
satisfiedBy = "allOf"

[[group]]
name = "cli"
satisfiedBy = "oneOf"
required = true

[[group]]
name = "utility"
satisfiedBy = "optional"
```

Group fields:

- `name`: The name of the group (REQUIRED)
- `required`: A boolean flag listing whether this group must be processed. By default, ONLY the global group is required. This must be set to `true` to require this group to be processed. Otherwise, `conditions` fields on a parcel may trigger inclusion of a group. Tools MAY allow groups to be toggled on or off. For example, a client may allow the user to request that the `server` group be installed even though it is not required.
- `satisfiedBy`: The criterion by which this group's requirements can be sat to be satisfied. Possible values are:
  - `allOf` (DEFAULT): All of the packages in this group are required
  - `oneOf`: The bindle requirements are satisfied if at least one of the parcels is present
  - `optional` (`anyOf`): The runtime may decide whether to install any of the parcels in this group  TODO: Can this be removed?

The members of the `[[parcel]]` list may declare themselves to be members of zero or more groups.

With a `[[parcel]]` definition, a parcel may use a `conditions` object to express its inclusion in a group.

By default, if no condition is provided, an item is a member of the "global" group, and is required.

- `memberOf`: A list of groups that this parcel is a member of. When a `memberOf` clause is present, the parcel is removed from the default global group and placed into _just_ the groups listed in the `memberOf` clause. `memberOf = []` indicates that this parcel is a member of no groups (including the global group). It is an error if a parcel references a group that is undefined in the `[[group]]` list. (OPTIONAL)
- `requires`: A list of other groups that must be satisfied if this parcel is installed. This has the effect of setting `require = true` on a group. (OPTIONAL)

Example:

```toml
bindleVersion = "v1.0.0"

[bindle]
name = "mybindle"
version = "v0.1.0"

[[group]]
name = "server"
satisfiedBy = "allOf"

[[group]]
name = "cli"
satisfiedBy = "oneOf"
required = true

[[group]]
name = "utility"
satisfiedBy = "optional"

[[parcel]]
label.sha256 = "e1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
label.mediaType = "application/bin"
label.name = "daemon"
conditions.memberOf = ["server"]
conditions.requires = ["utility"]

# One of a group
[[parcel]]
label.sha256 = "e1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
label.mediaType = "application/bin"
label.name = "first"
conditions.memberOf = ["cli", "utility"]

[[parcel]]
label.sha256 = "a1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
label.mediaType = "application/bin"
label.name = "second"
conditions.memberOf = ["cli"]

[[parcel]]
label.sha256 = "5b992e90b71d5fadab3cd3777230ef370df75f5b"
label.mediaType = "application/x-javascript"
label.name = "third"
conditions.memberOf = ["utility"]
```
(Source)[../test/data/full-invoice.toml]

IN the example above, three groups are declared:

- server
- cli
- utility

Only the `cli` must be installed with this bindle.

Four parcels are listed:

- daemon (member of server)
- first (member of cli and utility)
- second (member of cli)
- third (member of utility)

In the example above, only one of the members of `cli` needs to be installed, because only the `cli` group is required. The group states that `oneOf` the group parcels must be installed before the group is satisfied.

To satisfy `cli`, then, either `first` or `second` must be processed.

So either `first` or `second` can be installed.

If the `server` group is installed (for example, if a user requests that group be installed), then the `daemon` parcel will be installed. However, installing that will also `require` the `utility` group. This creates an interesting case:

- if `first` is chosen to satisfy `cli`, then it also satisfied `utility`.
- if `second` is chosen to satisfy `cli`, then one of `first` or `third` must be processed to satisfy the `utility` group.