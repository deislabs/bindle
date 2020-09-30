# The Bindle Format Specification

The purpose of the Bindle specification is to describe a format for transporting collections of (potentially large) related files in such a way that they can be safely shared and re-used.

A bindle consists of an `invoice.toml` file and one or more `parcel` objects.

The English word "bindle" refers to a collection or package. A popular portrayal of the bindle is a handkerchief-wrapped bundle hanging off of a stick. (The word itself is likely a corruption of the word "bundle"). This specification describes a format called "bindle" together with an API for storage and retrieval of bindles.

## Bindles and Parcels

A _bindle_ is a named and versioned container that enumerates its contents in an _invoice_. The contents of a bindle are opaque blobs of data describes as _parcels_, where each parcel has a _label_ describing it, and a _parcel.dat_ that contains the opaque data of the parcel.

```
BINDLE
  |
  |- invoice
  |
  |- parcels
       |- parcel 1
       |    |- label
       |    |- parcel.dat
       |
       |- parcel 2
       |    |- label
       |    |- parcel.dat
       |
       |- ...
```

Parcels are individual items identified by a hash (typically a SHA-256 digest). Parcels are individually addressable, meaning that if two bindles reference the same parcel, implementations can be sure that it is _exactly_ the same parcel, and can thus share/reuse the parcel between the two bindles.

## What Is Bindle Good For?

Bindles are good for expressing the relationship between related chunks of data. Here are some examples of how Bindle may be used.

### Packaging a website.
 Bindle could be used to model a website by storing each HTML page, image, script, and stylesheet a `parcel`, and then creating an `invoice` that listed each parcel.

 As the website changed and evolved, the `invoice` would be versioned (e.g. `v1.0.1`, then `v1.0.2`...). When the website is deployed, the deployment target inspects the `invoice` to see which `parcel`s changed. Then it installs just the changed `parcel`s. This can drastically cut down the amount of data that must be moved around with each website deployment

 ### Packaging Programming Language Dependencies

 Bindle can be used to package an application and its dependencies. For example, a typical Node.js application has many dependencies. A Bindle may describe the top-level application.

 The first `parcel` in the invoice may point to the main application, while the rest of the `parcels` point to the specific dependencies of that application (i.e. the `package.lock` file would be represented as a list of `parcel`s).

 During deployment, the Bindle manager could deploy just the resources that have changed since the last deployment.

 This is an illustration of how this might work for a programming language. Of course, in this case it might not add value over above NPM, but one could imagine implementing a programming language dependency manager that uses Bindle as a backend.

 ### An Operating System Package Manager

 Bindle can be used to package program like a traditional package manager. In this case, an application binary is packaged as a `parcel`, and all of its dependencies are also packaged as `parcel`s. Bindle can tell very quickly and precisely whether a program's dependencies are present and at the correct version.

 Moreover, Bindle supports conditional groups, which means a single package could allow the user to conditionally install some behaviors, add-ons, and dependencies. Examples that Bindle can support:

 - My application requires a Borne-style shell, which could be Bash, Korn, Zsh, or Busybox. At least one of these must be present.
 - My application has several optional add-ons that the user can choose from (though none are required).
 - My application needs to have at least one web server installed, but if the user chooses to install Apache, it must also install a special Apache module. Bindle can express these kinds of conditional chains.

 ### A Really Awesome Distributed Execution Environment in WebAssembly

 Bindle can express that on some systems, one `parcel` will do the trick, while on another system, a different `parcel` should be chosen. Furthermore, it can enable one system to request of another system that it needs the other system to run a very specific workload on its behalf.

 If that doesn't make sense, you haven't seen Stargazer Tian-yan yet.

 ## The Specifications

 - Start with the [Invoice Specification](invoice-spec.md) to learn about the top-level description of a Bindle
 - The [Parcel Specification](parcel-spec.md) describes the parts of a parcel
 - The [Label Specification](label-spec) descries a parcel's label TOML file
 - The [Protocol Specification](protocol-spec) describes how Bindles are transported from place to place, and how parcels are intelligently fetched based on need.

 And for reference, if TOML is new for you, read the [TOML specification](https://toml.io/en/v1.0.0-rc.2).