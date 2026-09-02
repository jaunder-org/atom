use std::borrow::Cow;
use std::io::{BufRead, Write};
use std::str::{self, FromStr};

use quick_xml::events::attributes::Attributes;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::category::Category;
use crate::entry::Entry;
use crate::error::{Error, XmlError};
use crate::extension::util::{
    is_extension, namespace_scope_from_attributes, parse_extension, NamespaceScope,
};
use crate::extension::Extension;
use crate::fromxml::FromXml;
use crate::generator::Generator;
use crate::link::Link;
use crate::person::Person;
use crate::text::Text;
use crate::toxml::{ToXml, WriterExt};
use crate::util::{
    atom_datetime, atom_text, attr_value, decode, default_fixed_datetime, skip, FixedDateTime,
};

/// Various options which control XML writer
#[derive(Clone, Copy)]
pub struct WriteConfig {
    /// Write XML document declaration at the beginning of a document. Default is `true`.
    pub write_document_declaration: bool,
    /// Indent XML tags. Default is `None`.
    pub indent_size: Option<usize>,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            write_document_declaration: true,
            indent_size: None,
        }
    }
}

/// Represents an Atom feed
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "builders", derive(Builder))]
#[cfg_attr(
    feature = "builders",
    builder(
        setter(into),
        default,
        build_fn(name = "build_impl", private, error = "std::convert::Infallible")
    )
)]
pub struct Feed {
    /// A human-readable title for the feed.
    pub title: Text,
    /// A universally unique and permanent URI.
    pub id: String,
    /// The last time the feed was modified in a significant way.
    pub updated: FixedDateTime,
    /// The authors of the feed.
    #[cfg_attr(feature = "builders", builder(setter(each = "author")))]
    pub authors: Vec<Person>,
    /// The categories that the feed belongs to.
    #[cfg_attr(feature = "builders", builder(setter(each = "category")))]
    pub categories: Vec<Category>,
    /// The contributors to the feed.
    #[cfg_attr(feature = "builders", builder(setter(each = "contributor")))]
    pub contributors: Vec<Person>,
    /// The software used to generate the feed.
    pub generator: Option<Generator>,
    /// A small image which provides visual identification for the feed.
    pub icon: Option<String>,
    /// The Web pages related to the feed.
    #[cfg_attr(feature = "builders", builder(setter(each = "link")))]
    pub links: Vec<Link>,
    /// A larger image which provides visual identification for the feed.
    pub logo: Option<String>,
    /// Information about rights held in and over the feed.
    pub rights: Option<Text>,
    /// A human-readable description or subtitle for the feed.
    pub subtitle: Option<Text>,
    /// The entries contained in the feed.
    #[cfg_attr(feature = "builders", builder(setter(each = "entry")))]
    pub entries: Vec<Entry>,
    /// The extensions for the feed.
    #[cfg_attr(feature = "builders", builder(setter(each = "extension")))]
    pub extensions: Vec<Extension>,
    /// Base URL for resolving any relative references found in the element.
    pub base: Option<String>,
    /// Indicates the natural language for the element.
    pub lang: Option<String>,
}

impl Feed {
    /// Attempt to read an Atom feed from the reader.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io::BufReader;
    /// use std::fs::File;
    /// use atom_syndication::Feed;
    ///
    /// let file = File::open("example.xml").unwrap();
    /// let feed = Feed::read_from(BufReader::new(file)).unwrap();
    /// ```
    pub fn read_from<B: BufRead>(reader: B) -> Result<Feed, Error> {
        let mut reader = Reader::from_reader(reader);
        reader.config_mut().expand_empty_elements = true;

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf).map_err(XmlError::new)? {
                Event::Start(element) => {
                    if decode(element.name().as_ref(), &reader)? == "feed" {
                        return Feed::from_xml(&mut reader, element.attributes());
                    } else {
                        return Err(Error::InvalidStartTag);
                    }
                }
                Event::Eof => break,
                _ => {}
            }

            buf.clear();
        }

        Err(Error::Eof)
    }

    /// Attempt to write this Atom feed to a writer using default `WriteConfig`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::BufReader;
    /// use std::fs::File;
    /// use atom_syndication::Feed;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let feed = Feed {
    ///     title: "Feed Title".into(),
    ///     id: "Feed ID".into(),
    ///     ..Default::default()
    /// };
    ///
    /// let out = feed.write_to(Vec::new())?;
    /// assert_eq!(&out, br#"<?xml version="1.0"?>
    /// <feed xmlns="http://www.w3.org/2005/Atom"><title>Feed Title</title><id>Feed ID</id><updated>1970-01-01T00:00:00+00:00</updated></feed>"#);
    /// # Ok(()) }
    /// ```
    pub fn write_to<W: Write>(&self, writer: W) -> Result<W, Error> {
        self.write_with_config(writer, WriteConfig::default())
    }

    /// Attempt to write this Atom feed to a writer.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::BufReader;
    /// use std::fs::File;
    /// use atom_syndication::{Feed, WriteConfig};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let feed = Feed {
    ///     title: "Feed Title".into(),
    ///     id: "Feed ID".into(),
    ///     ..Default::default()
    /// };
    ///
    /// let mut out = Vec::new();
    /// let config = WriteConfig {
    ///     write_document_declaration: false,
    ///     indent_size: Some(2),
    /// };
    /// feed.write_with_config(&mut out, config)?;
    /// assert_eq!(&out, br#"<feed xmlns="http://www.w3.org/2005/Atom">
    ///   <title>Feed Title</title>
    ///   <id>Feed ID</id>
    ///   <updated>1970-01-01T00:00:00+00:00</updated>
    /// </feed>"#);
    /// # Ok(()) }
    /// ```
    pub fn write_with_config<W: Write>(
        &self,
        writer: W,
        write_config: WriteConfig,
    ) -> Result<W, Error> {
        let mut writer = match write_config.indent_size {
            Some(indent_size) => Writer::new_with_indent(writer, b' ', indent_size),
            None => Writer::new(writer),
        };
        if write_config.write_document_declaration {
            writer
                .write_event(Event::Decl(BytesDecl::new("1.0", None, None)))
                .map_err(XmlError::new)?;
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n")))
                .map_err(XmlError::new)?;
        }
        self.to_xml(&mut writer)?;
        Ok(writer.into_inner())
    }

    /// Return the title of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_title("Feed Title");
    /// assert_eq!(feed.title(), "Feed Title");
    /// ```
    pub fn title(&self) -> &Text {
        &self.title
    }

    /// Set the title of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_title("Feed Title");
    /// ```
    pub fn set_title<V>(&mut self, title: V)
    where
        V: Into<Text>,
    {
        self.title = title.into();
    }

    /// Return the unique URI of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_id("urn:uuid:60a76c80-d399-11d9-b91C-0003939e0af6");
    /// assert_eq!(feed.id(), "urn:uuid:60a76c80-d399-11d9-b91C-0003939e0af6");
    /// ```
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// Set the unique URI of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_id("urn:uuid:60a76c80-d399-11d9-b91C-0003939e0af6");
    /// ```
    pub fn set_id<V>(&mut self, id: V)
    where
        V: Into<String>,
    {
        self.id = id.into();
    }

    /// Return the last time that this feed was modified.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    /// use atom_syndication::FixedDateTime;
    /// use std::str::FromStr;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_updated(FixedDateTime::from_str("2017-06-03T15:15:44-05:00").unwrap());
    /// assert_eq!(feed.updated().to_rfc3339(), "2017-06-03T15:15:44-05:00");
    /// ```
    pub fn updated(&self) -> &FixedDateTime {
        &self.updated
    }

    /// Set the last time that this feed was modified.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    /// use atom_syndication::FixedDateTime;
    /// use std::str::FromStr;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_updated(FixedDateTime::from_str("2017-06-03T15:15:44-05:00").unwrap());
    /// ```
    pub fn set_updated<V>(&mut self, updated: V)
    where
        V: Into<FixedDateTime>,
    {
        self.updated = updated.into();
    }

    /// Return the authors of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Person};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_authors(vec![Person::default()]);
    /// assert_eq!(feed.authors().len(), 1);
    /// ```
    pub fn authors(&self) -> &[Person] {
        self.authors.as_slice()
    }

    /// Set the authors of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Person};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_authors(vec![Person::default()]);
    /// ```
    pub fn set_authors<V>(&mut self, authors: V)
    where
        V: Into<Vec<Person>>,
    {
        self.authors = authors.into();
    }

    /// Return the categories this feed belongs to.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Category};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_categories(vec![Category::default()]);
    /// assert_eq!(feed.categories().len(), 1);
    /// ```
    pub fn categories(&self) -> &[Category] {
        self.categories.as_slice()
    }

    /// Set the categories this feed belongs to.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Category};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_categories(vec![Category::default()]);
    /// ```
    pub fn set_categories<V>(&mut self, categories: V)
    where
        V: Into<Vec<Category>>,
    {
        self.categories = categories.into();
    }

    /// Return the contributors to this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Person};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_contributors(vec![Person::default()]);
    /// assert_eq!(feed.contributors().len(), 1);
    /// ```
    pub fn contributors(&self) -> &[Person] {
        self.contributors.as_slice()
    }

    /// Set the contributors to this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Person};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_contributors(vec![Person::default()]);
    /// ```
    pub fn set_contributors<V>(&mut self, contributors: V)
    where
        V: Into<Vec<Person>>,
    {
        self.contributors = contributors.into();
    }

    /// Return the name of the software used to generate this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Generator};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_generator(Generator::default());
    /// assert!(feed.generator().is_some());
    /// ```
    pub fn generator(&self) -> Option<&Generator> {
        self.generator.as_ref()
    }

    /// Set the name of the software used to generate this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Generator};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_generator(Generator::default());
    /// ```
    pub fn set_generator<V>(&mut self, generator: V)
    where
        V: Into<Option<Generator>>,
    {
        self.generator = generator.into()
    }

    /// Return the icon for this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_icon("http://example.com/icon.png".to_string());
    /// assert_eq!(feed.icon(), Some("http://example.com/icon.png"));
    /// ```
    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    /// Set the icon for this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_icon("http://example.com/icon.png".to_string());
    /// ```
    pub fn set_icon<V>(&mut self, icon: V)
    where
        V: Into<Option<String>>,
    {
        self.icon = icon.into()
    }

    /// Return the Web pages related to this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Link};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_links(vec![Link::default()]);
    /// assert_eq!(feed.links().len(), 1);
    /// ```
    pub fn links(&self) -> &[Link] {
        self.links.as_slice()
    }

    /// Set the Web pages related to this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Link};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_links(vec![Link::default()]);
    /// ```
    pub fn set_links<V>(&mut self, links: V)
    where
        V: Into<Vec<Link>>,
    {
        self.links = links.into();
    }

    /// Return the logo for this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_logo("http://example.com/logo.png".to_string());
    /// assert_eq!(feed.logo(), Some("http://example.com/logo.png"));
    /// ```
    pub fn logo(&self) -> Option<&str> {
        self.logo.as_deref()
    }

    /// Set the logo for this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_logo("http://example.com/logo.png".to_string());
    /// ```
    pub fn set_logo<V>(&mut self, logo: V)
    where
        V: Into<Option<String>>,
    {
        self.logo = logo.into()
    }

    /// Return the information about the rights held in and over this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Text};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_rights(Text::from("© 2017 John Doe"));
    /// assert_eq!(feed.rights().map(Text::as_str), Some("© 2017 John Doe"));
    /// ```
    pub fn rights(&self) -> Option<&Text> {
        self.rights.as_ref()
    }

    /// Set the information about the rights held in and over this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Text};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_rights(Text::from("© 2017 John Doe"));
    /// ```
    pub fn set_rights<V>(&mut self, rights: V)
    where
        V: Into<Option<Text>>,
    {
        self.rights = rights.into()
    }

    /// Return the description or subtitle of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Text};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_subtitle(Text::from("Feed subtitle"));
    /// assert_eq!(feed.subtitle().map(Text::as_str), Some("Feed subtitle"));
    /// ```
    pub fn subtitle(&self) -> Option<&Text> {
        self.subtitle.as_ref()
    }

    /// Set the description or subtitle of this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Text};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_subtitle(Text::from("Feed subtitle"));
    /// ```
    pub fn set_subtitle<V>(&mut self, subtitle: V)
    where
        V: Into<Option<Text>>,
    {
        self.subtitle = subtitle.into()
    }

    /// Return the entries in this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Entry};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_entries(vec![Entry::default()]);
    /// assert_eq!(feed.entries().len(), 1);
    /// ```
    pub fn entries(&self) -> &[Entry] {
        self.entries.as_slice()
    }

    /// Set the entries in this feed.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::{Feed, Entry};
    ///
    /// let mut feed = Feed::default();
    /// feed.set_entries(vec![Entry::default()]);
    /// ```
    pub fn set_entries<V>(&mut self, entries: V)
    where
        V: Into<Vec<Entry>>,
    {
        self.entries = entries.into();
    }

    /// Return extensions directly attached to this feed.
    ///
    /// Feed extensions use the same namespace-aware [`Extension`] representation
    /// as entry extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use atom_syndication::extension::{ExpandedName, Extension};
    /// use atom_syndication::Feed;
    ///
    /// let mut feed = Feed::default();
    /// feed.set_extensions(vec![Extension {
    ///     name: ExpandedName {
    ///         namespace_uri: Some("urn:example".into()),
    ///         local_name: "rating".into(),
    ///         preferred_prefix: Some("x".into()),
    ///     },
    ///     attributes: Vec::new(),
    ///     content: Vec::new(),
    /// }]);
    ///
    /// assert_eq!(feed.extensions().len(), 1);
    /// ```
    pub fn extensions(&self) -> &[Extension] {
        &self.extensions
    }

    /// Replace extensions directly attached to this feed.
    ///
    /// [`Self::extensions`] exposes the replacement as a slice.
    pub fn set_extensions<V>(&mut self, extensions: V)
    where
        V: Into<Vec<Extension>>,
    {
        self.extensions = extensions.into()
    }

    /// Return base URL of the feed.
    pub fn base(&self) -> Option<&str> {
        self.base.as_deref()
    }

    /// Set base URL of the feed.
    pub fn set_base<V>(&mut self, base: V)
    where
        V: Into<Option<String>>,
    {
        self.base = base.into();
    }

    /// Return natural language of the feed.
    pub fn lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }

    /// Set the base URL of the feed.
    pub fn set_lang<V>(&mut self, lang: V)
    where
        V: Into<Option<String>>,
    {
        self.lang = lang.into();
    }
}

impl FromXml for Feed {
    fn from_xml<B: BufRead>(
        reader: &mut Reader<B>,
        mut atts: Attributes<'_>,
    ) -> Result<Self, Error> {
        let scope =
            namespace_scope_from_attributes(reader, atts.clone(), &NamespaceScope::default())?;
        let mut feed = Feed::default();
        let mut buf = Vec::new();

        for att in atts.with_checks(false).flatten() {
            match decode(att.key.as_ref(), reader)? {
                Cow::Borrowed("xml:base") => {
                    feed.base = Some(attr_value(&att, reader)?.to_string())
                }
                Cow::Borrowed("xml:lang") => {
                    feed.lang = Some(attr_value(&att, reader)?.to_string())
                }
                _ => {}
            }
        }

        loop {
            match reader.read_event_into(&mut buf).map_err(XmlError::new)? {
                Event::Start(element) => {
                    // Only unprefixed Atom names are core fields; foreign or
                    // prefixed `title` remains extension content by contract.
                    if is_extension(reader, &element, &scope)? {
                        parse_extension(reader, &element, &scope, &mut feed.extensions)?;
                    } else {
                        match decode(element.name().as_ref(), reader)? {
                            Cow::Borrowed("title") => {
                                feed.title = Text::from_xml(reader, element.attributes())?
                            }
                            Cow::Borrowed("id") => feed.id = atom_text(reader)?.unwrap_or_default(),
                            Cow::Borrowed("updated") => {
                                feed.updated =
                                    atom_datetime(reader)?.unwrap_or_else(default_fixed_datetime)
                            }
                            Cow::Borrowed("author") => feed.authors.push(
                                Person::from_xml_with_scope(reader, element.attributes(), &scope)?,
                            ),
                            Cow::Borrowed("category") => {
                                feed.categories.push(Category::from_xml(reader, &element)?);
                                skip(element.name(), reader)?;
                            }
                            Cow::Borrowed("contributor") => feed.contributors.push(
                                Person::from_xml_with_scope(reader, element.attributes(), &scope)?,
                            ),
                            Cow::Borrowed("generator") => {
                                feed.generator =
                                    Some(Generator::from_xml(reader, element.attributes())?)
                            }
                            Cow::Borrowed("icon") => feed.icon = atom_text(reader)?,
                            Cow::Borrowed("link") => {
                                feed.links.push(Link::from_xml(reader, &element)?);
                                skip(element.name(), reader)?;
                            }
                            Cow::Borrowed("logo") => feed.logo = atom_text(reader)?,
                            Cow::Borrowed("rights") => {
                                feed.rights = Some(Text::from_xml(reader, element.attributes())?)
                            }
                            Cow::Borrowed("subtitle") => {
                                feed.subtitle = Some(Text::from_xml(reader, element.attributes())?)
                            }
                            Cow::Borrowed("entry") => feed.entries.push(
                                Entry::from_xml_with_scope(reader, element.attributes(), &scope)?,
                            ),
                            _ => skip(element.name(), reader)?,
                        }
                    }
                }
                Event::End(_) => break,
                Event::Eof => return Err(Error::Eof),
                _ => {}
            }

            buf.clear();
        }

        Ok(feed)
    }
}

impl ToXml for Feed {
    fn to_xml<W: Write>(&self, writer: &mut Writer<W>) -> Result<(), XmlError> {
        let name = "feed";
        let mut element = BytesStart::new(name);
        element.push_attribute(("xmlns", "http://www.w3.org/2005/Atom"));

        if let Some(ref base) = self.base {
            element.push_attribute(("xml:base", base.as_str()));
        }

        if let Some(ref lang) = self.lang {
            element.push_attribute(("xml:lang", lang.as_str()));
        }

        writer
            .write_event(Event::Start(element))
            .map_err(XmlError::new)?;
        writer.write_object_named(&self.title, "title")?;
        writer.write_text_element("id", &self.id)?;
        writer.write_text_element("updated", &self.updated.to_rfc3339())?;
        writer.write_objects_named(&self.authors, "author")?;
        writer.write_objects(&self.categories)?;
        writer.write_objects_named(&self.contributors, "contributor")?;

        if let Some(ref generator) = self.generator {
            writer.write_object(generator)?;
        }

        if let Some(ref icon) = self.icon {
            writer.write_text_element("icon", icon)?;
        }

        writer.write_objects(&self.links)?;

        if let Some(ref logo) = self.logo {
            writer.write_text_element("logo", logo)?;
        }

        if let Some(ref rights) = self.rights {
            writer.write_object_named(rights, "rights")?;
        }

        if let Some(ref subtitle) = self.subtitle {
            writer.write_object_named(subtitle, "subtitle")?;
        }

        writer.write_objects(&self.entries)?;

        let scope = NamespaceScope::from([(None, "http://www.w3.org/2005/Atom".to_string())]);
        for extension in &self.extensions {
            extension.to_xml_with_scope(writer, &scope)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new(name)))
            .map_err(XmlError::new)?;

        Ok(())
    }
}

impl FromStr for Feed {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Feed::read_from(s.as_bytes())
    }
}

impl ToString for Feed {
    fn to_string(&self) -> String {
        let buf = self.write_to(Vec::new()).unwrap_or_default();
        // this unwrap should be safe since the bytes written from the Feed are all valid utf8
        String::from_utf8(buf).unwrap()
    }
}

impl Default for Feed {
    fn default() -> Self {
        Feed {
            title: Text::default(),
            id: String::new(),
            updated: default_fixed_datetime(),
            authors: Vec::new(),
            categories: Vec::new(),
            contributors: Vec::new(),
            generator: None,
            icon: None,
            links: Vec::new(),
            logo: None,
            rights: None,
            subtitle: None,
            entries: Vec::new(),
            extensions: Vec::new(),
            base: None,
            lang: None,
        }
    }
}

#[cfg(feature = "builders")]
impl FeedBuilder {
    /// Builds a new `Feed`.
    pub fn build(&self) -> Feed {
        self.build_impl().unwrap()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_default() {
        let feed = Feed::default();
        let xml_fragment = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title></title><id></id><updated>1970-01-01T00:00:00+00:00</updated></feed>"#;
        assert_eq!(feed.to_string(), xml_fragment);
        let loaded_feed = Feed::read_from(xml_fragment.as_bytes()).unwrap();
        assert_eq!(loaded_feed, feed);
        assert_eq!(loaded_feed.base(), None);
        assert_eq!(loaded_feed.lang(), None);
    }

    #[test]
    fn test_base_and_lang() {
        let mut feed = Feed::default();
        feed.set_base(Some("http://example.com/blog/".into()));
        feed.set_lang(Some("fr_FR".into()));
        let xml_fragment = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom" xml:base="http://example.com/blog/" xml:lang="fr_FR"><title></title><id></id><updated>1970-01-01T00:00:00+00:00</updated></feed>"#;
        assert_eq!(feed.to_string(), xml_fragment);
        let loaded_feed = Feed::read_from(xml_fragment.as_bytes()).unwrap();
        assert_eq!(loaded_feed, feed);
        assert_eq!(loaded_feed.base(), Some("http://example.com/blog/"));
        assert_eq!(loaded_feed.lang(), Some("fr_FR"));
    }

    #[test]
    fn test_write_no_decl() {
        let feed = Feed::default();
        let xml = feed
            .write_with_config(
                Vec::new(),
                WriteConfig {
                    write_document_declaration: false,
                    indent_size: None,
                },
            )
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&xml),
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><title></title><id></id><updated>1970-01-01T00:00:00+00:00</updated></feed>"#
        );
    }

    #[test]
    fn test_write_indented() {
        let feed = Feed::default();
        let xml = feed
            .write_with_config(
                Vec::new(),
                WriteConfig {
                    write_document_declaration: true,
                    indent_size: Some(4),
                },
            )
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&xml),
            r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
    <title></title>
    <id></id>
    <updated>1970-01-01T00:00:00+00:00</updated>
</feed>"#
        );
    }

    #[test]
    fn test_write_no_decl_indented() {
        let feed = Feed::default();
        let xml = feed
            .write_with_config(
                Vec::new(),
                WriteConfig {
                    write_document_declaration: false,
                    indent_size: Some(4),
                },
            )
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&xml),
            r#"<feed xmlns="http://www.w3.org/2005/Atom">
    <title></title>
    <id></id>
    <updated>1970-01-01T00:00:00+00:00</updated>
</feed>"#
        );
    }
}
