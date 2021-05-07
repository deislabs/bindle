# Describing Aggregate Applications in Bindle

> `$ Wake up, Neo...`

This document describes the mechanics of fetching and loading WebAssembly aggregate applications.

## Terminology

In addition to the Bindle terminology (invoice, parcel, bindle, label), the following terms are used in this document:

- Aggregate Application (AA): An application that functions as a single program, though it may have pieces running in different runtimes.
- Module: The compiled form of a WebAssembly binary. In this design, modules are stored as parcels.
- `sg1` (StarGazer One): A hypothetical CLI that executes aggregate applications without a graphical user interface
- `sgu` (StarGazer UI): Obviously superior to sg1, this executes aggregate applications with bindings for a graphical user interface.
- Stargazer: The name of the overarching project for running WebAssembly modules "in the cloud."
- Tianyan: The specific part of the Stargazer platform that executes modules in a distributed (multi-host) environment.
- Runtime (Or Stargazer Runtime): A tool that executes WebAssembly aggregate applications. This tool includes a Bindle client as well as the WebAssembly executor.

## Overview

This document explains how we can combine nanoprocesses, Tianyan beaming, and bindles as ways to represent, store, and deploy _aggregate applications._

An aggregate application is an set of programs (modules) that together behave as a cohesive individual program, though they are comprised of separate binaries that each runs in its own runtime.
Sometimes these runtimes may execute on the same host, and other times the runtimes may be spread across multiple hosts.

When it comes to Bindle storage, the basic idea is that an aggregated application is stored as a single bindle with multiple parcels.
The parcels contain the WebAssembly modules that compose that application.
Parcels may also store additional data used by these aggregated applications.
Not all of the parcels on a bindle are required.
For example, a single bindle may represent multiple different configurations for running the same app.
This document explains how groups and features are used to choose which modules will be used in a specific instance of an application.

The bindle describes not just all the necessary parts of an aggregated application, but all the possible parts.
So an application may require one parcel, and have another optional parcel.
Or it may require one of several parcels.
This configuration could (in theory) get even more complicated, where if one particular optional parcel is selected, than another parcel of a set of parcels must also be selected, and so on.
The examples later in this document illustrate such cases.

Regardless of the complexity of the selection, the end result is that the runtime selects the set of parcels that it needs to successfully run the application in its given context.

This document explains how a particular runtime should perform in order to use bindles for an aggregate application.
For its part, the runtime performs the following functions:

- It accepts a request to execute an application
- It fetches the application description from a bindle server. The description is stored in the form of a bindle invoice.
- Upon examining the invoice, it determines which set of parcels it needs in order to execute the aggregate application
    - This may involve input by a user or other external agent (e.g. CI)
- Importantly, it also determines _where_ these parcels will be run. It need not be the case that all parcels run on the same host.
- Once such decisions are made, the runtime executes the aggregate application
- During execution, the runtime is responsible for delegating user interactions. (This may mean running a UI, or may mean determining what does run the UI)
- Finally, when the aggregate application hits its stopping condition (program completes, user exits, fatal error, etc), it is the runtime's job to clean up

This document focuses on how the runtime makes decisions about how to fetch and load the constituent parts of an aggregated application.
That is, this document explains how an application is described as a bindle, and how a Stargazer runtime should interpret the information in that bindle.

## Stargazer Application Structure: Wasm, Tianyan, Nanoprocesses, and Bindle

We use the term "Stargazer Application" to describe aggregate Wasm applications written according to a specified model.
While this document does not describe the model in detail, it outlines the main features.

This discussion is oriented around WebAssembly applications that can execute in environments in and beyond the browser.
Edge, datacenter, Kubernetes, Nomad, and CLI runtimes are all considered "in bounds."

*Tianyan:* Tianyan is a runtime that selects and executes modules in an aggregate application.
It can distribute the execution of these modules over a variety of runtimes, and orchestrate their communication.
For example, it may take a three-part application, Tianyan may choose to execute two modules locally, and the third in the cloud.

*Beaming:* This term refers to the Tianyan process of sending a module or module reference to another trusted Tianyan runtime to execute the module on its behalf.

*Nanoprocesses:* We used to refer to this as "tianyan" as well, but have since adopted the language used by BCA.
We believe our usage of the term is the same as BCAs: A nanoprocess is a Wasm module that can execute on its
own, but communicate to other Wasm modules via the component architecture (Module Linking, Interface Types) and WASI (IO Streams, IO Arrays).

*Bindle:* In this context, we use Bindle as a _specific part_ of the overall Stargazer architecture.
To that end, we do not talk about any potential general applications of Bindle,
just the specific application of Bindle to Stargazer.

The Stargazer model says, essentially, that Stargazer Applications are organized into Nanoprocesses.
The developer tooling captures possible configurations of a Stargazer Application into a bindle,
where each modules is represented as a parcel.
A Tianyan-capable runtime reads the bindle invoice, constructs an appropriate representation of the application,
and orchestrates its execution as one or more nanoprocesses running on one or more trusted hosts.

In this document, we will walk through multiple examples of how Stargazer Applications are described, structured,constructed, read, and executed.

> A core principle of Bindle is that all members of a package must be referenced as parcels.
> In this model, there is no "runtime dependency resolution" in the sense where a nondeterministic process must resolve an identifier to an external package.
> Parcels are referenced by SHA and signed cryptographically.
> Only the parcels attached to an invoice may be used to construct the application.
> Groups and features may be used to swap implementations or toggle features.
> But this resolution process is intrinsic: Only parcels on the invoice are used in constructing the application.
> For this reason, an application never includes parts that the invoice signer did not explicitly attest.

The following example describe a Stargazer Application bindle. We will begin with this base application and show increasingly complicated models of applications.

## Example 1: A one-piece aggregate application

There is no requirement that an aggregate application has more than one WebAssembly module.
Given this, we can start with a simple example.

In this example, a single module runs as a simple program that prints the plain text output "Hello World"

Here is an example Bindle invoice:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/hello-world"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Greeter"

[[parcel]]
label.sha256 = "3287d35386474cb048264cef43e4fead1701e48f"
label.mediaType = "application/wasm"
label.name = "hello.wasm"
label.size = 1710256
```

A Bindle invoice contains a few distinct sections of data:

- The `bindle` section describes the bindle
- The `parcel`s contain the labels for each parcel

To use our present parlance, each invoice describes an aggregate application, with each part of that application stored as a parcel.

In the example above, the application is named `example/hello-world`. It contains only one parcel, a WebAssembly module named `hello.wasm` and identified by the given SHA.

> Note: SHAs and sizes in this document are for illustrative purposes only. Most are fictional, and have been formatted for this document.

Assume we have a client called `sg1` that can execute a simple command line program. And assume we have a Bindle server running at `example.com`. We might execute the above program like this:

```console
$ sg1 example.com/example/hello-world/0.1.0
Hello World
```

In the example above, here's how SG1 executed the program:

1. Fetch the Bindle invoice from example.com/example/hello-world
2. Find which parcels need to be loaded
    - In this case, there is only one. By default, it is required (as are all parcels in the default global group).
    - In this case, the media type is enough to tell the runtime whether or not it can execute the given parcel
3. Fetch the parcel
4. Start the runtime and load the parcel
5. Run the program to completion
6. Clean up the parcel
    - In this case, this may only entail shutting down the runtime

This example is the simplest case for an aggregate. In a moment, we will start to look at more advanced cases. But before that, here is a brief example of an error case.

### Example 1.a: A library and its "interface"

While the other examples in this document focus on runnable applications, it is also desirable that Bindle can store libraries.
Developers may then select these libraries from a Bindle server during the development process, and include them in their applications.

For example, while building a weather application, the developer may look for an available "weather almanac" module.
This module may publish both a binary and an interface specification.
Therefore, a bindle would need a parcel for the module, and a parcel for the interface definition.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/libalmanac"
version = "5.1.13"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "A weather predicting library based on historical data"

[[parcel]]
label.sha256 = "3287d35386474cb048264cef43e4fead1701e48f"
label.mediaType = "application/wasm"
label.name = "hello.wasm"
label.size = 1710256

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710

[[parcel]]
[parcel.label]
sha256 = "cef43e4fead1701e48f3287d35386474cb048264"
mediaType = "application/witx+wat"
name = "libalmanac.witx"
size = 2561710
```

In the example above there are two parcels:

- `libalmanac.wasm` is a WebAssembly module, as indicated by its `mediaType`
- `libalmanac.witx` is a WITX file (`mediaType: application/witx+wat`)

A user agent that wanted to know what definitions were exported by this module could retrieve the WITX file parcel alone.
But an agent that wanted to use the module could import both.

While WITX files could be included in any of the examples that follow, we will omit them for brevity.
It is a safe assumption that any Wasm file could have an accompanying WITX file.

> Note that most packages would also include license documents and other auxiliary documents required for legal reasons or usability reasons. These can be modeled as parcels, and are omitted from this document for brevity.

#### Libraries vs. Applications In Practice

This example illustrates an early stage of how Stargazer applications are built.
A library, such as the `libalmanac` library, may be served as a stand-alone bindle.
However, the intention in doing this is to make it available for developers, not to make it available to end users.

Applications, in contrast, will contain a complete manifest of all of the Wasm modules required to run the application.
See Example 3 for an illustration of how the `libalmanac` library is included.
Because Bindle uses the SHA256 (and other metadata, including signature) for identification, any host that has already pulled the `libalmanac` library will not need to re-pull it.

A library may have dependencies on other libraries, in which case those are presented in the parcel list (See example 3).
When a developer agent pulls in a library that itself has dependencies, the agent also pulls in the relevant dependency parcels.
Agents may decide whether to pull in all such parcels or only a subset, as it is incumbent on that tooling to ensure that the end result functions as intended.

For example, a library called `keyval-storage` might bring in a `redis` parcel and a `memcached` parcel.
In the library invoice, those may be selectable use groups or features.
A developer agent may determine that only `redis` is supported and remove `memcached`.
Or the agent may include both.

When the developer agent removes that dependency the application agent no longer has access to it as a possible configuration.
Therefore, an application runtime should never attempt to walk back up to the `keyvalue-storage` module and attempt to build an alternate dependency tree.
In other words, once a developer has indicated exclusion of a library, an application runtime must not override it.

It may be useful to compare this strategy to those in other systems:

- Assume everything is executable: OCI (Docker) does not differentiate between a container that is runnable and one that is not. It is a presumption of the system that all images are executable. In practice, this is not necessarily the case.
- Make no assumptions: NPM supports both executable and library packages, and does not distinguish. User-facing documentation is the only source of information.
- Use convention: Apt/DPkg typically uses naming conventions to distinguish between libraries and applications. Furthermore, Apt has a concept of unbuilt source as well, and source code packages are distinguished by convention.
- Assume everything is source: Go's package system assumes that it is always dealing with source code.

Other considered systems (Cargo, Composer, etc) fell into one of these categories.

Bindle is closer to the `Use convention` strategy employed by Apt.
We could make it explicit by adding a top-level metadata field such as `library: true`.

### Example 1.b: Pure Interface Libraries

It is possible to create a library that is just an interface specification.
Such a library might, for example, only have a WITX parcel.
These libraries are useful for developers during application composition.

For example, one might create a bindle that just defines a `keyvalue-storage` interface in WITX.
Other developer agents may use that bindle as a reference point for compatibility, making statements such as "this bindle implements the WITX supplied in `example/keyvalue-storage/1.2.3`.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/keyvalue-storage"
version = "1.2.3"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "An interface specifying key/value storage"

[[parcel]]
[parcel.label]
sha256 = "cef43e4fead1701e48f3287d35386474cb048264"
mediaType = "application/witx+wat"
name = "keyvalue-storage.witx"
size = 561710
```

An application bindle would never include a parcel that defines an interface without also including at least one parcel that implements the interface.
Application bindles must supply parcels that implement any necessary interfaces,
as an application bindle is always self contained and never has to resort to external reference resolution.

## Example 2: An un-runnable aggregate application

In this case, we can take the same Bindle invoice as before and make a slight alteration:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/hello-world-2"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Greeter"

[[parcel]]
label.sha256 = "3287d35386474cb048264cef43e4fead1701e48f"
label.mediaType = "application/x-not-wasm"
label.name = "hello.wasm"
label.size = 1710256
```

The only changes are the bindle name on line 4 and the media type on line 11. Our mythical `sg1` client does not know how to execute an application of type `application/x-not-wasm`.

```console
$ sg1 example.com/example/hello-world-2/0.1.0
ERROR: Cannot execute media type "application/x-not-wasm"
```

Here's the process `sg1` went through:

1. Fetch the Bindle invoice from example.com/example/hello-world-2
2. Find which parcels need to be loaded
    - In this case, there is only one. However, before even fetching, `sg1` can determine that it cannot execute anything in this invoice.
3. Produce error and clean up.

The important thing to note about this example is that the runtime can detect this problem before it has even fetched the binary data from the remote host.

As we build more complicated examples, this illustrates the case where no satisfactory set of parcels can be composed to execute an aggregate application. The error cases we consider in the remaining examples are largely of this sort. The runtime examines an invoice and determines that it cannot execute the aggregate application, so it exits.

Largely, we do not discuss runtime errors in this document. Runtime errors are those that occur after the application has been loaded. These are not discussed because they do not hinge on the bindle format.

## Example 3: A two-parcel aggregate application

In this example, the invoice points to an aggregate application that has two separate WASM modules (as parcels).

This program takes a ZIP code and predicts the weather based on almanac data. We will reference variants of this program elsewhere in this document, though it is just for illustrative purposes.

When it comes to the structure, the example the aggregate application consists of two modules:

- The main weather module, which handles the CLI processing
- The almanac library module, which makes predictions on the weather based on almanac data

The main module takes user input and then communicates with the almanac module to get the prediction. It then formats the data, prints it, and exits.

```
$ sg1 example.com/examples/weather/0.1.0 80907
High: 72F Low: 52F 
```

The Bindle invoice for this program looks like this:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
origin = "example/libalmanac/5.1.13"
[parcel.label.feature.wasm]
library = "true"
```

The weather app above is named `example/weather`, and has two parcels attached. Since neither is annotated otherwise, they are both required. When the `sg1` runtime executes this application, it will go through the following steps:

1. Fetch the Bindle invoice from example.com/example/weather
2. Find which parcels need to be loaded
    - There are two required parcels, so it will fetch both
    - The media type for each is runnable
3. Fetch the parcels
4. Start the runtime and load the parcel
    - The `libalmanac.wasm` parcel is annotated with a label that says it is a library (`library = "true"`). So sg1 will assume that `libalmanac.wasm` is not the entry point
    - The `weather.wasm` does not declare a type, so it is considered an entrypoint by default
5. Run the program to completion
    - The runtime will load both modules, each into its own isolated environment.
    - Because `weather.wasm` is marked as an entry point, it will be directly invoked (e.g. its `_start()` or `main()` will be called)
    - The exported symbols defined in `libalmanac.wasm` will be made available to `weather.wasm`
    - When `weather.wasm` calls a function defined in `libalmanac.wasm`, the runtime will perform the call on `weather.wasm`'s behalf and return the data to `weather.wasm`
    - The `weather.wasm` will run to completion and exit
7. Clean up the parcel
    - The environments for both `weather.wasm` and `libalmanac.wasm` will be torn down

Here we do not go into any detail about the interchange between the two modules. That is a detail outside the present scope. In practice, this functions something like an RPC.

The most important detail of this example is that the Bindle invoice provided sufficient information for the runtime to determine how to execute this.

> This design does not dictate that an aggregate application can have only one entrypoint. When there are multiple entrypoints, the runtime is free to choose which to execute.

### How `example/weather` uses `example/libalmanac`

In this example, the `libalmanac.wasm` file comes from `example/libalmanac/5.1.31`.
In example 1.a, we covered how library modules are declared.
Here, the `origin` label points to the upstream library module so that an auditing agent has a way of tracing provenance.

In the behind-the-scenes process, the `example/weather` developer fetched the `example/libalmanac` module, including it in their application.
Upon producing the `example/weather/0.1.0` invoice, the developer included the `libalmanac.wasm` parcel into this invoice.
It is up to tools which parcels are placed into the invoice. So, for example, tooling may also include the `libalmanac.witx` file as well.
The important criterion is that the invoice should contain all of the parcels required to run the application.

A WITX file may be valuable in some cases, and thus may be included. However, it may prove superfluous, as
(a) the Invoice itself is a statement of compatibility, and the runtime does not need to ensure that the included parcels are compatible, and
(b) the information present in the WITX may be compiled into the module itself.
But a decision as to what is best is left to runtime implementations, and makes no practical difference in the guidance offered in this document.

> Note that because of signing and hashing, `origin` cannot be forged to point to the wrong origin.
See the [label specification](./label-spec.md) and the [invoice specification](./invoice-spec.md)
for details on how this functions.

## Example 4: Remote execution of a library

Say we are running `sg1` on a device that is constrained in the amount of memory it can allocate. Here is the application definition from Example 3, which we will re-use here:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
```

Say the combination of the `1710256` bytes for `weather.wasm` and the `2561710` for `libalmanac.wasm` exceed the total amount of space the system can accommodate. Further, imagine that `sg1` has been linked to an account that has the ability to execute on a remote host.

Given that, when the `sg1` program is run, it does the following:

1. Fetch the Bindle invoice from example.com/example/weather
2. Find which parcels need to be loaded
    - There are two required parcels, but they are too big
    - The media type for each is runnable
3. Fetch the local parcel
    - Fetch the `weather.wasm` parcel that sg1 will execute locally
4. Assign parcel to remote host
    - Request a remote host fetch `libalmanac.wasm`
    - The exact host (and how the runtime decides) are determined by sg1's local configuration as well as the remote host's configuration
5. Start the runtime and load the parcel
    - The `weather.wasm` does not declare a type, so it is considered an entry point by default.
    - sg1 executes this locally
    - The remote host loads `libalmanac.wasm` in an instance reserved for this `sg1` session
    - The `libalmanac.wasm` parcel is a library, so the remote host will not try to execute an entrypoint
6. Run the program to completion
    - The `weather.wasm` is directly invoked on the local host
    - The exported symbols defined in `libalmanac.wasm` are made available to `weather.wasm`
    - When `weather.wasm` calls a function defined in `libalmanac.wasm`, the runtime will proxy that request to the remote runtime, which will perform the call on `weather.wasm`'s behalf and return the data (over the network) to the local runtime. It will send the data to `weather.wasm`
    - The `weather.wasm` will run to completion and exit
7. Clean up the parcel
    - The local sg1 will send a teardown request to the remote host, which will destroy the `libalmanac.wasm` instance
    - The local sg1 will destroy the `weather.wasm` environment

Other than perhaps network latency, the user will see identical behavior between this scenario (Example 4) and the previous (Example 3).

Again, the details of how the local and remote host communicate and manage sessions, state, etc. is outside the scope. It is important to note, though, that the decision to run part locally and part remotely was delegated to the `sg1` tool. But the information that sg1 used to determine how to execute was information it could obtain from the invoice.

## Example 5: Heavy and light alternatives

Continuing the vein of the 4th example, in this case we will look at a configuration where there are multiple alternatives for running an aggregate application.

In this case, the weather example has two alternative implementations of the almanac. They share the same exported function signatures, but there is a full version and a "lite" version, where the lite version only has a limited dataset.

The lite version is much smaller, but also has a lower probability of returning useful information.

Here's the invoice:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/better-weather"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[group]]
name = "almanac"
satisfiedBy = "oneOf"
required = true

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
[parcel.conditions]
memberOf = ["almanac"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac-lite.wasm"
size = 11710
[parcel.label.feature.wasm]
library = "true"
[parcel.conditions]
memberOf = ["almanac"]
```

There are several new features in this invoice. But they are all related to the idea of a parcel group.

A parcel group declares a collection of parcels that belong together. If a parcel is not assigned to a group, it is assumed to be in a global group (where all members are required). That is why up to this point every parcel has been fetched by the runtime.

But now we declare a new `group` on lines 9-12:

```toml=
[[group]]
name = "almanac"
satisfiedBy = "oneOf"
required = true
```

The group is named `almanac`. It is `required`, meaning that a runtime must load this group. But in this case, a group is considered loaded if `oneOf` the parcels in the group is loaded. (Other `satisfiedBy` values include `allOf` and `anyOf`.)

We assign two parcels to the `almanac` group using the `parcel.conditions`:

```toml=
[parcel.conditions]
memberOf = ["almanac"]
```

If we chain these together, we are expressing the following:

> One of of the members of the `almanac` group must get loaded by the runtime, and the two options are `libalmanac.wasm` and `libalmanac-lite.wasm`.

Assume that sg1's environment has enough memory to run the `libalmanac-lite.wasm`, but not `libalmanac.wasm`.

For the first time in our examples, it is possible for the sg1 client to take more than one route to execution:

- It could use the `libalmanac-lite.wasm` and run everything locally
- It could use the `libalmanac.wasm` and run in a mixed local/remote mode, as in Example 4.

We do not have to prescribe how sg1 would make this decision. It may detect a poor network connection, and opt for speed over accuracy. Or it may opt for accuracy and load the remote module. In fact, it could even load both, and try one, falling back to the other if necessary. (e.g. if the network goes down, use the local copy.)

## Example 6: Different target runtimes

To this point, we have focused on sg1, a command-line runner. Assume we have a second client called _sgu_. The sgu client supports a graphical user interface. It provides this by executing the WASM modules inside of an Electron instance, binding a series of built-in libraries to an appropriately configured WASM module.

In practice, then, a WASM module that is tuned for sgu may have access to a `window` object or an `application` object, where those objects are provided via bindings to the sgu runtime.

Of course, this introduces some difficulties: A runtime now needs to be able to determine whether a given parcel requires such a runtime environment.

Here is an example of the weather app whose entry point requires the sgu bindings:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather-ui"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
```

The important thing in the example above is the new declaration on line 16: The main entrypoint declares that it needs a `ui_kit` of `electron+sgu`.

When the sg1 runtime is used, it would read the Bindle invoice and see the `ui_kit` requirement. Since it cannot satisfy that condition, it must exit with an error.

```console
$ sg1 example.com/example/hello-world/0.1.0
ERROR: sg1 does not support ui_kit "electron+sgu"
```

But if the sgu runtime executes this program, it will be able to satisfy the `ui_kit` requirement and run the program.

## Example 7: Supporting files

Building on the previous example, the runtime might need extra data that is not merely a WASM module. For example, The `electron+sgu` UI kit might allow passing in a CSS file as well.

This is accomplished by adding the file as a parcel.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather-ui"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"

[[parcel]]
[parcel.label]
sha256 = "ef43e4fead1701e48f3287d35386474cb048264c"
mediaType = "text/css"
name = "style.css"
size = 6620
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"
```

The last item on this invoice is a CSS file (named `style.css` and with media type `text/css`).

In this case, we add the `ui_kit` requirement to the CSS for added safety, though we know that the sg1 runtime would fail regardless of what this annotation is set to. Later, however, we will see how this can be useful.

With the `ui_kit` label attached, we assume that the sgu runtime will read that file and understand what to do with it.

Again, the sg1 client would merely fail when confronted with one or more parcels with the `ui_kit` annotation.

## Example 8: Multiple UIs

The last two examples showed cases where sgu could execute an aggregated app, but sg1 could not. But we could re-organize our app a bit, and do something akin to what the Web browser world calls "progressive enhancement." We can write a bindle that allows the runtime to select an entry point that provides the best user experience.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather-progressive"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[group]]
name = "entrypoint"
satisfiedBy = "oneOf"
required = true

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather-ui.wasm"
size = 1710256
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"
[parcel.conditions]
memberOf = ["entrypoint"]

[[parcel]]
[parcel.label]
sha256 = "048264cef43e4fead1701e48f3287d35386474cb"
mediaType = "application/wasm"
name = "weather-cli.wasm"
size = 1410256
[parcel.conditions]
memberOf = ["entrypoint"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
```

Above, we have used the `group` technique to declare two entrpoints, where the runtime must choose exactly one. One entrypoint has a `ui_kit` requirement and the other does not.

When sg1 executes the bundle above, it will read through the `entrypoint` group, determining that it cannot execute `weather-ui.wasm` (because of the `ui_kit` requirement), but determining that it can run `weather-cli.wasm`.

Sg1 will then fetch `weather-cli.wasm` and `libalmanac.wasm` and execute those two locally.

On the other hand, sgu will be able to execute both items in the `entrypoint` group. It may then use its own decision tree (which we don't need to specify) to determine which entrypoint it will run. Assuming the user wants a UI, sgu would likely select that parcel as the `oneOf` match for the `entrypoint` group.

## Example 9: Conditional dependencies with groups

In Example 7, we saw how to include non-WASM files. This example combines the concepts in examples 7 and 8 to conditionally include dependencies when a runtime chooses one parcel versus another.

In this scenario, let's imagine that the `electron+sgu` version requires several extra bits to work, whereas the CLI version is lighter weight and requires fewer dependencies.

Here is the invoice that expresses these conditional dependencies by making richer use of groups.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather-progressive"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[group]]
name = "entrypoint"
satisfiedBy = "oneOf"
required = true

[[group]]
name = "ui-support"
satisfiedBy = "allOf"
required = false

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather-ui.wasm"
size = 1710256
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"
[parcel.conditions]
memberOf = ["entrypoint"]
requires = ["ui-support"]

[[parcel]]
[parcel.label]
sha256 = "048264cef43e4fead1701e48f3287d35386474cb"
mediaType = "application/wasm"
name = "weather-cli.wasm"
size = 1410256
[parcel.conditions]
memberOf = ["entrypoint"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "text/html"
name = "almanac-ui.html"
size = 2561710
[parcel.label.feature.wasm]
dat = "true"
[parcel.conditions]
memberOf = ["ui-support"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "text/css"
name = "styles.css"
size = 2561710
[parcel.label.feature.wasm]
data = "true"
[parcel.conditions]
memberOf = ["ui-support"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "uibuilder.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
[parcel.conditions]
memberOf = ["ui-support"]
```

The invoice above is larger than any we have yet examined. Here is what it does:

- The group `entrypoint` is the same as the one in Example 8.
- The group `ui-support` is a non-required group
    - It has three parcels: an HTML file, a CSS file, and an extra WebAssembly module
    - It is only satisfied when all of the parcels attached to the group are pulled
- The parcel `weather-ui.wasm` _requires_ that the group `ui-support` be pulled.

When sg1 inspects this invoice and builds an app, it will select the `weather-cli.wasm` parcel. Thus, it will execute with only two parcels: `weather-cli.wasm` and `libalmanac.wasm`.

When sgu inspects this invoice, it will build a more complex app. It will select `weather-ui.wasm`, which in turn will require sgu to include the group `ui-support`. That group requires the selection of three more parcels (`almanac-ui.html`, `styles.css`, and `uibuilder.wasm`). So when sgu finally assembles the app, it will have five total parcels.

One difficulty stems from the possibility of running part of this on a remote host: The host may not be able to determine whether a data file like `styles.css` is required by `weather-ui.wasm` or by `uibuilder.wasm` (or both). Any resources marked `data` are ambiguous in this way. Runtimes may support any number of ways to disambiguate this problem, or we may need to add some additional features in the `feature.wasm` section for `data = "true"`. For example, we could add a `requiredBy = []` definition.

## Example 10: The shim parcel pattern

**TODO:** Consider a polyfill `type` as an alternative approach to this.

In the last few examples, we have seen cases where the runtime provides particular features that a client may take advantage of. The `sgu` runtime exposes an `electron+sgu` UI toolkit.

What do we do if we want to make it possible for a selection algorithm to mock out a facility as a WASM module instead of having the host implement it?

For that case, we can use a shim parcel pattern.

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/weather-ui-shim"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
description = "Weather Prediction"

[[group]]
name = "ui-shim"
satisfiedBy = "one"
required = true

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather.wasm"
size = 1710256
[parcel.label.feature.wasm]
ui_kit = "electron+sgu"
[parcel.conditions]
memberOf = ["ui-shim"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "weather-mock-ui.wasm"
size = 1710256
[parcel.conditions]
memberOf = ["ui-shim"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "libalmanac.wasm"
size = 2561710
[parcel.label.feature.wasm]
library = "true"
```

In this case, we have two things that satisfy the entrypoint requirements, but one of them is a mock UI. THe idea is that a runtime would allow the user to select cases like this explicitly:

```console
$ sgu example.com/example/weather-ui-shim/0.1.0 \
  --require-parcel-name weather-mock-ui.wasm
```

But this is an implementation detail that a runtime may or may not support.


## A Note on Caching Parcels

Parcels are content addressed by SHA, which means they can safely be cached.

A runtime may therefore cache a module and safely reuse it to satisfy any bindle invoice that requests that particular SHA.

The algorithm may then be something like this:

1. Read the bindle invoice
2. Look at the local cache for any parcels that match SHAs in the bindle invoice
3. For any unfound parcels, fetch them from the remote bindle server

## The `feature.wasm` fields

The following is a definition of the fields that can be in the Parcel label's `feature.wasm` field.

- `library`: Boolean string. Indicates that the parcel contains a library. One of `true`, `t`, `false`, `f`.
- `entrypoint`: Boolean string. Indicates that the parcel contains a thing that can be executed to start an application. One of `true`, `t`, `false`, `f`.
- `data`: Boolean string. Indicates that the parcel contains opaque data. One of `true`, `t`, `false`, `f`.
- `ui_kit`: String. The name of a UI toolkit that must be present to execute this module. The value is undefined by the spec, and individual runtimes are allowed to declare their own. If this is not present, agents must assume that the app does not require a UI toolkit.
- `wasi`: boolean string. Whether or not WASI support is required. One of `true`, `t`, `false`, `f`. The default is `true`.

We might also add a `feature.wasm-opt` field that would allow optional (not required) "progressive enhancements" as well.

TODO: How might we handle the case where a shim module could mock a requirement declared in `feature.wasm`?

## Considerations for Beaming Parcels

In several places in this document, we have discussed the idea of running parcels on hosts other than the local host. The process for distributing application components in this way is what we have called "beaming" elsewhere.

There are some design implications that Bindle needs to determine. Most notable among them is whether beaming from Host A to Host B is necessarily sending the parcel from Host A to Host B, or whether it might be directing Host B to contact a Bindle server and fetch the parcel directly.

So in this beaming model, Host A may request that Host B run a parcel, and Host B may then directly request that parcel from the Bindle server. This is a greatly advantageous model in cases where Host A may be on a constrained network. In this case, Host A's not having to fetch and then send the parcel is much more fitting. Host A would merely send Host B the parcel ID (and possibly the Bindle server URL), and Host B would fetch the parcel.

> One design detail of Bindle is that Bindle can host an invoice that points to parcels that it does not have. That is, there is no requirement that a client, upon posting an invoice, MUST also post all of the parcels. As a caveat, we may change this behavior. But the intention of the design was to allow it to be the case that parcels could be distributed. That is, an invoice may be pushed to one location while the parcels are pushed to another. While there is no defined mechanism in the present spec, we envisioned a meta-level service that may be able to locate where an agent may find a particular parcel. Because a runtime can calculate ahead-of-run whether it cannot get all of the necessary parcels, this trade-off feels okay. Again, though, this behavior is subject to change. 

This all raises an interesting issue that Bindle would need to participate in solving: If Host A sends Host B the parcel and expects Host B to fetch it, then we may need some way to certify (for AuthN/Z) that Host B is _allowed_ to fetch the parcel on Host A's behalf.

The blessings model from Vanadium is one example of how we could do this.

### Dependency Collapses when Dealing with Beamed Parcels

Say we have a modular dependency graph like this:

```
A
|- B
|
|- C
   |- B
```

In this example, A requires two modules: B and C. C requires one module: B.

When flattening a dependency tree, the above can become:

```
A
|- B
|- C
```

But when generating Bindles, the dependency tree MUST NOT be flattened in the parcel list.

So the appropriate way to express the initial dependency tree is something like this:

```toml=
bindleVersion = "1.0.0"

[bindle]
name = "example/dep-tree"
version = "0.1.0"

[[group]]
name = "a-dependencies"
satisfiedBy = "allOf"

[[group]]
name = "c-dependencies"
satisfiedBy = "allOf"

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "A.wasm"
[parcel.conditions]
requires = ["a-dependencies"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "B.wasm"
[parcel.conditions]
memberOf = ["a-dependencies", "c-dependencies"]

[[parcel]]
[parcel.label]
sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
mediaType = "application/wasm"
name = "C.wasm"
[parcel.conditions]
memberOf = ["a-dependencies"]
requires = ["c-dependencies"]
```

The salient detail here is that the Bindle interpreting routine can recompose from the
above the DAG of dependencies. The Bindle representation of the above ends up being
something like this:

```
A.wasm
  |- a-dependencies
         |- B.wasm
         |- C.wasm
             |- c-dependencies
                  |- B.wasm
```

Now, if Bindle delegates C to a remote host for execution, it knows that it needs to beam
both B.wasm and C.wasm to the remote host.

In more sophisticated trees, the Bindle engine may even be able to calculate the
cost of sending one aggregate of WASMs versus another. In other words, it can determine the
total runtime requirements of all modules that must be run together in concert, and then
determine which aggregate subset should be beamed to a remote host.