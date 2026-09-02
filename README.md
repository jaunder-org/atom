# atom

[![Build status](https://github.com/rust-syndication/atom/workflows/Build/badge.svg)](https://github.com/rust-syndication/atom/actions)
[![Crates.io Status](https://img.shields.io/crates/v/atom_syndication.svg)](https://crates.io/crates/atom_syndication)
[![Coverage](https://codecov.io/gh/rust-syndication/atom/branch/master/graph/badge.svg)](https://codecov.io/gh/rust-syndication/atom/)

Library for serializing the Atom web content syndication format.

[Documentation](https://docs.rs/atom_syndication/)

This crate requires *Rustc version 1.83.0 or greater*.

## Usage

Add the dependency to your `Cargo.toml`.

```toml
[dependencies]
atom_syndication = "0.13"
```

Or, if you want [Serde](https://github.com/serde-rs/serde) include the feature like this:

```toml
[dependencies]
atom_syndication = { version = "0.13", features = ["with-serde"] }
```

The package includes a single crate named `atom_syndication`.

```rust
extern crate atom_syndication;
```

## Namespace-aware extensions

Extensions are represented by expanded names: a namespace URI plus local name.
XML spells each name as `local` or `prefix:local`; parsing resolves that source
spelling to an expanded name. This example reads an element-scoped namespace
declaration, looks up the extension by expanded name, and round-trips it:

```rust
use atom_syndication::Entry;

let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>Example</title><x:rating xmlns:x="urn:example" x:stars="5"/></entry>"#;
let entry = Entry::read_from(xml.as_slice()).unwrap();

let rating = entry.extensions().iter().find(|extension| {
    extension.name.namespace_uri.as_deref() == Some("urn:example")
        && extension.name.local_name == "rating"
});
assert!(rating.is_some());

let reparsed: Entry = entry.to_string().parse().unwrap();
assert_eq!(
    reparsed.extensions()[0].name.namespace_uri.as_deref(),
    Some("urn:example")
);
```

`preferred_prefix` is serialization metadata, not semantic identity: writing may
choose a different prefix or declaration placement while
preserving each expanded name, attribute, and mixed-content order. For the
breaking 0.12 to 0.13 API change, including construction and lookup migration,
see the
[0.13 migration guide](CHANGELOG.md#0130---2026-09-01).

## Reading

A feed can be read from any object that implements the `BufRead` trait or using the `FromStr` trait.

```rust
use std::fs::File;
use std::io::BufReader;
use atom_syndication::Feed;

let file = File::open("example.xml").unwrap();
let feed = Feed::read_from(BufReader::new(file)).unwrap();

let string = "<feed></feed>";
let feed = string.parse::<Feed>().unwrap();
```

## Writing

A feed can be written to any object that implements the `Write` trait or converted to an XML string using the `ToString` trait.

### Example

```rust
use std::fs::File;
use std::io::{BufReader, sink};
use atom_syndication::Feed;

let file = File::open("example.xml").unwrap();
let feed = Feed::read_from(BufReader::new(file)).unwrap();

// write to the feed to a writer
feed.write_to(sink()).unwrap();

// convert the feed to a string
let string = feed.to_string();
```

## Invalid Feeds

As a best effort to parse invalid feeds `atom_syndication` will default elements declared as "required" by the Atom specification to an empty string.

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
