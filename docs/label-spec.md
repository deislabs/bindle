# Label Specification

A _label_ (`label.toml`) contains metadata about a box. It is in TOML format, and
is considered immutable once stored.

```toml
[box]
mediaType = "application/x-javascript"
name = "foo.js"
size = 248098
```

## `box` Fields

- `mediaType` is the media type (MIME type) of the box's data
- `name` is a recommended filename for the boxed data
- `size` is the size in bytes of the boxed data
- `sha512` is the SHA2-512 hash of the boxed data
- `sha256` is the SHA2-256 hash of the boxed data