use atom_syndication::Entry;

#[test]
fn standalone_entry_retains_scoped_extension_namespace_declarations() {
    let xml = r#"<entry xmlns="http://www.w3.org/2005/Atom"><title>Entry</title><x:item xmlns:x="urn:example:extension">content</x:item></entry>"#;
    let entry = Entry::read_from(xml.as_bytes()).unwrap();

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
