# The Invoice

This specification describes the invoice (`invoice.toml`).

An invoice is the top-level descriptor of a bindle. Every bindle has exactly one invoice.

```toml
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[annotations]
myname = "myvalue"

[[parcel]]
label.sha256 = "e1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
label.mediaType = "text/html"
label.name = "myparcel.html"
label.size = 248098

# Experimental support for conditional inclusions
conditions.memberOf = ["server"]

[[parcel]]
label.sha256 = "098fa798779ac88094b6d54a3f5cdba41fe5a901"
label.name = "style.css"
label.mediaType = "text/css"
label.size = 248098

[[parcel]]
label.sha256 = "5b992e90b71d5fadab3cd3777230ef370df75f5b"
label.mediaType = "application/x-javascript"
label.name = "foo.js"
label.size = 248098
label.origin = "footastic 1.2.3"
```
(Source)[../test/data/simple-invoice.toml]

The above bindle declares its `bindle` description, and then declares a manifest containing three parcels.

## Top-level Fields

- `bindleVersion` is required, and should be `1.0.0` for this version of the specification.
- `yanked` is a boolean field that indicates whether a Bindle has been yanked. This field appears outside of the `bindle` because it is mutable, though it can only be toggled on. Once set to true, a Bindle MUST NOT be un-yanked. A yanked bundle should never be served in an index or search, but MAY be accessed directly.
- `yanked_reason` (OPTIONAL) is a string field in which a human-readable reason can be given for yanking the invoice.

## `bindle` Fields

- `name`: Alpha-numeric name of the bindle, designed for humans (REQUIRED). The [reference specification](reference-spec.md) describes the allowable characters.
- `version`: [SemVer](https://semver.org) version (REQUIRED). The SemVer specification describes the allowable characters.
- `authors`: Optional list of authors, where each field is a string conventionally containing a name and email address (OPTIONAL)
- `description`: A one-line description intended to be viewed by end users (OPTIONAL)

## `signature` and `yanked_signature` Lists

A `signature` list contains all signature blocks for a bindle's content.
A `yanked_signature` list contains all signature blocks that sign a yank operation.

These sections are described in detail in the [Signing Specification](signing-spec.md).

## `annotations` Fields

The `annotations` section contains arbitrary name/value pairs. Implementors of the Bindle system may use this section to store custom configuration.

The annotations section is OPTIONAL.

Implementations MUST NOT add fields anywhere else in the invoice except here and in the `annotations` field of a bundle label.

## `parcel` List

In TOML, a list header (`[[parcel]]`) precedes each list item. Each parcel is a separate `[[parcel]]` entry.

Currently, each `[[parcel]]` contains `label` object (see [the label spec](label-spec.md)). Implementations SHOULD use the SHA-256 or SHA-512 on the label item to identify or validate the appropriate parcel.

A `[[parcel]]` item may also include `conditions`. Conditions are not part of the parcel itself, and thus only appear on the invoice. They are markers that the given parcel object may have additional conditions for consideration when composing the parcels into a whole.

### `group` Lists and `conditions` Fields

It may be the case that not all of the parcels in a bindle are _required_. It may be the case that some are optional (based on undefined criteria) or that only one of N choices may be necessary.

To support such combinations, bindles provide the concept of a top-level `group` object and various `condition`s that can be attached to individual parcels within an invoice.

#### Groups

A group is a top-level organization object that may contain zero or more parcels. Every parcel belongs to at least one group, but may belong to others.

An implicit global group exists. It has no name, and includes _only_ the parcels that are not assigned to any other group. There is no mechanism for explicitly assigning a parcel to the unnamed global group.

For any explicitly created group, it is empty by default. Parcels must be placed into a group using conditions, which are discussed later in this document.

The `[[group]]` list is used to create a group. In the following example, three groups are defined: `server`, `cli`, and `utility`.

```toml
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
version = "0.1.0"

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
  - `optional` (`anyOf`): The runtime may decide whether to install any of the parcels in this group.

The combination of `required` and `satisfiedBy` makes grouping a powerful way to compose bindles from parcels.

The `required` field indicates whether a Bindle processor must process the group. A group that is not required need not be processed at all. A group that is required MUST be processed. (The global unnamed group is unalterably required.)

The `required` field above presents one of two ways to mark a group as required. The other, discussed below, is for another required parcel to indicate this group in its `requires` field.

The `satisfiedBy` field indicates the conditions under which a group's `required` state may be considered fulfilled. By default, if a group is required, then all of its parcels are also required. It is also possible to state that a group is satisfied if _one_ parcel is selected. Currently, it is possible to mark a group's satisfaction as `optional`, which means that the group can be satisfied even if none of the parcels in the group are selected. This provision is in place to provide a feature present in some package managers that _recommend_ particular dependencies, but don't force the user to select the recommended dependencies.

#### Parcels and Conditions

Inside of an invoice, a `[[parcel]]` describes a parcel that is considered part of the bindle. A parcel's `label` points to the actual Bindle parcel. But a parcel record in the invoice may also declare zero or more `conditions`.

The `conditions` associate parcels to groups, and they can work one of two ways: They may indicate that a given parcel is part of a group, or they may indicate that this parcel requires another group.

Between groups and conditions, it is possible to build tree-like dependency structures.

> In the present draft, groups and conditions are acyclic. A group cannot include a parcel that depends upon that group (directly or indirectly). Bindle runtimes SHOULD produce an error when a cycle is detected.

By default, if no condition is provided, an item is a member of the "global" group, and is required.

- `memberOf`: A list of groups that this parcel is a member of. When a nonempty `memberOf` clause is present, the parcel is removed from the default global group and placed into _just_ the groups listed in the `memberOf` clause.
  - If `memberOf` is not present, or `memberOf = []`, the parcel is a member of the "global" group.
  - It is an error if a parcel references a group that is undefined in the `[[group]]` list. (OPTIONAL)
  - In Bindle, it is impossible for a parcel to be a member of no groups.
- `requires`: A list of other groups that must be satisfied if this parcel is installed. This has the effect of setting `require = true` on a group. (OPTIONAL)

Example:

```toml
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
version = "0.1.0"

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

In the example above, three groups are declared:

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

## Provenance

In our dealing with provenance, we are particularly interested in the level of cryptographic trust that provenance can enhance.
We are not interested in provenance for other reasons (such as, for example, digital rights management).

One anticipated usage of Bindle supposes that parcels from one invoice will be re-used (intentionally) within another invoice.
For example, an application might fetch a dependency (as a bindle), and then include some or all of that dependency's parcels in its own invoice.

In cases such as this, it is useful to retain sufficient information to reconstruct provenance.

For example, say an application called `mybindle 0.1.0` uses a parcel originally from `footastic 1.2.3`.
The `footastic 1.2.3` bindle has information that is useful to retain:

- Original metadata
- Signatures

But that information is specific to the original invoice.
Transferring the signature from the original invoice to the present invoice would not make sense,
because signatures are contextual.
A better solution is for a bindle to include references to any other bindles used in constructing the package.

For this reason, the `[[parcel]]` description includes an `origin` field that can point to the original invoice from which the parcel was derived.

Given this field, an auditing agent can fetch a bindle, and then for each parcel with an origin, fetch the referent.
From there, it can reconstruct the provenance of the parcels in a bindle.

Agents SHOULD include `origin` information on each parcel that is known to have come from another bindle.

As a technical detail, there is no guarantee that just because two parcels are identical in content, they share provenance.

Consider the trivial, but nearly universal example:

```
Hello, world!
```

It is possible that multiple invoices might all use a parcel with exactly the same content,
but not through shared provenance. Bindle (as a virtue of the system) would crate and maintain
only one internal parcel for all of the different invoices that reference the file whose
contents are `Hello, world!`.

Second, the provenance trail is discretionary, and directed from referrer to referent.
In other words, the invoice that uses parcels that were obtained via another invoice is
the thing that references the other invoice.
Therefore, the author of an invoice may omit reference to the origin, and this situation
becomes undetectable in the system. This is not a problem in trust-based provenance systems,
since in such situations, removing the `origin` is harmful to a provenance evaluation.

For these two reasons, Bindle's provenance system is not particularly well suited for digital rights management.
