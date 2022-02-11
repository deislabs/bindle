# Keyring Protocol Specification

This specification describes an HTTPS protocol for fetching public key entries for use in Bindle
keyrings. It is _not required_ to distribute keys in this manner, but it does allow for a convenient
way to distribute well known keys.

## Path and Protocols

Implementations MUST use HTTP secured with TLS (i.e. HTTPS) in order to guarantee that the received
data has not been tampered with. Implementations are not required to only host this protocol as part
of a single server (e.g. they can serve other endpoints besides the keys).

It is RECOMMENDED that implementations serve this protocol on the `/bindle-keys` path, mounted onto
any path prefixes (e.g. `/api/v1/bindle-keys`).

## Verbs and Parameters

Implementations MUST support the `GET` HTTP verb and MAY support `HEAD`. All other verbs MUST NOT be
used.

Implementations MUST support the following query string parameters. Other parameters MAY be added by
implementations, but they are considered non standard. If no query string parameters are specified,
implementations SHOULD return all available keys.

- `roles`: A comma delimited list of roles to filter on. All keys with these roles will be returned.
  Example: `roles=creator,approver`. Allowed roles can be found in the [signing
  spec](./signing-spec.md#signing-and-roles)

## Response Data

Implementations MUST return a keyring response in TOML as specified in the [signing
spec](./signing-spec.md#keyrings). Implementations MAY also support other serialization formats such
as JSON. Different formats MUST be requested with the HTTP `Accept` header. If no `Accept` header is
specified, implementations MUST return the response as TOML

## Authentication and Authorization

Implementations MAY require authentication and authorization

## Exclusions

This specification explicitly excludes any guidelines on how keys may be uploaded to a server as
there could be many different methods of uploading keys depending on the circumstances
