use crate::error::Result;
use oxigraph::model::{BlankNode, GraphName, NamedNode, NamedOrBlankNode, Quad, Term};
use oxigraph::store::Store;
use std::collections::HashSet;

mod attachment;
pub(crate) mod json;
pub(crate) mod xml;

/// `within` reaches the path's first occurrence from the record, and is none for
/// the record itself or a value it carries directly. `kind` is the record kind the path
/// stands in where a survey asked for it, and `count` then counts the occurrences in that kind.
pub(crate) struct Occurrence {
    pub(crate) path: String,
    pub(crate) within: Option<String>,
    pub(crate) count: usize,
    pub(crate) kind: Option<String>,
}

pub(crate) struct Valued {
    pub(crate) path: String,
    pub(crate) value: String,
    pub(crate) within: Option<String>,
    pub(crate) count: usize,
}

/// One occurrence of a path in `carried`, each one apart: `within` as an occurrence's,
/// an attribute's ending in its own step. `empty` where it holds nothing but whitespace.
pub(crate) struct Standing {
    pub(crate) path: String,
    pub(crate) within: Option<String>,
    pub(crate) empty: bool,
}

/// `kinds` asks each occurrence to be told by the record kind it stands in.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Paths {
    Kept {
        valued: HashSet<String>,
        carried: HashSet<String>,
        kinds: bool,
    },
    Dropped,
}

impl Paths {
    /// The paths of a record and nothing held at them, each told by its record kind.
    pub(crate) fn surveyed() -> Self {
        Self::Kept {
            valued: HashSet::new(),
            carried: HashSet::new(),
            kinds: true,
        }
    }
}

/// Where an envelope of a JSON adapter writes a record's kind: `record` is the pointer to it from
/// the record, `contained` the path contained records stand at, and `kind` the pointer to the
/// kind of one of them from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kinds {
    pub(crate) record: String,
    pub(crate) contained: Option<String>,
    pub(crate) kind: String,
}

/// `text` is the record written out on its own, for a validator and an address
/// check that bring their own parser.
pub(crate) struct Unit {
    pub(crate) store: Store,
    pub(crate) text: String,
    selector: String,
    occurrences: Vec<Occurrence>,
    values: Vec<Valued>,
    standings: Vec<Standing>,
}

impl Unit {
    pub(crate) fn selector(&self) -> String {
        self.selector.clone()
    }

    pub(crate) fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    pub(crate) fn values(&self) -> &[Valued] {
        &self.values
    }

    pub(crate) fn standings(&self) -> &[Standing] {
        &self.standings
    }
}

/// What admits a document to an envelope: its document element's local name, or the
/// name of a member of its value and that member's value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Admission {
    pub(crate) root: Option<String>,
    pub(crate) value: Option<String>,
    /// A JSON envelope's `bridge:jsonPathOfEachRecord`.
    pub(crate) records: Option<String>,
    /// A JSON envelope's `bridge:pathOfRecordKindBelowContent`, where it declares one.
    pub(crate) kinds: Option<Kinds>,
    /// An XML envelope's `bridge:pathOfRecordKindBelowContent`, which a survey does not read.
    pub(crate) unread_kind_path: Option<String>,
}

/// How a document is split: by the adapter's `bridge:elementNameOfEachRecord`, or by
/// the path of the envelope it is read in, `named` or the one that admits it.
pub(crate) struct Reading<'a> {
    pub(crate) element: Option<&'a str>,
    /// The adapter's `bridge:elementNameBreakingText`.
    pub(crate) breaking: &'a [String],
    pub(crate) envelopes: Vec<&'a Admission>,
    pub(crate) named: Option<usize>,
}

/// The envelope named, or else the first that admits the document, one naming a
/// member's value before one naming none.
pub(crate) fn admitting(lift: &dyn Lift, reading: &Reading<'_>) -> Option<usize> {
    reading.named.or_else(|| {
        let admits = |(_, admission): &(usize, &&Admission)| lift.admits(admission);
        let valued = reading
            .envelopes
            .iter()
            .enumerate()
            .filter(|(_, admission)| admission.value.is_some())
            .find(admits);
        valued
            .or_else(|| reading.envelopes.iter().enumerate().find(admits))
            .map(|(index, _)| index)
    })
}

/// One document being lifted: its records one at a time, then the skeleton the
/// detect query reads.
pub(crate) trait Lift {
    fn next_unit(&mut self) -> Result<Option<Unit>>;

    /// Known of an XML document once its document element has been read.
    fn admits(&self, admission: &Admission) -> bool;

    /// Whether the text was a document of the syntax: JSON that parsed, or XML with a document element.
    fn has_document_element(&self) -> bool;

    /// `described` is the envelope's, for a document whose root was never read.
    fn document_selector(&self, described: Option<&str>) -> String;

    fn into_skeleton(self: Box<Self>) -> Result<Store>;
}

fn store_of(quads: Vec<Quad>) -> Result<Store> {
    let store = Store::new()?;
    store.extend(quads)?;
    Ok(store)
}

pub(crate) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub(crate) const FX: &str = "http://sparql.xyz/facade-x/ns/";
pub(crate) const XYZ: &str = "http://sparql.xyz/facade-x/data/";

/// RFC 3987's `iunreserved`.
fn iunreserved(character: char) -> bool {
    matches!(character,
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~'
        | '\u{A0}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFEF}'
        | '\u{10000}'..='\u{1FFFD}' | '\u{20000}'..='\u{2FFFD}' | '\u{30000}'..='\u{3FFFD}'
        | '\u{40000}'..='\u{4FFFD}' | '\u{50000}'..='\u{5FFFD}' | '\u{60000}'..='\u{6FFFD}'
        | '\u{70000}'..='\u{7FFFD}' | '\u{80000}'..='\u{8FFFD}' | '\u{90000}'..='\u{9FFFD}'
        | '\u{A0000}'..='\u{AFFFD}' | '\u{B0000}'..='\u{BFFFD}' | '\u{C0000}'..='\u{CFFFD}'
        | '\u{D0000}'..='\u{DFFFD}' | '\u{E1000}'..='\u{EFFFD}')
}

/// `%` is never `iunreserved`, so two names never land on one IRI.
pub(crate) fn name(namespace: &str, local: &str) -> Result<NamedNode> {
    let mut iri = String::with_capacity(namespace.len() + local.len());
    iri.push_str(namespace);
    for character in local.chars() {
        if iunreserved(character) {
            iri.push(character);
            continue;
        }
        let mut octets = [0; 4];
        for octet in character.encode_utf8(&mut octets).as_bytes() {
            iri.push_str(&format!("%{octet:02X}"));
        }
    }
    Ok(NamedNode::new(iri)?)
}

pub(crate) fn member(index: usize) -> Result<NamedNode> {
    Ok(NamedNode::new(format!("{RDF}_{index}"))?)
}

pub(crate) fn triple(subject: &BlankNode, predicate: NamedNode, object: impl Into<Term>) -> Quad {
    Quad::new(
        NamedOrBlankNode::from(subject.clone()),
        predicate,
        object,
        GraphName::DefaultGraph,
    )
}
