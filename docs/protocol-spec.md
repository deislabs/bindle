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
- `/_q`: Reserved for future use (as support for a search engine)

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
