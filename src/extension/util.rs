use std::collections::{BTreeMap, HashSet};
use std::io::BufRead;

use quick_xml::{
    escape::resolve_predefined_entity,
    events::{attributes::Attributes, BytesStart, Event},
    Reader,
};

use crate::error::{Error, XmlError};
use crate::extension::{ExpandedName, Extension, ExtensionAttribute, ExtensionContent};
use crate::util::{attr_value, decode};

/// Prefix-to-URI bindings visible at one XML element.
///
/// `None` is the default namespace. Scopes are built by inheriting parent
/// bindings and applying declarations on the current start tag.
pub(crate) type NamespaceScope = BTreeMap<Option<String>, String>;

/// The namespace URI bound exclusively to the `xml` prefix.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
/// The reserved namespace URI used only to spell namespace declarations.
pub(crate) const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// Error payload for XML namespace and extension invariants.
///
/// It is wrapped in [`XmlError`] so malformed namespace input is reported
/// through the crate's existing XML error contract.
#[derive(Debug)]
struct NamespaceError(String);

impl std::fmt::Display for NamespaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NamespaceError {}

/// Applies one `xmlns` declaration to the scope for its declaring element.
///
/// Empty URIs only undeclare the default namespace. Reserved prefixes and URIs
/// are rejected so later expanded-name resolution has XML's required bindings.
fn apply_namespace_declaration(
    scope: &mut NamespaceScope,
    prefix: Option<&str>,
    uri: String,
) -> Result<(), Error> {
    if prefix.is_some_and(|prefix| {
        !valid_ncname(prefix)
            || (prefix != "xml" && prefix.len() >= 3 && prefix[..3].eq_ignore_ascii_case("xml"))
    }) {
        return malformed("invalid or reserved namespace prefix");
    }
    if prefix == Some("xmlns") || uri == XMLNS_NAMESPACE {
        return malformed("reserved xmlns namespace binding");
    }
    if prefix == Some("xml") && uri != XML_NAMESPACE {
        return malformed("xml prefix must use the XML namespace");
    }
    if uri == XML_NAMESPACE && prefix != Some("xml") {
        return malformed("XML namespace must use the xml prefix");
    }
    let key = prefix.map(str::to_owned);
    if uri.is_empty() {
        if prefix.is_some() {
            return malformed("prefixed namespace declarations cannot be empty");
        }
        scope.remove(&key);
    } else {
        scope.insert(key, uri);
    }
    Ok(())
}

/// Returns whether `name` is an XML NCName accepted by namespace declarations.
///
/// This implements XML's Unicode start/continuation character classes and
/// rejects empty names and names containing colons.
pub(crate) fn valid_ncname(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(character) if ncname_start(character))
        && characters.all(ncname_char)
}

/// Returns whether a character may begin an XML NCName.
fn ncname_start(character: char) -> bool {
    character == '_'
        || character.is_ascii_alphabetic()
        || matches!(character as u32, 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

/// Returns whether a character may continue an XML NCName.
fn ncname_char(character: char) -> bool {
    ncname_start(character)
        || character.is_ascii_digit()
        || matches!(character as u32, 0x2D | 0x2E | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

/// Constructs this element's scope from inherited bindings and a raw attribute stream.
///
/// Namespace declaration attributes are consumed into the returned scope; all
/// other attributes are ignored. The required `xml` binding is installed even
/// when absent from the document.
pub(crate) fn namespace_scope_from_attributes<R: BufRead>(
    reader: &Reader<R>,
    mut attributes: Attributes<'_>,
    inherited: &NamespaceScope,
) -> Result<NamespaceScope, Error> {
    let mut scope = inherited.clone();
    scope.insert(Some("xml".to_string()), XML_NAMESPACE.to_string());
    for attribute in attributes.with_checks(false) {
        let attribute = attribute.map_err(XmlError::new)?;
        let key = decode(attribute.key.as_ref(), reader)?;
        let value = attr_value(&attribute, reader)?.to_string();
        if key == "xmlns" {
            apply_namespace_declaration(&mut scope, None, value)?;
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            apply_namespace_declaration(&mut scope, Some(prefix), value)?;
        }
    }
    Ok(scope)
}

/// Constructs this start tag's scope from its inherited bindings and declarations.
///
/// The returned scope owns the current element's declarations and always
/// includes the required `xml` binding.
pub(crate) fn namespace_scope<R: BufRead>(
    reader: &Reader<R>,
    element: &BytesStart<'_>,
    inherited: &NamespaceScope,
) -> Result<NamespaceScope, Error> {
    let mut scope = inherited.clone();
    scope.insert(Some("xml".to_string()), XML_NAMESPACE.to_string());
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(XmlError::new)?;
        let key = decode(attribute.key.as_ref(), reader)?;
        let value = attr_value(&attribute, reader)?.to_string();
        if key == "xmlns" {
            apply_namespace_declaration(&mut scope, None, value)?;
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            apply_namespace_declaration(&mut scope, Some(prefix), value)?;
        }
    }
    Ok(scope)
}

/// Returns whether `element` must be retained as an extension rather than sent to Atom dispatch.
///
/// Classification resolves the name after its own declarations apply, so
/// malformed or unbound prefixes remain errors. Any explicitly prefixed name is
/// an extension—even when it resolves to Atom's URI—to preserve the legacy
/// parser contract. Only unprefixed Atom names use core-field dispatch.
pub(crate) fn is_extension<R: BufRead>(
    reader: &Reader<R>,
    element: &BytesStart<'_>,
    scope: &NamespaceScope,
) -> Result<bool, Error> {
    let scoped = namespace_scope(reader, element, scope)?;
    let name = expanded_name(reader, element.name().as_ref(), &scoped, false)?;
    Ok(name.preferred_prefix.is_some()
        || name.namespace_uri.as_deref() != Some("http://www.w3.org/2005/Atom"))
}

/// Parses one extension child and appends it in document order.
///
/// This includes both non-Atom names and explicitly prefixed Atom names. The
/// latter must not be redirected into core Atom field dispatch.
/// `inherited` is the parent's scope; declarations on `element` are applied by
/// the recursive parser before it resolves the extension's expanded names.
pub(crate) fn parse_extension<R: BufRead>(
    reader: &mut Reader<R>,
    element: &BytesStart<'_>,
    inherited: &NamespaceScope,
    extensions: &mut Vec<Extension>,
) -> Result<(), Error> {
    extensions.push(parse_extension_element(reader, element, inherited)?);
    Ok(())
}

/// Parses an extension subtree with expanded names and ordered mixed content.
///
/// Each recursive call creates the child scope from its declaring start tag,
/// rejects duplicate expanded attribute names, and preserves text/element
/// order. XML read and name-resolution failures propagate unchanged.
fn parse_extension_element<R: BufRead>(
    reader: &mut Reader<R>,
    element: &BytesStart<'_>,
    inherited: &NamespaceScope,
) -> Result<Extension, Error> {
    // Child declarations shadow only inside this subtree, so derive its scope
    // before resolving the element and every attribute name.
    let scope = namespace_scope(reader, element, inherited)?;
    let name = expanded_name(reader, element.name().as_ref(), &scope, false)?;
    let mut attributes = Vec::new();
    let mut seen = HashSet::new();

    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(XmlError::new)?;
        let raw_name = decode(attribute.key.as_ref(), reader)?;
        if raw_name == "xmlns" || raw_name.starts_with("xmlns:") {
            continue;
        }
        // XML permits distinct prefixes for the same expanded name, but an
        // element may not carry that expanded attribute more than once.
        let name = expanded_name(reader, attribute.key.as_ref(), &scope, true)?;
        if !seen.insert((name.namespace_uri.clone(), name.local_name.clone())) {
            return malformed("duplicate expanded attribute name");
        }
        attributes.push(ExtensionAttribute {
            name,
            value: attr_value(&attribute, reader)?.to_string(),
        });
    }

    let mut content = Vec::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(XmlError::new)? {
            Event::Start(child) => content.push(ExtensionContent::Element(
                parse_extension_element(reader, &child, &scope)?,
            )),
            Event::Text(text) => push_text(
                &mut content,
                text.decode().map_err(XmlError::new)?.into_owned(),
            ),
            Event::CData(text) => {
                content.push(ExtensionContent::Text(decode(&text, reader)?.into_owned()))
            }
            Event::GeneralRef(reference) => {
                let entity = reference.decode().map_err(XmlError::new)?;
                let value = if let Some(resolved) = resolve_predefined_entity(&entity) {
                    resolved.to_string()
                } else if let Some(character) =
                    reference.resolve_char_ref().map_err(XmlError::new)?
                {
                    character.to_string()
                } else {
                    format!("&{entity};")
                };
                push_text(&mut content, value);
            }
            Event::End(_) => break,
            Event::Eof => return Err(Error::Eof),
            _ => {}
        }
        buffer.clear();
    }

    Ok(Extension {
        name,
        attributes,
        content,
    })
}

/// Resolves a serialized XML name to its expanded name in `scope`.
///
/// Element names inherit the default namespace; unprefixed attributes do not.
/// The source prefix is retained only as a serialization preference. Invalid
/// prefixes or local names, multiple-colon names, and unbound prefixes return
/// XML errors.
pub(crate) fn expanded_name<R: BufRead>(
    reader: &Reader<R>,
    raw_name: &[u8],
    scope: &NamespaceScope,
    attribute: bool,
) -> Result<ExpandedName, Error> {
    let raw_name = decode(raw_name, reader)?;
    let (prefix, local_name) = match raw_name.split_once(':') {
        Some((prefix, local_name))
            if !prefix.is_empty() && !local_name.is_empty() && !local_name.contains(':') =>
        {
            (Some(prefix), local_name)
        }
        Some(_) => return malformed("malformed XML name"),
        None => (None, raw_name.as_ref()),
    };
    if !valid_ncname(local_name)
        || prefix.is_some_and(|prefix| !valid_ncname(prefix) || prefix == "xmlns")
    {
        return malformed("invalid XML prefix or local name");
    }
    let namespace_uri = match prefix {
        Some(prefix) => scope
            .get(&Some(prefix.to_string()))
            .cloned()
            .ok_or_else(|| {
                Error::Xml(XmlError::new(NamespaceError(format!(
                    "unbound prefix '{prefix}'"
                ))))
            })
            .map(Some)?,
        None if attribute => None,
        None => scope.get(&None).cloned(),
    };
    Ok(ExpandedName {
        namespace_uri,
        local_name: local_name.to_string(),
        preferred_prefix: prefix.map(str::to_string),
    })
}

/// Appends character data while coalescing it with preceding text content.
///
/// Entity and text events may split one logical text run; coalescing preserves
/// its semantic value and keeps extension mixed content compact.
fn push_text(content: &mut Vec<ExtensionContent>, text: String) {
    if let Some(ExtensionContent::Text(previous)) = content.last_mut() {
        previous.push_str(&text);
    } else {
        content.push(ExtensionContent::Text(text));
    }
}

/// Builds the crate-standard XML error used for namespace extension violations.
pub(crate) fn invalid_extension(message: &str) -> XmlError {
    XmlError::new(NamespaceError(message.to_string()))
}

/// Returns a namespace-invariant violation through the XML parse error channel.
fn malformed<T>(message: &str) -> Result<T, Error> {
    Err(Error::Xml(invalid_extension(message)))
}
