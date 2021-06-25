# Bindle: Aggregate Object Storage

> This repository is a :100: experimental code created by the DeisLab team on a whim. We really don't think you should use this in production.

## Aggregate Object Storage Means Keeping Related Things Together

A photo album. A sock drawer. A bookshelf. We like storage solutions that less us keep related things in a single location.

Consider the humble silverware drawer. When we set the table for dinner, it's convenient to open one drawer and get the forks, spoons, and knives. Yes, a fork is a different thing than a knife. Yes, there are multiple different kinds of spoons. And, yes, silverware is not even uniform in size or shape brand-to-brand, model-to-model. Some people keep chopsticks with the silverware. Others toss in those tiny spreader things you use to slather on the cream cheese at a fancy party. In my house we keep the straws in the silverware drawer. Drawers are flexible. They can accommodate these variances.

Bindle is the digital silverware drawer.

More specifically, Bindle is an _aggregate object storage system_. Merriam-Webster defines an aggregate as "a mass or body of units or parts somewhat loosely associated with one another." The fundamental feature of Bindle is that it provides you with a way to group your associated objects into an organized and named unit. It thinks in terms of aggregates.

### The Usefulness of Aggregate Object Storage

Again with the silverware, the attraction of the metaphor is how relatable it is. We understand why we need silverware drawers. But why do we want aggregate object storage?

Let's look at a classic C/C++ program developed in a UNIX-like environment. Many times, the modern C/C++ application makes use of shared objects (SOs). These are binary files that contain the compiled version of a shared library. Say we have a top-level program called `maple` that uses several shared object files. In order to execute our `maple` program, we need to make sure we have all of the required SO files present on the same filesystem as `maple`. And when moving `maple` to another system, we need to also move all of those SOs. So `maple` is really an aggregate application: To run it, we need to keep track several different pieces.

We can jump to a different part of our tech stack for another example. A web application may have components in HTML, in JavaScript, CSS, and even images or other media. No one thing on that list is "the application." The application is an aggregate of all of those resources.

In the last few years, we've even seen the emergence of large-scale distributed computing. In this world, aggregates of microservices together make up a single application. Indeed, as the industry progresses, it is essential that we learn how to capture the definition of an application as an aggregate of related programs.

Bindle is a tool for treating the group of related parts as a thing itself. In geology an "aggregate" is a single chunk of rock that is composed of an assortment of individual minerals. Modern applications are like geological aggregates. We want to be able to talk about the individual parts, but within the context of the whole that they represent.

### What Does an Aggregate Application Look Like?

To take our web application example above, Bindle would see that application as a unit that looked something like this:

```
my-web-app 1.2.3
  |- index.html
  |- style.css
  |- library.js
  |- pretty-picture.jpg
```

Our top-level application, `my-web-app 1.2.3`, is composed of four individual parts that all need to be present. It's simultaneously important for us to talk about the aggregate as a whole while still appreciating the individuality of each of its parts.

Bindle goes one step further, though: It allows you to express relationships between these parts. Keeping with the silverware drawer example, it lets you say "these are both spoons, but this spoon is only used when we are having soup, while that one is used for tea."

To that end, Bindle supports a more complex notion of composition. We might have a case where one part of the application has requirements that can be satisfied by multiple different parts. Here's an example: Say we have an application that reads through a pool of sports data and makes projections about who will win this weekend's SportsBall game.

A frontend is the user interface, and it connections to some prediction engine. The prediction engine might use a set of simple statistical prediction rules, or it might use a sophisticated machine learning algorithm. Which engine we use may be determined by a range of factors, including the capabilities of the system on which it is run or the accuracy demanded of the output.

Bindle can model this situation by keeping all of the objects stored together, and letting the client figure out which combination it needs. So the Bindle looks like this:

```
sports-predictionator 2.0.0
  |
  |- preditionator-frontend
  |- One of
        |- lib-machine-learning
        |- lib-statistical-prediction
```

A client with plenty of time and resources might select the lib-machine-learning one, while a constrained client might pick the simple statistical formulas. But the bindle describes both possibilities.

While it is not apparent from these simple examples, Bindle provides information that helps runtimes make these decisions. Take a look at [the specifications](docs/bindle-spec.md) to get into the details.

Still, there's a little more to the Bindle story.

### Don't Store the Same Thing Twice

Rewinding to our example of the C program that used shared objects, one important word in this design is _shared_. Modern applications, be they web applications or system tools, benefit from sharing. Bindle, too, cares about this. Sharing is also good when it comes to cost. "Storage is cheap." No, it's really not. And bandwidth charges are certainly far from cheap. Bindle is structured so that:

1. An object is only stored once (where "object" here means "unique stream of bytes")
2. Clients only have to pull objects they don't already have -- and it's easy for them to figure this out.
3. Servers inform clients about when they need to send data. There's no reason for a client to be compelled to upload data that the server already has.
4. All of this is done with [content addressable storage](https://en.wikipedia.org/wiki/Content-addressable_storage) and cryptographically secure hashing and signing.

Because of this, Bindle can keep download times fast, bandwidth costs low, and storage space minimized--all without sacrificing data integrity.

Enough with talk, let's get down to the business of using Bindle.

## Using Bindle

To build Bindle, you can use `make` or `cargo`:

```console
$ # Recommended
$ make build
$ # The above is approximately equivalent to:
$ cargo build --feature=cli --bin bindle
$ cargo build --all-features --bin bindle-server
```

The binaries will be built in `target/debug/bindle` and `target/debug/bindle-server`.
For both client and server, the `--help` flag will print out documentation.

### Starting the Server

To start the compiled server, simply run `target/debug/bindle-server`. If you would like
to see the available options, use the `--help` command.

If you would like to run the server with `cargo run` (useful when debugging), use `make serve` or `make serve-tls`.
(The first time you run `make serve-tls`, it will prompt you to create a testing TLS cert.)

#### Supplying a Configuration File

The Bindle server looks for a configuration file in `$XDG_DATA/bindle/server.toml`.
If it finds one, it loads configuration from there.
You can override this location with the `--config-path path/to/some.toml` flag.

```toml
address = "127.0.0.1:8080"
bindle-directory = "/var/run/bindle"
cert-path = "/etc/ssl/bindle/certificate.pem"
key-path = "/etc/ssl/bindle/key.pem"
```

### Running the Client

If you compiled, the client is in `target/debug/bindle`. You can also run from source with
`cargo run --features=cli --bin=bindle` or `$(make client)` (e.g. `$(make client) --help`).

You will either need to supply the `--server` parameter on the command line or set the `BINDLE_SERVER_URL`.

```console
$ export BINDLE_SERVER_URL="http://localhost:8080/v1"
$ # Running from build
$ target/debug/bindle --help
$ # Running from Cargo
$ cargo run --bin bindle --features=cli -- --help
$ # Running from make
$ $(make client) --help
```

For more, see [the docs](docs/README.md).

## Concepts

In the Bindle system, the term _bindle_ refers to a _bundle of related data called parcels_.
A _bindle_ might be simple, containing only a single binary file. Or it may be complex, 
containing hundreds of discrete data objects (files, libraries, or whatnot). It can
represent a layer diagram, like Docker, or just a regular file download. With experimental
conditions, it can even represent packages containing mandatory, optional, and conditional
components.

A bindle is composed of several parts:

- The _invoice_ (`invoice.toml`) contains information about the bindle (`name`, `description`...)
  as well as a manifest of parcels (individual data items).
- A _parcel_ has a (`parcel.dat`) that contains the opaque and arbitrary data

A _bindle hub_ is a service that manages storage and retrieval of bindles. It is available
via an HTTP/2 connection (almost always over TLS). A hub supports the following actions:

- GET: Get a bindle and any of its parcels that you don't currently have
- POST: Push a bindle and any of its parcels that the hub currently doesn't have
- DELETE: Remove a bindle

Note that you cannot modify any part of a bindle. Not the payload. Not the name. Not even
the description. Bindles are truly immutable. It's like the post office: Once you ship a
package, you can't go back and change it. This greatly increases the security of the
entire system.

### Bindle Names

There are many fancy naming conventions in the world. But Bindle eschews the fancy in
favor of the easy. Bindle names are _paths_. The following are all valid bindle names:

- `mybindle`
- `mybindle.txt`
- `example.com/stuff/mybindle`
- `mybindle/v1.2.3`

While all of the above are valid bindles, those that end with a version string (a SemVer)
have some special features. Thus, we recommend using versioned bindle names:

- `mybindle/v1.0.0`
- `mybindle/v1.0.1-beta.1+ab21321`
- `example.com/stuff/mybindle/v1.2.3`

### First-class Semver

One frequently used convention in the software world is _versioning_. And one standard for
version numbering is called [SemVer](https://semver.org). Bindle has strong support for SemVer. 

Each Bindle invoice MUST have a semantic version. There is no `head` or `latest` in Bindle. Every release is named with a specific version number. With this strong notion of versioning, we can track exact objects with no ambiguity. (Remember, Bindles are immutable. A version number is always attached to exactly one release.)

And SemVer queries are a way of locating "near relatives" of bindles.

For example, searching for `1.2.3` of a bindle will return an exact version. Searching
for `1.2` will return the latest patch release of the 1.2 version of the bindle (which
might be `1.2.3` or perhaps `1.2.4`, etc).

Version ranges must be explicitly requested _as queries_. A direct fetch against `v1.2`
will only return a bindle whose version string is an exact match to `v1.2`. But a version
query for `v1.2` will return what Bindle thinks is the most appropriate matching version.

## The Bindle Specification

The [docs](docs/) folder on this site contains the beginnings of a formal specification
for Bindle. The best place to start is with the [Bindle Specification](docs/bindle-spec.md).

## Okay, IRL what's a "bindle"

The word "bindle" means a cloth-wrapped parcel, typically of clothing. In popular U.S. 
culture, hobos were portrayed as carrying bindles represented as a stick with a
handkerchief-wrapped bundle at the end. Also, it sounds cute.