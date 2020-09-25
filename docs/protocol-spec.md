# The Bindle Protocol

Bindle uses HTTP/3 with TLS as a transport protocol. HTTP/3 has many advantages, and is
what makes Bindle as robust and speedy as it is.

HTTP Endpoints:
- `/_i/{bindle-name}`: The path to a bindle's invoice. Note that {bindle-name} can be pathy. For example, `/_i/example.com/mybindle/v1.2.3` is a valid path to a bindle named `example.com/mybindle/v1.2.3`.
    - `GET`: Get a bindle by name
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a new bindle, optionally also sending some or all of the boxes.
    - `DELETE`: Remove a bindle. This may mark a box for cleanup (or it may not)
- `/_b/{box-id}`: The path to a box ID, where `{box-id}` is an exact SHA to a box.
    - `GET`: Directly fetch a box
    - `HEAD`: Send just the headers of a GET request
    - `POST`: Create a box if it does not already exist. This may be disallowed.
- `/_q`: Reserved for future use (as support for a search engine)