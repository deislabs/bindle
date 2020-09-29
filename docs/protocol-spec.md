# The Bindle Protocol

Bindle uses HTTP/3 with TLS as a transport protocol. HTTP/3 has many advantages, and is
what makes Bindle as robust and speedy as it is.

HTTP Endpoints:
- `/_i/{bindle-name}`: The path to a bindle's invoice. Note that {bindle-name} can be pathy. For example, `/_i/example.com/mybindle/v1.2.3` is a valid path to a bindle named `example.com/mybindle/v1.2.3`.
    - `GET`: Get a bindle by name
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a new bindle, optionally also sending some or all of the parcels.
    - `DELETE`: Remove a bindle. This may mark a parcel for cleanup (or it may not)
- `/_b/{parcel-id}`: The path to a parcel ID, where `{parcel-id}` is an exact SHA to a parcel.
    - `GET`: Directly fetch a parcel
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a parcel if it does not already exist. This may be disallowed.
- `/_q`: Reserved for future use (as support for a search engine)