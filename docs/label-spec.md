# Label Specification

A _label_ (`label.toml`) contains metadata about a box. It is in TOML format, and is considered immutable once stored.

```toml
sha256 = 5b992e90b71d5fadab3cd3777230ef370df75f5b...
mediaType = "application/x-javascript"
name = "foo.js"
size = 248098

[annotations]
key1 = "value 1"

[feature.wasm]
# These fields are illustrative only
runtime = "wasmtime-v1.2.3"
dependencies = [foo-v1.0.0, bar-v2.3.4]
```

## Top Level Fields

The top-level fields describe the `parcel.dat` content of this parcel.

- `sha256` is the SHA2-256 hash of the `parcel.dat` data (REQUIRED)
- `mediaType` is the media type (MIME type) of the parcel's data (REQUIRED)
- `name` is a recommended filename for the parcel data (OPTIONAL)
- `size` is the size in bytes (unsigned integer) of the parcel data (REQUIRED)
- `sha512` is the SHA2-512 hash of the parcel data (Not yet supported)

## The `feature` Section

The `feature` section provides a location for storing additional details about the parcel.
Typically, it describes a particular set of features that this parcel has support for.

The individual elements in the `feature` section are not dictated by the specification.
Implementations are free to use this section to facilitate runtime-specific metadata.

```
[feature.SECION_NAME]
PROPERTY_NAME = "PROPERTY VALUE"
PROPERTY_NAME_2 = "PROPERTY VALUE 2"
```

- `SECTION_NAME` (string) describes a group of related properties
- `PROPERTY_NAME` (string) names a particular property
- `PROPERTY VALUE` (string) provides a value for a named property. The value SHOULD be limited to 2048 characters

The format of the `feature` section is best described as a hashtable whose keys are strings, and whose values are hashtables of string keys and values.
In pseudocode, this can be represented as:

```
feature = hashtable<string, hashtable<string, string>>
```

Implementations MUST NOT assume that sections or features are ordered. In other words, the following sections should be treated as identical:

```toml
[label]
# ...
[feature.a]
setting = "one"
setting = "two"
[feature.b]
setting = "three"
```

and 

```toml
[label]
# ...
[feature.b]
setting = "three"
[feature.a]
setting = "two"
setting = "one"
```

The purpose of dividing features into sections is to enforce a top-level grouping mechanism to divide similar properties up. By convention, `SECTION_NAME` MAY be either the name of the specific runtime or of a group of related configuration parameters.

For example, consider a two-section feature set:

```toml
[label]
# ...
[feature.wasm]
stack_size: 2048
wasi: false
[feature.gpu]
required_cores: 4
```

The first section defines features for a WebAssembly (Wasm) runtime.
The second set describes the more general GPU needs of this parcel.

The underscore character (`_`) SHOULD be used as the delimiter for long setting names and property names.
The characters `-` and `.` SHOULD NOT be used.
Implementations MAY produce an error if `-` or `.` are present in names.
While TOML is case-sensitive, it is not recommended that implementors use camel case for names.
In the example above, the stack size was represented as `stack_size`, not `stack-size`, `stack.size` or `stackSize`.

### Evaluating Feature Declarations

Implementations of Bindle SHOULD follow the guidelines in this section, as they stipulate a uniform way to evaluate feature sets.

When a feature section is present, the runtime SHOULD determine that this is an indication that properties ascribed to that section are to be processed.
The absence of a feature section SHOULD be treated as an indication that those features do not apply to the parcel.

The previous example had a section called `[feature.gpu]`.
The presence of this section indicates that this parcel has been configured with `gpu` settings in mind.
The absence of that section SHOULD be interpreted to mean that the parcel has no requirements pertinent to the `gpu` settings.
In other words, the presence of a section indicates its _participation in_ a particular set of settings, while absence of a section indicates _non-participation_ in such settings.

Within feature sections, the feature property name means something similar. Consider the following three scenarios:

```toml
# Example 1
[label]
# ...
[feature.frobnitz]
ui_framework = "v1"
other_setting = "one"

# Example 2
[label]
# ...
[feature.frobnitz]
ui_framwork = "v2"
other_setting = "one"

# Example 3
[label]
# ...
[feature.frobnitz]
other_setting = "one"
```

Example 1 indicates that this parcel participates in the `ui_framework` property configuration, and that the value of that property is `v1`.

Example 2 indicates that this parcel participates in the `ui_framework` property configuration, and that the value of that property is `v2`.

But Example 3 indicates that it does not participate in `has_sprocket` at all.
An implementation would therefore be correct in interpreting example 3 as "this parcel has no way to apply a concept of a `ui_framework`."

This distinction is important when filtering a list of parcels based on features.
To arrive at a list of parcels that have a `ui_framework` not equal to `v2`, the implementation should return only the first parcel above.
The last parcel does not have any `ui_framework`.