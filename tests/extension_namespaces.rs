use atom_syndication::extension::{ExpandedName, Extension, ExtensionAttribute, ExtensionContent};
use atom_syndication::{Entry, Feed};

fn expanded(uri: &str, local: &str, prefix: &str) -> ExpandedName {
    ExpandedName {
        namespace_uri: Some(uri.to_string()),
        local_name: local.to_string(),
        preferred_prefix: Some(prefix.to_string()),
    }
}

fn unqualified(local: &str) -> ExpandedName {
    ExpandedName {
        namespace_uri: None,
        local_name: local.to_string(),
        preferred_prefix: None,
    }
}

fn element(content: &ExtensionContent) -> &Extension {
    let ExtensionContent::Element(element) = content else {
        panic!("expected extension element, found {content:?}");
    };
    element
}

/// Protects preservation of an extension namespace declared only on its element.
#[test]
fn standalone_entry_retains_scoped_extension_namespace_declarations() {
    let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>Entry</title><x:item xmlns:x="urn:example:extension">content</x:item></entry>"#;
    let entry = Entry::read_from(xml.as_slice()).unwrap();

    let serialized = entry.to_string();

    assert!(
        serialized.contains(r#"xmlns:x="urn:example:extension""#),
        "serialized entry loses the scoped extension namespace declaration: {serialized}"
    );
    assert!(
        serialized.contains("<x:item"),
        "serialized entry loses the extension prefix: {serialized}"
    );
    assert!(
        serialized.parse::<Entry>().is_ok(),
        "serialized entry is not reparsable: {serialized}"
    );
}

/// Protects prefixed Atom elements from being mistaken for modeled Atom fields.
#[test]
fn prefixed_atom_name_is_retained_as_an_extension() {
    let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><a:title xmlns:a="http://www.w3.org/2005/Atom">T</a:title></entry>"#;
    let entry = Entry::read_from(xml.as_slice()).unwrap();

    assert!(entry.title.value.is_empty());
    assert_eq!(entry.extensions.len(), 1);
    assert_eq!(
        entry.extensions[0].name,
        expanded("http://www.w3.org/2005/Atom", "title", "a")
    );
    assert_eq!(
        entry.extensions[0].content,
        vec![ExtensionContent::Text("T".into())]
    );
    assert_eq!(entry.to_string().parse::<Entry>().unwrap(), entry);
}

/// Protects scoped namespace resolution, attributes, and mixed content in one extension tree.
#[test]
fn standalone_entry_preserves_scoped_namespaces_attributes_and_mixed_content() {
    let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><x:item xmlns:x="urn:one" x:same="one" xmlns:y="urn:two" y:same="two"><child xmlns="urn:default" plain="outside-default">before<x:child xmlns:x="urn:rebound"/>after</child><x:duplicate>first</x:duplicate><x:duplicate>second</x:duplicate></x:item></entry>"#;
    let entry = Entry::read_from(xml.as_slice()).unwrap();
    let item = &entry.extensions[0];
    assert_eq!(item.name, expanded("urn:one", "item", "x"));
    assert_eq!(item.attributes.len(), 2);
    assert_ne!(item.attributes[0].name, item.attributes[1].name);
    let child = element(&item.content[0]);
    assert_eq!(child.name.namespace_uri.as_deref(), Some("urn:default"));
    assert_eq!(child.attributes[0].name.namespace_uri, None);
    assert_eq!(
        child.content[0],
        ExtensionContent::Text("before".to_string())
    );
    let rebound = element(&child.content[1]);
    assert_eq!(rebound.name.namespace_uri.as_deref(), Some("urn:rebound"));
    assert_eq!(
        child.content[2],
        ExtensionContent::Text("after".to_string())
    );
    let first_duplicate = element(&item.content[1]);
    let second_duplicate = element(&item.content[2]);
    assert_eq!(first_duplicate.name, second_duplicate.name);
    assert_eq!(
        first_duplicate.name.namespace_uri.as_deref(),
        Some("urn:one")
    );
    assert_eq!(entry.to_string().parse::<Entry>().unwrap(), entry);
}

/// Protects namespace inheritance and rebinding through feed, entry, and source contexts.
#[test]
fn feed_and_embedded_source_preserve_inherited_and_rebound_namespaces() {
    let xml = br#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:x="urn:one"><title>F</title><id>f</id><updated>1970-01-01T00:00:00Z</updated><x:feed/><entry><title>E</title><id>e</id><updated>1970-01-01T00:00:00Z</updated><x:entry/><source><title>S</title><id>s</id><updated>1970-01-01T00:00:00Z</updated><x:source><x:rebound xmlns:x="urn:two"/></x:source></source></entry></feed>"#;
    let feed = Feed::read_from(xml.as_slice()).unwrap();
    assert_eq!(
        feed.extensions[0].name.namespace_uri.as_deref(),
        Some("urn:one")
    );
    let entry = &feed.entries[0];
    assert_eq!(
        entry.extensions[0].name.namespace_uri.as_deref(),
        Some("urn:one")
    );
    let source = entry.source.as_ref().unwrap();
    assert_eq!(
        source.extensions[0].name.namespace_uri.as_deref(),
        Some("urn:one")
    );
    let rebound = element(&source.extensions[0].content[0]);
    assert_eq!(rebound.name.namespace_uri.as_deref(), Some("urn:two"));
    assert_eq!(feed.to_string().parse::<Feed>().unwrap(), feed);
}

/// Protects extension round trips from a standalone entry's embedded source.
#[test]
fn standalone_entry_with_source_round_trips_extensions() {
    let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:x="urn:one"><title>E</title><source><title>S</title><id>s</id><updated>1970-01-01T00:00:00Z</updated><x:item/></source></entry>"#;
    let entry = Entry::read_from(xml.as_slice()).unwrap();
    assert_eq!(
        entry.source.as_ref().unwrap().extensions[0]
            .name
            .namespace_uri
            .as_deref(),
        Some("urn:one")
    );
    assert_eq!(entry.to_string().parse::<Entry>().unwrap(), entry);
}

/// Protects rejection of malformed XML names and invalid namespace bindings.
#[test]
fn malformed_unbound_prefixes_are_rejected() {
    assert!(Entry::read_from(
        br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><bad:item/></entry>"#
            .as_slice()
    )
    .is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><x:item xmlns:x="urn:x" bad:attribute="v"/></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><x:item xmlns:x="urn:same" xmlns:y="urn:same" x:a="one" y:a="two"/></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><x:item:part xmlns:x="urn:x"/></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:xml="urn:not-xml"><title>T</title></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(
        br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:XML="urn:x"><title>T</title></entry>"#
            .as_slice()
    )
    .is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:x="http://www.w3.org/2000/xmlns/"><title>T</title></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:-bad="urn:x"><title>T</title></entry>"#.as_slice()).is_err());
    assert!(Entry::read_from(br#"<entry xmlns="http://www.w3.org/2005/Atom" xmlns:XMLfoo="urn:x"><title>T</title></entry>"#.as_slice()).is_err());
}

/// Protects prefix-insensitive expanded-name identity and unordered attribute equality.
#[test]
fn equality_ignores_prefix_hints_and_attribute_order_but_not_content_order() {
    let name = expanded("urn:x", "item", "first");
    let equivalent = ExpandedName {
        preferred_prefix: Some("second".into()),
        ..name.clone()
    };
    assert_eq!(name, equivalent);
    let attribute = ExtensionAttribute {
        name: expanded("urn:a", "same", "a"),
        value: "one".into(),
    };
    let other_attribute = ExtensionAttribute {
        name: expanded("urn:b", "same", "b"),
        value: "two".into(),
    };
    let left = Extension {
        name: name.clone(),
        attributes: vec![attribute.clone(), other_attribute.clone()],
        content: vec![
            ExtensionContent::Text("before".into()),
            ExtensionContent::Element(Extension {
                name: expanded("urn:x", "child", "x"),
                attributes: vec![],
                content: vec![],
            }),
            ExtensionContent::Text("after".into()),
        ],
    };
    let right = Extension {
        name: equivalent,
        attributes: vec![other_attribute, attribute],
        content: left.content.clone(),
    };
    assert_eq!(left, right);
    let mut split_text = right.clone();
    split_text.content = vec![
        ExtensionContent::Text("be".into()),
        ExtensionContent::Text("fore".into()),
        left.content[1].clone(),
        ExtensionContent::Text("after".into()),
    ];
    assert_eq!(left, split_text);
    let duplicate = ExtensionAttribute {
        name: expanded("urn:duplicate", "attribute", "d"),
        value: "v".into(),
    };
    let duplicate_left = Extension {
        name: name.clone(),
        attributes: vec![duplicate.clone(), duplicate.clone()],
        content: vec![],
    };
    let duplicate_right = Extension {
        name,
        attributes: vec![duplicate.clone(), duplicate.clone()],
        content: vec![],
    };
    assert_eq!(duplicate_left, duplicate_right);
    let other = ExtensionAttribute {
        name: expanded("urn:other", "attribute", "o"),
        value: "v".into(),
    };
    let left_multiplicity = Extension {
        name: duplicate_left.name.clone(),
        attributes: vec![duplicate.clone(), duplicate.clone(), other.clone()],
        content: vec![],
    };
    let right_multiplicity = Extension {
        name: duplicate_left.name.clone(),
        attributes: vec![duplicate, other.clone(), other],
        content: vec![],
    };
    assert_ne!(left_multiplicity, right_multiplicity);
    let mut reordered = right.clone();
    reordered.content.swap(0, 2);
    assert_ne!(left, reordered);
}

/// Protects equality of text split at Unicode character boundaries.
#[test]
fn equality_compares_unicode_text_at_character_boundaries() {
    let name = expanded("urn:example", "item", "x");
    let combined = Extension {
        name: name.clone(),
        attributes: vec![],
        content: vec![ExtensionContent::Text("café".into())],
    };
    let split = Extension {
        name: name.clone(),
        attributes: vec![],
        content: vec![
            ExtensionContent::Text("caf".into()),
            ExtensionContent::Text("é".into()),
        ],
    };
    assert_eq!(combined, split);

    let ascii = Extension {
        name: name.clone(),
        attributes: vec![],
        content: vec![ExtensionContent::Text("a".into())],
    };
    let unicode = Extension {
        name,
        attributes: vec![],
        content: vec![ExtensionContent::Text("é".into())],
    };
    assert_ne!(ascii, unicode);
}

/// Protects unprefixed elements in foreign default namespaces as extensions.
#[test]
fn unprefixed_foreign_default_namespace_extension_is_preserved() {
    let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T</title><foreign xmlns="urn:foreign"><child/></foreign><title xmlns="urn:foreign">foreign title</title><unknown/></entry>"#;
    let entry = Entry::read_from(xml.as_slice()).unwrap();
    assert_eq!(entry.extensions.len(), 2);
    assert_eq!(
        entry.extensions[0].name.namespace_uri.as_deref(),
        Some("urn:foreign")
    );
    assert_eq!(entry.extensions[0].content.len(), 1);
    assert_eq!(entry.extensions[1].name.local_name, "title");
    assert_eq!(
        entry.extensions[1].name.namespace_uri.as_deref(),
        Some("urn:foreign")
    );
    assert_eq!(entry.to_string().parse::<Entry>().unwrap(), entry);
}

/// Protects namespace-correct output when an element and attribute prefer the same prefix.
#[test]
fn writer_renames_conflicting_preferred_prefixes() {
    let extension = Extension {
        name: expanded("urn:element", "item", "x"),
        attributes: vec![ExtensionAttribute {
            name: expanded("urn:attribute", "value", "x"),
            value: "v".into(),
        }],
        content: vec![],
    };
    let entry = Entry {
        extensions: vec![extension],
        ..Entry::default()
    };
    let written = entry.to_string();
    assert!(written.contains("xmlns:x=\"urn:element\""));
    assert_eq!(written.parse::<Entry>().unwrap(), entry);
}

/// Protects rejection of duplicate attribute expanded names during serialization.
#[test]
fn writer_rejects_duplicate_expanded_attributes() {
    let extension = Extension {
        name: expanded("urn:extension", "item", "e"),
        attributes: vec![
            ExtensionAttribute {
                name: expanded("urn:attribute", "same", "a"),
                value: "one".into(),
            },
            ExtensionAttribute {
                name: expanded("urn:attribute", "same", "b"),
                value: "two".into(),
            },
        ],
        content: vec![],
    };
    assert!(Entry {
        extensions: vec![extension],
        ..Entry::default()
    }
    .write_to(Vec::new())
    .is_err());
}

/// Protects rejection of malformed expanded names and reserved namespace uses.
#[test]
fn writer_rejects_invalid_expanded_names_and_reserved_namespaces() {
    let invalid_names = [
        unqualified("bad:name"),
        expanded("", "item", ""),
        expanded("http://www.w3.org/2000/xmlns/", "item", "xmlns"),
    ];
    for name in invalid_names {
        let entry = Entry {
            extensions: vec![Extension {
                name,
                attributes: vec![],
                content: vec![],
            }],
            ..Entry::default()
        };
        assert!(entry.write_to(Vec::new()).is_err());
    }
    let entry = Entry {
        extensions: vec![Extension {
            name: expanded("urn:extension", "item", "e"),
            attributes: vec![ExtensionAttribute {
                name: unqualified("xmlns"),
                value: "urn:bad".into(),
            }],
            content: vec![],
        }],
        ..Entry::default()
    };
    assert!(entry.write_to(Vec::new()).is_err());
}

/// Protects the required `xml` prefix when its serialization hint is wrong.
#[test]
fn writer_repairs_xml_namespace_prefix_hints() {
    let entry = Entry {
        extensions: vec![Extension {
            name: ExpandedName {
                namespace_uri: Some("http://www.w3.org/XML/1998/namespace".into()),
                local_name: "lang".into(),
                preferred_prefix: Some("wrong".into()),
            },
            attributes: vec![],
            content: vec![],
        }],
        ..Entry::default()
    };
    let written = entry.to_string();
    assert!(written.contains("<xml:lang"));
    assert_eq!(written.parse::<Entry>().unwrap(), entry);
}

/// Protects deterministic generated prefixes when a preferred prefix is invalid.
#[test]
fn invalid_preferred_prefixes_fall_back_to_deterministic_prefixes() {
    for prefix in ["-bad", "XMLThing"] {
        let extension = Extension {
            name: expanded("urn:extension", "item", prefix),
            attributes: vec![],
            content: vec![],
        };
        let entry = Entry {
            extensions: vec![extension],
            ..Entry::default()
        };
        let written = entry.to_string();
        assert!(written.contains("xmlns:ns0=\"urn:extension\""));
        assert!(!written.contains(&format!("xmlns:{prefix}=")));
        assert_eq!(written.parse::<Entry>().unwrap(), entry);
    }
}

/// Protects namespace declaration synthesis when names omit prefix hints.
#[test]
fn writer_synthesizes_valid_namespace_declarations() {
    let extension = Extension {
        name: ExpandedName {
            namespace_uri: Some("urn:extension".into()),
            local_name: "item".into(),
            preferred_prefix: None,
        },
        attributes: vec![ExtensionAttribute {
            name: ExpandedName {
                namespace_uri: Some("urn:attribute".into()),
                local_name: "value".into(),
                preferred_prefix: None,
            },
            value: "v".into(),
        }],
        content: vec![],
    };
    let entry = Entry {
        extensions: vec![extension],
        ..Entry::default()
    };
    let written = entry.to_string();
    assert!(written.contains("xmlns:ns0=\"urn:extension\""));
    assert_eq!(written.parse::<Entry>().unwrap(), entry);
}

/// Protects construction of the canonical extension model through generated builders.
#[cfg(feature = "builders")]
#[test]
fn builders_construct_the_canonical_extension_model() {
    let name = atom_syndication::extension::ExpandedNameBuilder::default()
        .namespace_uri(Some("urn:x".to_string()))
        .local_name("item")
        .preferred_prefix(Some("x".to_string()))
        .build();
    let attribute = atom_syndication::extension::ExtensionAttributeBuilder::default()
        .name(
            atom_syndication::extension::ExpandedNameBuilder::default()
                .local_name("attribute")
                .build(),
        )
        .value("value")
        .build();
    let extension = atom_syndication::extension::ExtensionBuilder::default()
        .name(name)
        .attribute(attribute)
        .content_item(ExtensionContent::Text("text".into()))
        .build();
    assert_eq!(
        extension.content,
        vec![ExtensionContent::Text("text".into())]
    );
}

/// Protects Serde preservation of the canonical extension data shape.
#[cfg(feature = "with-serde")]
#[test]
fn serde_round_trips_canonical_extension_fields() {
    let extension = Extension {
        name: expanded("urn:x", "item", "x"),
        attributes: vec![ExtensionAttribute {
            name: expanded("urn:a", "attribute", "a"),
            value: "v".into(),
        }],
        content: vec![ExtensionContent::Text("text".into())],
    };
    let encoded = serde_json::to_string(&extension).unwrap();
    assert!(encoded.contains("namespace_uri"));
    assert_eq!(
        serde_json::from_str::<Extension>(&encoded).unwrap(),
        extension
    );
}
