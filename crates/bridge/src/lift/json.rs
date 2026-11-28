// The whole document is read before any record is handed over: which envelope admits
// it, and so which path splits it, is said by its value's members.
use super::attachment::{decoded, names_text, TEXT_DECODED_FROM_BASE64};
use super::{
    admitting, member, name, store_of, triple, Admission, Kinds, Occurrence, Paths, Reading,
    Standing, Unit, Valued, FX, RDF, XYZ,
};
use crate::error::{Error, Result};
use crate::json::{self, Node, Value};
use crate::syntax::Syntax;
use oxigraph::model::{BlankNode, Literal, NamedNode, Quad, Term};
use oxigraph::store::Store;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

pub(crate) struct Lift<'a> {
    text: &'a str,
    root: Node,
    records: Vec<Vec<usize>>,
    next: usize,
    paths: Paths,
    kinds: Option<Kinds>,
}

pub(crate) fn lift<'a>(text: &'a str, reading: &Reading<'_>, paths: Paths) -> Result<Lift<'a>> {
    let root = json::parse(text)?;
    if !root.is_container() {
        return Err(Error::document(
            "the document's value is neither an object nor an array, and the JSON lift lifts \
             nothing from it",
        ));
    }
    let mut lift = Lift {
        text,
        root,
        records: Vec::new(),
        next: 0,
        paths,
        kinds: None,
    };
    let chosen = admitting(&lift, reading)
        .and_then(|chosen| reading.envelopes.get(chosen))
        .copied();
    if let Some(path) = chosen.and_then(|admission| admission.records.as_deref()) {
        lift.records = json::records(&lift.root, &json::path(path)?);
    }
    if matches!(lift.paths, Paths::Kept { kinds: true, .. }) {
        lift.kinds = chosen.and_then(|admission| admission.kinds.clone());
    }
    Ok(lift)
}

/// Fresh blank nodes, and the triples written from them.
#[derive(Default)]
struct Written {
    quads: Vec<Quad>,
    next: usize,
    /// Everywhere but in a skeleton.
    decodes_attachments: bool,
}

/// FHIR's `Attachment` in base64, of a text media type and no other charset than UTF-8.
fn text_of_attachment(node: &Node) -> Option<String> {
    let Value::Object(members) = &node.value else {
        return None;
    };
    let string = |name: &str| {
        members
            .iter()
            .find(|(named, _)| named == name)
            .and_then(|(_, member)| match &member.value {
                Value::String(text) => Some(text.as_str()),
                _ => None,
            })
    };
    string("contentType")
        .filter(|content_type| names_text(content_type))
        .and_then(|_| decoded(string("data")?))
}

impl Written {
    fn decoding_attachments() -> Self {
        Self {
            decodes_attachments: true,
            ..Self::default()
        }
    }

    fn fresh(&mut self) -> BlankNode {
        self.next += 1;
        BlankNode::new_unchecked(format!("j{}", self.next))
    }

    /// A container's node; one in `emptied` keeps its scalar members alone.
    fn container(&mut self, node: &Node, emptied: &HashSet<*const Node>) -> Result<BlankNode> {
        let id = self.fresh();
        let emptying = emptied.contains(&std::ptr::from_ref(node));
        let children: Vec<(NamedNode, &Node)> = match &node.value {
            Value::Object(members) => members
                .iter()
                .map(|(named, member)| Ok((name(XYZ, named)?, member)))
                .collect::<Result<_>>()?,
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| Ok((member(index + 1)?, item)))
                .collect::<Result<_>>()?,
            _ => Vec::new(),
        };
        for (predicate, child) in children {
            let object = match child.scalar() {
                Some(text) => Term::from(Literal::new_simple_literal(text)),
                None if child.is_container() && !emptying => {
                    Term::from(self.container(child, emptied)?)
                }
                None => continue,
            };
            self.quads.push(triple(&id, predicate, object));
        }
        if let Some(text) = self
            .decodes_attachments
            .then(|| text_of_attachment(node))
            .flatten()
        {
            self.quads.push(triple(
                &id,
                NamedNode::new(TEXT_DECODED_FROM_BASE64)?,
                Literal::new_simple_literal(text),
            ));
        }
        Ok(id)
    }

    fn root(mut self, node: &Node, emptied: &HashSet<*const Node>) -> Result<Vec<Quad>> {
        let id = self.container(node, emptied)?;
        self.quads.push(triple(
            &id,
            NamedNode::new(format!("{RDF}type"))?,
            NamedNode::new(format!("{FX}root"))?,
        ));
        Ok(self.quads)
    }
}

/// The lift of a whole document's value.
#[cfg(test)]
pub(crate) fn lifted(text: &str) -> Result<Vec<Quad>> {
    Written::decoding_attachments().root(&json::parse(text)?, &HashSet::new())
}

impl super::Lift for Lift<'_> {
    fn next_unit(&mut self) -> Result<Option<Unit>> {
        let Some(positions) = self.records.get(self.next) else {
            return Ok(None);
        };
        self.next += 1;
        let record = self.root.at(positions).expect("a record the path selected");
        let census = match &self.paths {
            Paths::Kept {
                valued, carried, ..
            } => census(record, valued, carried, self.kinds.as_ref()),
            Paths::Dropped => Census::default(),
        };
        Ok(Some(Unit {
            store: store_of(Written::decoding_attachments().root(record, &HashSet::new())?)?,
            text: self.text[record.span.clone()].to_owned(),
            selector: json::pointer(&json::tokens_at(&self.root, positions)),
            occurrences: census.occurrences,
            values: census.values,
            standings: census.standings,
        }))
    }

    fn admits(&self, admission: &Admission) -> bool {
        let (Value::Object(members), Some(root)) = (&self.root.value, &admission.root) else {
            return false;
        };
        members.iter().any(|(named, member)| {
            named == root
                && admission
                    .value
                    .as_deref()
                    .is_none_or(|value| member.scalar() == Some(value))
        })
    }

    fn document_selector(&self, _described: Option<&str>) -> String {
        String::new()
    }

    fn into_skeleton(self: Box<Self>) -> Result<Store> {
        let emptied: HashSet<*const Node> = self
            .records
            .iter()
            .filter_map(|positions| self.root.at(positions))
            .map(std::ptr::from_ref)
            .collect();
        store_of(Written::default().root(&self.root, &emptied)?)
    }
}

#[derive(Default)]
struct Census {
    seen: HashMap<String, usize>,
    occurrences: Vec<Occurrence>,
    held: HashMap<(String, String), usize>,
    values: Vec<Valued>,
    standings: Vec<Standing>,
}

struct Kept<'a> {
    valued: &'a HashSet<String>,
    carried: &'a HashSet<String>,
    kinds: Option<&'a Kinds>,
}

/// The text of the scalar a pointer of member names reaches from `node`.
fn text_at(node: &Node, pointer: &str) -> Option<String> {
    let mut node = node;
    for token in pointer.split('/').skip(1) {
        let name = token.replace("~1", "/").replace("~0", "~");
        let Value::Object(members) = &node.value else {
            return None;
        };
        node = &members.iter().find(|(named, _)| *named == name)?.1;
    }
    node.scalar().map(str::to_owned)
}

/// A path's step is a member's name as a JSON Pointer reference token; an array's
/// items stand at its own path. Where `kinds` is given, a path is counted apart in each record
/// kind it stands in: the record's, or that of the contained record it stands below.
fn census(
    record: &Node,
    valued: &HashSet<String>,
    carried: &HashSet<String>,
    kinds: Option<&Kinds>,
) -> Census {
    let mut census = Census::default();
    let kind = kinds.and_then(|kinds| text_at(record, &kinds.record));
    census.members(
        record,
        "",
        &mut Vec::new(),
        &Kept {
            valued,
            carried,
            kinds,
        },
        &kind,
    );
    census
}

fn holds_nothing(node: &Node) -> bool {
    match &node.value {
        Value::String(text) => Syntax::Json.holds_nothing(text),
        Value::Object(members) => members.iter().all(|(_, member)| holds_nothing(member)),
        Value::Array(items) => items.iter().all(holds_nothing),
        Value::Null => true,
        Value::Number(_) | Value::Bool(_) => false,
    }
}

impl Census {
    fn members(
        &mut self,
        node: &Node,
        path: &str,
        tokens: &mut Vec<String>,
        kept: &Kept<'_>,
        kind: &Option<String>,
    ) {
        let Value::Object(members) = &node.value else {
            return;
        };
        for (named, member) in members {
            let path = format!("{path}/{}", named.replace('~', "~0").replace('/', "~1"));
            tokens.push(named.clone());
            self.stand(member, &path, tokens, kept, kind);
            tokens.pop();
        }
    }

    fn stand(
        &mut self,
        node: &Node,
        path: &str,
        tokens: &mut Vec<String>,
        kept: &Kept<'_>,
        kind: &Option<String>,
    ) {
        match &node.value {
            Value::Null => {}
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    tokens.push(index.to_string());
                    self.stand(item, path, tokens, kept, kind);
                    tokens.pop();
                }
            }
            _ => {
                let contained;
                let kind = match kept.kinds {
                    Some(kinds) if kinds.contained.as_deref() == Some(path) => {
                        contained = text_at(node, &kinds.kind);
                        &contained
                    }
                    _ => kind,
                };
                let within = json::pointer(tokens);
                if let Some(value) = node.scalar().filter(|_| kept.valued.contains(path)) {
                    self.hold(path, value, &within);
                }
                if kept.carried.contains(path) {
                    self.standings.push(Standing {
                        path: path.to_owned(),
                        within: Some(within.clone()),
                        empty: holds_nothing(node),
                    });
                }
                self.add(path, within, kind);
                self.members(node, path, tokens, kept, kind);
            }
        }
    }

    fn add(&mut self, path: &str, within: String, kind: &Option<String>) {
        let key = match kind {
            Some(kind) => Cow::Owned(format!("{kind}\0{path}")),
            None => Cow::Borrowed(path),
        };
        match self.seen.get(&*key) {
            Some(&first) => self.occurrences[first].count += 1,
            None => {
                self.seen.insert(key.into_owned(), self.occurrences.len());
                self.occurrences.push(Occurrence {
                    path: path.to_owned(),
                    within: Some(within),
                    count: 1,
                    kind: kind.clone(),
                });
            }
        }
    }

    fn hold(&mut self, path: &str, value: &str, within: &str) {
        let held = (path.to_owned(), value.to_owned());
        match self.held.get(&held) {
            Some(&first) => self.values[first].count += 1,
            None => {
                self.held.insert(held, self.values.len());
                self.values.push(Valued {
                    path: path.to_owned(),
                    value: value.to_owned(),
                    within: Some(within.to_owned()),
                    count: 1,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;
