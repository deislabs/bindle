# Label Specification

A _label_ contains metadata about a parcel. It is in TOML format and exists only in the `parcel` array of an invoice

```toml
sha256 = 5b992e90b71d5fadab3cd3777230ef370df75f5b...
mediaType = "application/x-javascript"
name = "foo.js"
size = 248098

[annotations]
key1 = "value 1"

[feature.wasm]
# These fields are illustrative only. They don't have any meaning to Bindle.
runtime = "wasmtime-1.2.3"
dependencies = ["foo-1.0.0", "bar-2.3.4"]
```

## Top Level Fields

The top-level fields describe the `parcel.dat` content of this parcel.

- `sha256` is the SHA2-256 hash of the `parcel.dat` data (REQUIRED)
- `mediaType` is the media type (MIME type) of the parcel's data (REQUIRED)
- `name` is a recommended filename for the parcel data (OPTIONAL)
- `size` is the size in bytes (unsigned integer) of the parcel data (REQUIRED)
- `origin` indicates the name and version of the upstream invoice (if any) originally referred to this parcel (OPTIONAL)
- `sha512` is the SHA2-512 hash of the parcel data (Not yet supported)

## The `annotations` Section

Annotations are name (string) value (string) pairs that can be attached to a label.
Annotations are defined as non-functional metadata about a parcel.
While features are used to turn on and off parts of a bindle,
annotations are used to express information about this parcel and its content.

They look like this:

```toml
[[annotations]]
homepage = "https://example.com"
count = "80"
"example.com/namespaced/key" = "some value"
```

### Reserved Annotations

The following annotations are reserved, and are described here:

- `bindle.dev/readme`: Accepts values `true` or `false`. Indicates that this parcel is a README file. The `mediaType` property should be consulted to determine the format. Easy to read text formats such as `text/plain` and `text/markdown` are recommended.
    - Multiple parcels may have `"bindle.dev/readme" = "true"`. Bindle does not define how an agent should handle this case. However, one methodology may be to choose based on `mediaType`.
- `bindle.dev/license`: Accepted values are `OTHER` and any of the identifiers defined in the [SPDX license list](https://spdx.org/licenses/). This indicates that the parcel _is_ a license document. The `mediaType` should be consulted to determine format. Plain text with the `text/plain` media type is encouraged.
    - If an SPDX identifier is used, the license text MUST be of the license indicated by the SPDX identifier.
    - Multiple parcels may be identified as containing licenses. Bindle does not define how licenses are to apply to the bindle contents.
    

## The `feature` Section

The `feature` section provides a location for storing additional details about the parcel.
Typically, it describes a particular set of features that this parcel has support for.

The individual elements in the `feature` section are not dictated by the specification.
Implementations are free to use this section to facilitate runtime-specific metadata.

```
[feature.SECTION_NAME]
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

The purpose of dividing features into sections is to enforce a top-level grouping mechanism to divide similar properties. By convention, `SECTION_NAME` MAY be either the name of the specific runtime or of a group of related configuration parameters.

For example, consider a two-section feature set:

```toml
[label]
# ...
[feature.wasm]
stack_size = 2048
wasi = false
[feature.gpu]
required_cores = 4
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

#### Filtering Features

This section describes how clients can filter based upon features.

A feature is a special annotation on a parcel. For example, in this excerpt from an `invoice.toml`, this parcel declares a single feature:

```toml
[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
type = "library"
```

While group membership is attached to the parcel declaration, a feature is attached to the parcel's label. What this means is that features are durable across invoice usages.

When evaluating an invoice, a client implementation MAY choose to provide filtering on features. That is, a client may choose parcels based on which features are present/absent, or on what the value of a feature is. This section describes normative behaviors of filtering that must be applied so that parcels are evaluated without contradiction by runtimes.

**A feature name is unique to a section.** No section can have two features with the same name. The following is therefore illegal:

```toml
[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
type = "library"
type = "executable"
```

It is illegal because `wasm.type` is specified twice inside the same section.

**Names are not global across sections.** If two sections have features of the same name, an implementation SHOULD NOT assume that they refer to the same feature. For example, the two `type` declarations in this example are not equivalent.

```toml
[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
type = "library"
[parcel.label.feature.architecture]
type = "x86"
```

Consequently, implementations SHOULD NOT provide support for filtering based on name without consideration for section.

**Values of features are considered mutually exclusive.** That is, filters should not allow two effective rules that specify the same group/name, but different values. Allowing this would potentially allow conflicting features to be simultaneously loaded by a client.

For example, when supporting a set of parcels that act as an aggregate application, where some parcels are libraries and some are executables, this structure is NOT recommended:

```toml
bindleVersion = "1.0.0"

[bindle]
name = "example/weather"
version = "0.1.0"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256
[parcel.label.feature.wasm]
type = "executable"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
type = "library"
```

This structure labels one parcel of type "executable" and another of type "library". But it is not considered legal to run a filter that says `wasm.type = library or wasm.type = executable`.

One alternative would be to not label the executable with a `type`. A better solution would be to specify these as two separate options:

```toml
bindleVersion = "1.0.0"

[bindle]
name = "example/weather"
version = "0.1.0"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256
[parcel.label.feature.wasm]
executable = "true"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
```

In this case, the filter `wasm.library = "true" or wasm.executable = "true"` is a legal feature selector because it selects on two different _names_, not two different _values_.

> NOTE: This particular rule may be reconsidered in the future. At present, we are primarily interested in systematically avoiding conflicting feature loading.

