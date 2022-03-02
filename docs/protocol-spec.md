# The Bindle Protocol

Bindle uses HTTP/2 with TLS as a transport protocol. All bodies and responses expect to use TOML, with the `application/toml` content type. Other content types may be supported (such as the reference server supporting `application/json`), but are not required by the specification

The HTTP endpoints defined above MAY exist as a subpath on a server, or in the server's root. For example, `https://example.com/v1/_i/foo` and `https://example.com/_i/foo` are both legal paths for the specification below. However, `https://example.com/_i/v1/foo` is not (or, rather, it is a legal URI for a package named `v1/foo`).

HTTP Endpoints:
- `/_i/{bindle-name}`: The path to a bindle. Note that `{bindle-name}` can be pathy. For example, `/_i/example.com/mybindle/1.2.3` is a valid path to a bindle named `example.com/mybindle/1.2.3`.
    - `GET`: Get a bindle by name.
    - `HEAD`: Send just the headers of a GET request
    - `DELETE`: Yank a bindle. This will set the `yank` field on a bindle to `true`. This is the only mutation allowed on a Bindle.
- `/_i`
    - `POST`: Create a new bindle. If all of the parcels specified in the bindle exist, a 201 status will be returned. If 1 or more of the parcels are missing, a 202 status will be returned with a reference to the missing parcels
- `/_i/{bindle-name}@{parcel-id}`: The path to a Bindle name and parcel ID, where `{parcel-id}` is an exact SHA of a parcel and `{bindle-name}` follows the same rules as outlined above. Parcels can only be accessed if the client has the proper permissions to access the given bindle and, as such, cannot be accessed directly
    - `GET`: Directly fetch a parcel's opaque data.
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a parcel if it does not already exist. This may be disallowed. The data included in the body must have the same SHA as indicated by the `{parcel-id}` and must exist within the invoice
- `/_q`: The query endpoint
- `/_r`: The relationships endpoint. This endpoint allows for querying of various relationships between parts of a bindle.
    - `/_r/missing/{bindle-name}`: An endpoint for retrieving missing parcels in a bindle. `{bindle-name}` follows the same aforementioned rules around bindle naming
        - `GET`: Returns a list of label objects for missing parcels (i.e. parcels that haven't been uploaded). Yanked bindles are not supported by this endpoint as parcels for yanked bindles should not be uploaded
- `/login`: Triggers a login flow for the API
  - `GET`: Redirects to the login provider to start an OIDC device login flow. It will trigger a Device Authorization Flow as defined in [RFC8628](https://datatracker.ietf.org/doc/html/rfc8628). The response will be a standard response as defined in [Section 3.2]( https://datatracker.ietf.org/doc/html/rfc8628#section-3.2) with 2 additional parameters: `client_id` will contain the client ID of the OIDC provider, and `token_url` will contain the OAuth2 token authorization endpoint for use in obtaining tokens. This endpoint supports the following query parameters:
    - `provider` (required): The name of the provider to use: For example: `provider=github`.
- `/bindle-keys`: An OPTIONAL implementation of the [keyring protocol specification](./keyring-protocol-spec.md). The reference implementation only exposes public keys with the `host` role, but other implementations MAY support all types of keys

While bindle names MAY be hierarchical, neither the `_i` nor the `_p` endpoints support listing the contents of a URI. This constraint is for both scalability and security reasons. To list available bindles, agents MUST use the `_q` endpoint if implemented. In absence of the `_q` endpoint, this specification does not support any way to list available bindles. However, implementations MAY support alternative endpoints, provided that the URI for those endpoints does not begin with the `_` character.

## Bindle object

Normally, a Bindle object consists of an `invoice` string (with the exact contents signed by the bindle creator) and one or more `signature` blocks:

```toml
invoice = """
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
author = ["Bindle Author <author@example.com>]
...
"""

[[signature]]
# Untrusted label: Bindle Author <author@example.com>
info = """
key = "1c44..."
role = "creator"
at = 1611960337
"""
signature = "ddd237895ac..."
```

A _yanked_ Bindle object additionally has the top-level entry `yanked = true` and one or more `yanked_signature` blocks:

```toml
yanked = true
invoice = """
bindleVersion = "1.0.0"

[bindle]
name = "mybindle"
author = ["Bindle Author <author@example.com>]
...
"""

# ... other signatures ...

[[yanked_signature]]
# Untrusted label: Bindle Host [https://bindle.example.com]
info = """
key = "107a..."
role = "host"
at = 1613960337
"""
signature = "bbb237895ac..."
```

## Missing parcels

When creating a new invoice, a response body will be returned containing two keys: `invoice` and `missing`. The `invoice` will always contain the newly created invoice object. The `missing` key will have a list of missing parcels set if a 202 status code is returned and will be empty otherwise. An example response body is below:

```toml
[invoice]
bindleVersion = "1.0.0"

[invoice.bindle]
name = "enterprise.com/cargobay"
version = "1.0.0"
description = "The cargo bay manifest"
authors = ["Miles O'Brien <chief@ufp.com>"]

[[invoice.parcel]]
[invoice.parcel.label]
sha256 = "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5"
mediaType = "text/plain"
name = "isolinear_chip.txt"
size = 9

[[invoice.parcel]]
[invoice.parcel.label]
sha256 = "51534027079925942fdea13d4d088c7126f3e456364525b67d6ca0858d6587bc"
mediaType = "text/plain"
name = "barrel.txt"
size = 15

[[invoice.parcel]]
[invoice.parcel.label]
sha256 = "a6e41416c2bee47e9b97900ba57de696cccc1920e331f5a0d490726a7938d8c6"
mediaType = "text/plain"
name = "crate.txt"
size = 14

[[missing]]
sha256 = "51534027079925942fdea13d4d088c7126f3e456364525b67d6ca0858d6587bc"
mediaType = "text/plain"
name = "barrel.txt"
size = 15

[[missing]]
sha256 = "a6e41416c2bee47e9b97900ba57de696cccc1920e331f5a0d490726a7938d8c6"
mediaType = "text/plain"
name = "crate.txt"
size = 14
```

## Errors
Any errors should reply with the proper HTTP status code for the problem and a TOML body containing a single `error` key with a string value containing additional information like so:

```toml
error = "resource already exists"
```

## Yanked Bindles

A bindle that is marked `yanked = true` MUST be treated according to the following rules:

- It MUST NOT be served in a `_q` query
- It MUST NOT be accepted by a `POST` operation
- The `DELETE` operation is a no-up on a yanked Bindle
- A `GET` request should only be fulfilled if the `yanked=true` query parameter is set. In any other case, it should mark it as "access denied"
- The query endpoint MUST NOT return yanked bindles unless the `yanked=true` parameter is set. If that optional parameter is not provided by the implementation, the implementation MUST NOT return yanked bindles in a query.

Parcels cannot be yanked.

## Deleting Bindles

No support is provided for deleting Bindles.

## The Query Endpoint (`/_q`)

The query endpoint is a generic listing and filtering API.

The query endpoint _only_ returns `invoice` objects.
Parcels are not to be analyzed via the query endpoint.

The query endpoint uses URL query string parameters for passing parameters from the client to the server.
The following query parameters are defined by this specification:

- `q`: (OPTIONAL) A string that, if present, MUST be applied to search results according to the description below. The whitespace character (` `) separates query strings
- `o`: (OPTIONAL) The offset marker as an unsigned 64-bit integer. This is used for paging results
- `l`: (OPTIONAL) The upper limit of results that may be returned on a query page as an unsigned 8-bit integer
- `strict`: (OPTIONAL) A boolean flag (`true`|`false`) indicating whether the strict matching mode must be applied
- `v`: (OPTIONAL) SemVer constraint match operator
- `yanked`: (OPTIONAL) A boolean flag (`true`|`false`) indicating whether yanked bindles should be returned. By default, this is `false`, meaning yanked bindles are never returned.

### Processing queries and determining matches

This section describes two modes for querying. An implementation of Bindle MUST implement `strict` mode. An implementation MAY implement standard mode. If an implementation does not implement standard mode, non-strict queries MUST return the same results returned in strict queries. In other words, if standard mode is not supported, strict results must be returned regardless of the value of the `strict` query parameter.

If a service supports both strict and standard modes, then strict mode SHOULD only be applied when the `strict` parameter is set to `true`. In all other cases, the standard mode SHOULD be applied.

Whether strict or standard, a query MUST NOT match a yanked bindle unless the `yanked` parameter is set to `true`.

Whether strict or standard, a query MAY choose to treat an empty query string as a universal match, matching all non-yanked bindles.

Whether strict or standard, if a Bindle server supports authorization controls, the query engine SHOULD omit results that the agent is not authorized to see.

#### Strict Mode

In strict mode, every term in the `q` string MUST be found in the `name` field of the bindle. No "fuzzy matching" may be applied in strict mode.

For example, if the query is `q=foo/bar/baz`, then it MUST be true that...

- An invoice named `foo/bar/baz` matches
- An invoice named `hello/foo/bar/baz/goodbye` matches
- An invoice named `foo/hello/bar/baz` does not match
- An invoice named `hello` and with the description `foo/bar/baz` does not match
- An invoice named `foo-bar-baz` does not match

If the query string has multiple components, all components must be present in the name. For example, if the query string is `q=foo bar baz`, then it MUST be true that...

- An invoice named `foo/bar/baz` matches
- An invoice named `hello/foo/bar/baz/goodbye` matches
- An invoice named `foo/hello/bar/baz` matches
- An invoice named `foo-bar-baz` matches
- An invoice named `hello` and with the description `foo/bar/baz` does not match

Additionally, if a `v` SemVer range modified is present, the query engine MUST exclude any results that do not match the range modifier.

#### Standard Mode

In standard query mode, the search terms SHOULD all _match_ in the list of `bindle` fields. Here, _match_ allows for fuzzy matching algorithms. The purpose of this statement, though, is to indicate that queries are considered an AND-ed list of required terms, not an OR-ed list of disjunctive terms.

The following fields MUST be included in the standard search index:

- `name`

The following fields SHOULD be included in the standard search index:

- `version`
- `authors`
- `description`

Implementations MAY choose to weight some fields higher than others. This specification suggests that if weighted rankers are employed, `name` SHOULD be the highest weighted field.

Annotations SHOULD NOT be included in search indices because the data stored in these fields is arbitrary, and thus are potentially used for information not intended for general consumption.

Parcel information MUST NOT be included in search indices. Inclusion of such information introduces security concerns.

In strict mode "fuzzy matching" (e.g. soundex or similar) MAY be applied to some or all query terms.

Additionally, if a `v` SemVer range modified is present, the query engine MUST exclude any results that do not match the range modifier.

#### The SemVer Range Modifier

The `v` query parameter, if supplied, MUST contain a valid SemVer range modifier.

For example, the range modifier `v=1.0.0-beta.1` indicates that a version MUST match version `1.0.0-beta.1`. Version `1.0.0-beta.12` does NOT match this modifier. 

The range modifiers will include the following modifiers, all based on [the Node.js _de facto_ behaviors](https://www.npmjs.com/package/semver):

- `<`, `>`, `<=`, `>=`, `=` -- all approximately their mathematical equivalents
- `-` (`1.2.3 - 1.5.6`) -- range declaration
- `^` -- patch/minor updates allow (`^1.2.3` would accept `1.2.4` and `1.3.0`)
- `~` -- at least the given version

An example Rust implementation of the above is the [`semver` crate](https://crates.io/crates/semver)

## Returning Matches

When a query is executed without error, the following structure MUST be used for responses. In this specification, the format is TOML. However, if the `ACCEPT` header indicates otherwise, implementations MAY select different encoding formats.

```toml
query = "mybindle"
strict = true
offset = 0
limit = 50
total = 1
more = false
yanked = false

[[invoices]]
bindleVersion = "1.0.0"

[invoices.bindle]
name = "mybindle"
description = "My first bindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]

[invoices.annotations]
myname = "myvalue"

[[invoices.parcels]]
[invoices.parcels.label]
sha256 = "e1706ab0a39ac88094b6d54a3f5cdba41fe5a901"
mediaType = "text/html"
name = "myparcel.html"

[[invoices.parcels]]
[invoices.parcels.label]
sha256 = "098fa798779ac88094b6d54a3f5cdba41fe5a901"
mediaType = "text/css"
name = "style.css"

[[invoices.parcels]]
[invoices.parcels.label]
sha256 = "5b992e90b71d5fadab3cd3777230ef370df75f5b"
mediaType = "application/x-javascript"
name = "foo.js"
size = 248098

[[invoices]]
bindleVersion = "1.0.0"

[invoices.bindle]
name = "example.com/mybindle"
version = "0.1.0"

[[invoices.parcels]]
[invoices.parcels.label]
sha256 = "5b992e90b71d5fadab3cd3777230ef370df75f5b"
mediaType = "application/x-javascript"
name = "first"

[[invoices.parcels]]
[invoices.parcels.label]
omitted...

[[invoices.parcels]]
[invoices.parcels.label]
omitted...

[[invoices.parcels]]
[invoices.parcels.label]
omitted...
```

The top-level search fields are:

- `query`: (REQUIRED) The query string, as parsed by the search engine
- `strict`: (REQUIRED) An indication of whether the query engine processed the query in strict mode
- `offset`: (REQUIRED) The offset (as an unsigned 64-bit integer) for the first record in the returned results
- `limit`: (REQUIRED) The maximum number of results that this query would return on this page
- `timestamp`: (REQUIRED) The UNIX timestamp (as a 64-bit integer) at which the query was processed
- `yanked`: (REQUIRED) A boolean flag indicating whether the list of invoices includes potentially yanked invoices 
- `total`: (OPTIONAL) The total number of matches found. If this is set to 0, it means no matches were found. If it is unset, it MAY be interpreted that the match count was not tallied.
- `more`: (OPTIONAL) A boolean flag indicating whether more matches are available on the server at the time indicated by `timestamp`.

The attached list of invoices MUST contain the `[bindle]` fields of the `invoice` object. Results MAY also contain `[annotations]` data (in a separate annotations section). Results MAY contain `[[parcel]]` definitions.

See the [Invoice Specification](invoice-spec.md) for a description of the `[bindle]` fields.

### Ordering of Results

The specification does not provide any guidance on ordering of search results. It is a desirable, but not required, property of search results that under the same circumstances, two identical queries return identical results, including identical ordering.