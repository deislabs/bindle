# The Bindle Protocol

Bindle uses HTTP/3 with TLS as a transport protocol. HTTP/3 has many advantages, and is
what makes Bindle as robust and speedy as it is.

HTTP Endpoints:
- `/_i/{bindle-name}`: The path to a bindle's invoice. Note that {bindle-name} can be pathy. For example, `/_i/example.com/mybindle/v1.2.3` is a valid path to a bindle named `example.com/mybindle/v1.2.3`.
    - `GET`: Get a bindle by name
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a new bindle, optionally also sending some or all of the parcels.
    - `DELETE`: Yank a bindle. This will set the `yank` field on a bindle to `true`. This is the only mutation allowed on a Bindle.
- `/_b/{parcel-id}`: The path to a parcel ID, where `{parcel-id}` is an exact SHA to a parcel.
    - `GET`: Directly fetch a parcel
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a parcel if it does not already exist. This may be disallowed.
- `/_q`: The query endpoint

## Yanked Bindles

A bindle that is marked `yanked = true` MUST be treated according to the following rules:

- It MUST NOT be served in a `_q` query
- It MUST NOT be accepted by a `POST` operation
- The `DELETE` operation is a no-up on a yanked Bindle
- A `GET` request should only be fulfilled if the `yanked=true` query parameter is set. In any other case, it should mark it as "access denied" (TODO: what is the actual HTTP code)
    - If `yanked=true` in the query string, the server SHOULD serve the bindle unaltered, including the `invoice.toml`'s `yanked = true` attribute.

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

### Processing queries and determining matches

This section describes two modes for querying. An implementation of Bindle MUST implement `strict` mode. An implementation MAY implement standard mode. If an implementation does not implement standard mode, non-strict queries MUST return the same results returned in strict queries. In other words, if standard mode is not supported, strict results must be returned regardless of the value of the `strict` query parameter.

If a service supports both strict and standard modes, then strict mode SHOULD only be applied when the `strict` parameter is set to `true`. In all other cases, the standard mode SHOULD be applied.

Whether strict or standard, a query MUST NOT match a yanked bindle.

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

## Returning Matches

When a query is executed without error, the following structure MUST be used for responses. In this specification, the format is TOML. However, if the `ACCEPT` header indicates otherwise, implementations MAY select different encoding formats.

```
query = "foo/bar/baz"
strict = true
offset = 0
limit = 20
timestamp = 1234567890
total = 2
more = false

[[bindle]]
name = "foo/bar/baz"
version = "v0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "My first bindle"

[[bindle]]
name = "hello/foo/bar/baz/goodbye"
version = "v8.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Another bindle example"
```

The top-level search fields are:

- `query`
- `strict`
- `offset`
- `limit`
- `timestamp`
- `total`
- `more`

The attached list of bindles MUST contain the `[bindle]` fields of the `invoice` object. Results MAY also contain `[annotations]` data (in a separate annotations section) if the query engine allows