//! Namespace-aware Atom extensions.
//!
//! An extension name is identified by its expanded name: its namespace URI (if
//! any) and local name. The
//! [`preferred_prefix`](crate::extension::ExpandedName::preferred_prefix) field is
//! serialization metadata, excluded from semantic identity.
//!
//! When parsing, namespace declarations in scope are resolved into expanded
//! names. When writing, declarations needed by an extension tree are
//! synthesized, and a valid prefix is selected when a preferred one cannot be
//! used. Therefore an extension parse/write/parse round trip preserves
//! expanded names, attributes, and mixed-content order, but need not preserve
//! source prefixes or declaration placement.
//!
//! Malformed namespace use is rejected while reading. Writing also
//! returns an error for invalid names or duplicate attribute expanded names.
//!
//! # Examples
//!
//! ```
//! use atom_syndication::Entry;
//!
//! let xml = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>Example</title><x:rating xmlns:x="urn:example" x:stars="5"/></entry>"#;
//! let entry = Entry::read_from(xml.as_slice()).unwrap();
//! let reparsed: Entry = entry.to_string().parse().unwrap();
//!
//! assert_eq!(reparsed.extensions()[0].name.namespace_uri.as_deref(), Some("urn:example"));
//! assert_eq!(reparsed.extensions()[0].name.local_name, "rating");
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use crate::error::XmlError;
use crate::toxml::ToXml;

pub(crate) mod util;

/// An XML expanded name: a namespace URI plus a local name.
///
/// The semantic identity is `(namespace_uri, local_name)`; equality ignores
/// [`Self::preferred_prefix`]. Use `None` for an unqualified expanded name. A
/// preferred prefix is serialization metadata and may be changed
/// or omitted when XML is written.
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Default, Clone, Eq)]
#[cfg_attr(feature = "builders", derive(Builder))]
#[cfg_attr(
    feature = "builders",
    builder(
        setter(into),
        default,
        build_fn(name = "build_impl", private, error = "std::convert::Infallible")
    )
)]
pub struct ExpandedName {
    /// The namespace URI, if any.
    pub namespace_uri: Option<String>,
    /// The local part of the name.
    pub local_name: String,
    /// A serialization hint; it is not part of the expanded name.
    pub preferred_prefix: Option<String>,
}

impl PartialEq for ExpandedName {
    fn eq(&self, other: &Self) -> bool {
        self.namespace_uri == other.namespace_uri && self.local_name == other.local_name
    }
}

/// An attribute of an [`Extension`].
///
/// Attribute identity is its [`ExpandedName`]. An extension's attributes are
/// unordered and must have unique expanded names; duplicate names cause
/// serialization to return an error.
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "builders", derive(Builder))]
#[cfg_attr(
    feature = "builders",
    builder(
        setter(into),
        default,
        build_fn(name = "build_impl", private, error = "std::convert::Infallible")
    )
)]
pub struct ExtensionAttribute {
    /// The namespace-aware attribute name.
    pub name: ExpandedName,
    /// The attribute value.
    pub value: String,
}

/// Ordered mixed content within an [`Extension`].
///
/// Text and child elements are retained in document order. Consecutive text
/// items represent the same character data as their concatenation.
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionContent {
    /// Character data.
    Text(String),
    /// A child extension element. Within a foreign extension subtree, modeled
    /// child elements are extensions: regular Atom elements are not nested there.
    Element(Extension),
}

/// A namespace-aware extension element.
///
/// Its attributes are an unordered, expanded-name-unique collection, while
/// [`Self::content`] is ordered mixed content. Writers synthesize the namespace
/// declarations required by its names and descendants.
///
/// # Examples
///
/// ```
/// use atom_syndication::extension::{ExpandedName, Extension, ExtensionAttribute};
/// use atom_syndication::Entry;
///
/// let extension = Extension {
///     name: ExpandedName {
///         namespace_uri: Some("urn:example".into()),
///         local_name: "rating".into(),
///         preferred_prefix: Some("x".into()),
///     },
///     attributes: vec![ExtensionAttribute {
///         name: ExpandedName {
///             namespace_uri: Some("urn:example".into()),
///             local_name: "stars".into(),
///             preferred_prefix: Some("x".into()),
///         },
///         value: "5".into(),
///     }],
///     content: Vec::new(),
/// };
/// let mut entry = Entry::default();
/// entry.set_extensions(vec![extension]);
///
/// let reparsed: Entry = entry.to_string().parse().unwrap();
/// assert_eq!(reparsed.extensions()[0].name.local_name, "rating");
/// ```
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Default, Clone, Eq)]
#[cfg_attr(feature = "builders", derive(Builder))]
#[cfg_attr(
    feature = "builders",
    builder(
        setter(into),
        default,
        build_fn(name = "build_impl", private, error = "std::convert::Infallible")
    )
)]
pub struct Extension {
    /// The namespace-aware element name.
    pub name: ExpandedName,
    /// Attributes. Attribute order is not semantically meaningful.
    #[cfg_attr(feature = "builders", builder(setter(each = "attribute")))]
    pub attributes: Vec<ExtensionAttribute>,
    /// Text and child elements in document order.
    #[cfg_attr(feature = "builders", builder(setter(each = "content_item")))]
    pub content: Vec<ExtensionContent>,
}

impl Extension {
    /// Writes this extension using bindings inherited from its parent element.
    ///
    /// The inherited scope is cloned before this element's declarations are
    /// applied, so bindings synthesized here remain visible to descendants but
    /// cannot escape into siblings. Serialization preserves expanded names and
    /// mixed-content order, may choose different prefixes, and returns
    /// an error for invalid names or duplicate expanded attribute names.
    pub(crate) fn to_xml_with_scope<W: Write>(
        &self,
        writer: &mut Writer<W>,
        inherited: &util::NamespaceScope,
    ) -> Result<(), XmlError> {
        // A child may inherit declarations made here, but those bindings must
        // not mutate the parent's scope or leak to its sibling elements.
        let mut scope = inherited.clone();
        let mut declarations = BTreeMap::new();
        let mut used_prefixes = BTreeMap::new();
        let element_name = serialized_name(
            &self.name,
            false,
            &mut scope,
            &mut declarations,
            &mut used_prefixes,
        )?;
        let mut element = BytesStart::new(&element_name);
        let mut seen_attributes = HashSet::new();
        for attribute in &self.attributes {
            if !seen_attributes.insert((
                attribute.name.namespace_uri.as_deref(),
                attribute.name.local_name.as_str(),
            )) {
                return Err(util::invalid_extension("duplicate expanded attribute name"));
            }
            let name = serialized_name(
                &attribute.name,
                true,
                &mut scope,
                &mut declarations,
                &mut used_prefixes,
            )?;
            element.push_attribute((name.as_str(), attribute.value.as_str()));
        }

        // Names must be allocated first: only then is this start tag's complete
        // declaration set known, so emit synthesized bindings together.
        for (prefix, uri) in &declarations {
            match prefix.as_deref() {
                Some(prefix) => {
                    element.push_attribute((format!("xmlns:{prefix}").as_str(), uri.as_str()))
                }
                None => element.push_attribute(("xmlns", uri.as_str())),
            }
        }
        writer
            .write_event(Event::Start(element))
            .map_err(XmlError::new)?;

        for content in &self.content {
            match content {
                ExtensionContent::Text(text) => writer
                    .write_event(Event::Text(BytesText::new(text)))
                    .map_err(XmlError::new)?,
                ExtensionContent::Element(element) => element.to_xml_with_scope(writer, &scope)?,
            }
        }

        writer
            .write_event(Event::End(BytesEnd::new(&element_name)))
            .map_err(XmlError::new)
    }
}

impl PartialEq for Extension {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && attributes_equal(&self.attributes, &other.attributes)
            && content_equal(&self.content, &other.content)
    }
}

/// Compares ordered mixed content by expanded element identity and character data.
///
/// Adjacent text nodes are semantically equivalent to their concatenation, so
/// this compares their Unicode scalar values across node boundaries without
/// allocating concatenated strings.
fn content_equal(left: &[ExtensionContent], right: &[ExtensionContent]) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        match (&left[left_index], &right[right_index]) {
            (ExtensionContent::Element(left), ExtensionContent::Element(right))
                if left == right =>
            {
                left_index += 1;
                right_index += 1;
            }
            (ExtensionContent::Text(_), ExtensionContent::Text(_)) => {
                // Walk scalar values directly so equivalent adjacent text nodes
                // compare equal without allocating concatenated text.
                let (mut left_offset, mut right_offset) = (0, 0);
                loop {
                    while matches!(left.get(left_index), Some(ExtensionContent::Text(text)) if left_offset == text.len())
                    {
                        left_index += 1;
                        left_offset = 0;
                    }
                    while matches!(right.get(right_index), Some(ExtensionContent::Text(text)) if right_offset == text.len())
                    {
                        right_index += 1;
                        right_offset = 0;
                    }
                    let (
                        Some(ExtensionContent::Text(left_text)),
                        Some(ExtensionContent::Text(right_text)),
                    ) = (left.get(left_index), right.get(right_index))
                    else {
                        break;
                    };
                    let Some(left_character) = left_text[left_offset..].chars().next() else {
                        unreachable!("text offset is checked before slicing")
                    };
                    let Some(right_character) = right_text[right_offset..].chars().next() else {
                        unreachable!("text offset is checked before slicing")
                    };
                    if left_character != right_character {
                        return false;
                    }
                    left_offset += left_character.len_utf8();
                    right_offset += right_character.len_utf8();
                }
                if matches!(left.get(left_index), Some(ExtensionContent::Text(_)))
                    || matches!(right.get(right_index), Some(ExtensionContent::Text(_)))
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    left_index == left.len() && right_index == right.len()
}

/// Compares attributes as an unordered multiset of expanded names and values.
///
/// The multiplicity check also gives malformed, duplicate-containing values a
/// deterministic equality result even though valid serialized extensions reject
/// duplicate expanded attribute names.
fn attributes_equal(left: &[ExtensionAttribute], right: &[ExtensionAttribute]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut counts: HashMap<_, usize> = HashMap::with_capacity(left.len());
    for attribute in left {
        *counts
            .entry((
                attribute.name.namespace_uri.as_deref(),
                attribute.name.local_name.as_str(),
                attribute.value.as_str(),
            ))
            .or_default() += 1;
    }

    right.iter().all(|attribute| {
        let key = (
            attribute.name.namespace_uri.as_deref(),
            attribute.name.local_name.as_str(),
            attribute.value.as_str(),
        );
        match counts.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        }
    })
}

/// Chooses a legal serialized XML name for `name` in this start tag's namespace scope.
///
/// Element names may use or clear the default namespace; attributes must use a
/// prefix when namespaced. `declarations` records bindings to emit on this tag,
/// while `used_prefixes` prevents a prefix already assigned on the tag from
/// being rebound to a different URI.
fn serialized_name(
    name: &ExpandedName,
    attribute: bool,
    scope: &mut util::NamespaceScope,
    declarations: &mut BTreeMap<Option<String>, String>,
    used_prefixes: &mut BTreeMap<String, String>,
) -> Result<String, XmlError> {
    validate_serialized_name(name, attribute)?;
    let Some(uri) = name.namespace_uri.as_ref() else {
        if !attribute && scope.get(&None).is_some() {
            scope.remove(&None);
            declarations.insert(None, String::new());
        }
        return Ok(name.local_name.clone());
    };

    if uri == "http://www.w3.org/XML/1998/namespace" {
        return Ok(format!("xml:{}", name.local_name));
    }

    // A prefix used earlier on this start tag cannot be rebound: every name
    // there must resolve against the one declaration emitted for that prefix.
    let reusable = |prefix: &str| {
        used_prefixes
            .get(prefix)
            .is_none_or(|used_uri| used_uri == uri)
    };
    let prefix = name
        .preferred_prefix
        .as_deref()
        .filter(|prefix| {
            valid_prefix(prefix) && *prefix != "xml" && *prefix != "xmlns" && reusable(prefix)
        })
        .map(str::to_owned)
        .or_else(|| {
            scope.iter().find_map(|(prefix, bound)| {
                (prefix.is_some() && bound == uri && reusable(prefix.as_ref().unwrap()))
                    .then(|| prefix.clone().unwrap())
            })
        })
        .unwrap_or_else(|| next_prefix(scope, used_prefixes));

    used_prefixes.insert(prefix.clone(), uri.clone());
    if scope.get(&Some(prefix.clone())) != Some(uri) {
        scope.insert(Some(prefix.clone()), uri.clone());
        declarations.insert(Some(prefix.clone()), uri.clone());
    }
    Ok(format!("{prefix}:{}", name.local_name))
}

/// Rejects names that cannot be represented by this namespace-aware writer.
///
/// Local names must be NCNames, namespace URIs cannot be empty or the reserved
/// `xmlns` URI, and an unqualified `xmlns` attribute cannot masquerade as a
/// declaration synthesized by the writer.
fn validate_serialized_name(name: &ExpandedName, attribute: bool) -> Result<(), XmlError> {
    if !util::valid_ncname(&name.local_name) {
        return Err(util::invalid_extension("invalid XML NCName"));
    }
    match name.namespace_uri.as_deref() {
        Some("") => return Err(util::invalid_extension("namespace URI cannot be empty")),
        Some(util::XMLNS_NAMESPACE) => {
            return Err(util::invalid_extension("reserved xmlns namespace URI"))
        }
        _ => {}
    }
    if attribute && name.namespace_uri.is_none() && name.local_name == "xmlns" {
        return Err(util::invalid_extension(
            "namespace declaration attribute name",
        ));
    }
    Ok(())
}

/// Returns whether `prefix` is a writer-generated-prefix candidate.
///
/// Generated prefixes intentionally use the ASCII NCName subset and exclude
/// case-insensitive `xml*` names, which XML reserves.
fn valid_prefix(prefix: &str) -> bool {
    let mut characters = prefix.chars();
    matches!(characters.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric()
                || character == '_'
                || character == '-'
                || character == '.'
        })
        && !prefix[..prefix.len().min(3)].eq_ignore_ascii_case("xml")
}

/// Finds the first generated prefix unused by both the inherited scope and this tag.
fn next_prefix(scope: &util::NamespaceScope, used_prefixes: &BTreeMap<String, String>) -> String {
    (0..)
        .map(|index| format!("ns{index}"))
        .find(|prefix| {
            !scope.contains_key(&Some(prefix.clone())) && !used_prefixes.contains_key(prefix)
        })
        .expect("unbounded namespace prefix iterator")
}

impl ToXml for Extension {
    fn to_xml<W: Write>(&self, writer: &mut Writer<W>) -> Result<(), XmlError> {
        self.to_xml_with_scope(writer, &util::NamespaceScope::default())
    }
}

#[cfg(feature = "builders")]
impl ExpandedNameBuilder {
    /// Builds a new `ExpandedName`.
    pub fn build(&self) -> ExpandedName {
        self.build_impl().unwrap()
    }
}

#[cfg(feature = "builders")]
impl ExtensionAttributeBuilder {
    /// Builds a new `ExtensionAttribute`.
    pub fn build(&self) -> ExtensionAttribute {
        self.build_impl().unwrap()
    }
}

#[cfg(feature = "builders")]
impl ExtensionBuilder {
    /// Builds a new `Extension`.
    pub fn build(&self) -> Extension {
        self.build_impl().unwrap()
    }
}
