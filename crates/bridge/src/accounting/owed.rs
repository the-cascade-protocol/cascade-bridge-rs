use super::{Accounting, Carry, Owed};
use crate::annotation::Record;
use crate::decode::holds_no_text;
use crate::lift::Unit;
use crate::load::as_subject;
use crate::syntax::Respelling;
use crate::terms::{
    BRIDGE_ARRIVED_AS, OA_HAS_BODY, OA_HAS_SELECTOR, OA_HAS_TARGET, OA_REFINED_BY,
    PROV_SPECIALIZATION_OF, RDF_TYPE, RDF_VALUE,
};
use oxigraph::model::{NamedOrBlankNode, Quad, Term};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

/// What `test` fails an entry for.
#[derive(Clone)]
pub(crate) enum Unmet {
    /// An occurrence of a path the accounting says is carried, owed by the records it
    /// belongs to, with none of `owed` written on them and no finding about it.
    Occurrence {
        path: String,
        address: String,
        /// Each class owed and the property looked for on it.
        owed: Vec<(String, String)>,
    },
    /// A record whose arrival selector places it nowhere in the unit, so that what it holds
    /// cannot be told.
    Unplaced { selector: String, selects: String },
}

impl fmt::Display for Unmet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Occurrence {
                path,
                address,
                owed,
            } => write!(f, "{path} at {address:?}: {}", looked_for(owed)),
            Self::Unplaced { .. } => f.write_str(&self.summary()),
        }
    }
}

impl Unmet {
    /// As an entry's description says it: what is owed, then where.
    pub(crate) fn summary(&self) -> String {
        match self {
            Self::Occurrence {
                path,
                address,
                owed,
            } => format!("{path}: {}, at {address:?}", looked_for(owed)),
            Self::Unplaced { selector, selects } => format!(
                "the record selector {selector:?} {selects}, so no occurrence can be held to the \
                 record it places"
            ),
        }
    }
}

fn looked_for(owed: &[(String, String)]) -> String {
    let looked: Vec<String> = owed
        .iter()
        .map(|(class, property)| format!("{property} on its {class}"))
        .collect();
    format!(
        "no {} is written, and no finding is about it",
        looked.join(" or ")
    )
}

impl Accounting {
    /// `nested` is where, in this unit, the records nested in it stand, each one step
    /// below it; `spelling` spells an address of the document as the unit's occurrences are.
    pub(crate) fn unmet(
        &self,
        record: &Record<'_>,
        unit: &Unit,
        nested: Option<&str>,
        produced: &[Quad],
        findings: &[Quad],
        spelling: &dyn Respelling,
    ) -> Vec<Unmet> {
        let mut unmet = Vec::new();
        if self.owed.is_empty() || unit.standings().is_empty() {
            return unmet;
        }
        let written = Written::of(produced);
        if written.selected.is_empty() {
            return unmet;
        }
        let mut spelled: HashMap<&str, std::result::Result<String, String>> = HashMap::new();
        for (_, selector) in &written.selected {
            spelled.entry(*selector).or_insert_with(|| {
                if *selector == record.selector {
                    Ok((*selector).to_owned())
                } else {
                    spelling.spelled(selector)
                }
            });
        }
        let unit_at = steps(record.selector);
        let mut places: Vec<(&NamedOrBlankNode, Vec<&str>)> = Vec::new();
        let mut unplaced: BTreeSet<(&str, String)> = BTreeSet::new();
        for (holder, selector) in &written.selected {
            match &spelled[selector] {
                Ok(spelt) => {
                    let place = steps(spelt);
                    if place.starts_with(&unit_at) {
                        places.push((holder, place));
                    } else {
                        let outside =
                            format!("selects a node outside the unit at {:?}", record.selector);
                        unplaced.insert((*selector, outside));
                    }
                }
                Err(selects) => {
                    unplaced.insert((*selector, selects.clone()));
                }
            }
        }
        if !unplaced.is_empty() {
            return unplaced
                .into_iter()
                .map(|(selector, selects)| Unmet::Unplaced {
                    selector: selector.to_owned(),
                    selects,
                })
                .collect();
        }
        let findings = spelling.respelled(findings.to_vec()).findings;
        let mut about = About::of(&findings, record, &self.not_carried_whole);
        // A finding that nothing in an element is carried, where a record stands in it, is
        // contradicted by that record.
        about
            .dropped
            .retain(|found| !places.iter().any(|(_, place)| holds(found, place)));
        let nested = nested.map(steps);
        let nested = nested.as_deref();
        // A value written from several members of one element is not written where one is
        // empty; a repeat of the same path is no other member.
        let mut hollow: HashMap<(String, &str), Vec<&str>> = HashMap::new();
        for standing in unit.standings().iter().filter(|standing| standing.empty) {
            let address = joined(record.selector, standing.within.as_deref());
            let element = parent(&address);
            for carry in self
                .owed
                .get(&standing.path)
                .into_iter()
                .flat_map(|owed| &owed.carries)
            {
                hollow
                    .entry((element.clone(), carry.property.as_str()))
                    .or_default()
                    .push(standing.path.as_str());
            }
        }
        for standing in unit.standings().iter().filter(|standing| !standing.empty) {
            let Some(owed) = self.owed.get(&standing.path) else {
                continue;
            };
            let address = joined(record.selector, standing.within.as_deref());
            let element = parent(&address);
            let hollowed = |carry: &Carry| {
                hollow
                    .get(&(element.clone(), carry.property.as_str()))
                    .is_some_and(|paths| paths.iter().any(|path| *path != standing.path))
            };
            if owed.carries.iter().any(hollowed) {
                continue;
            }
            let at = steps(&address);
            if about.drops(&at) {
                continue;
            }
            // What is in a nested record no record stands in is held by the records around it.
            let mut inside = region(nested, &at);
            if !places
                .iter()
                .any(|(_, place)| region(nested, place) == inside)
            {
                inside = None;
            }
            let mut nearest: Vec<&NamedOrBlankNode> = Vec::new();
            let mut best = (0, false);
            for (holder, place) in &places {
                if region(nested, place) != inside {
                    continue;
                }
                let shared = place
                    .iter()
                    .zip(&at)
                    .take_while(|(one, two)| one == two)
                    .count();
                let near = (shared, shared == place.len());
                if nearest.is_empty() || near > best {
                    best = near;
                    nearest.clear();
                }
                if near == best {
                    nearest.push(*holder);
                }
            }
            let mut looked: Vec<(&Carry, &NamedOrBlankNode)> = Vec::new();
            for carry in &owed.carries {
                for holder in &nearest {
                    if written.is_of(holder, &carry.class) {
                        looked.push((carry, *holder));
                    }
                }
            }
            if looked.is_empty()
                || looked
                    .iter()
                    .any(|(carry, holder)| written.carries(holder, &carry.property))
                || about.meets(&at, owed)
            {
                continue;
            }
            let mut owed: Vec<(String, String)> = looked
                .iter()
                .map(|(carry, _)| (carry.class.clone(), carry.property.clone()))
                .collect();
            owed.sort();
            owed.dedup();
            unmet.push(Unmet::Occurrence {
                path: standing.path.clone(),
                address,
                owed,
            });
        }
        unmet
    }
}

/// What a unit's conversion wrote, read for where each record stands and what is
/// written on it.
struct Written<'a> {
    classes: HashMap<NamedOrBlankNode, Vec<&'a str>>,
    /// Each record with the address one of its arrivals was selected at, as written.
    selected: Vec<(NamedOrBlankNode, &'a str)>,
    /// Each property written with a value that is not empty, on a record, its versions,
    /// their arrivals, or a node they point at that is no record, version or arrival of
    /// its own.
    carried: HashMap<NamedOrBlankNode, HashSet<&'a str>>,
}

impl<'a> Written<'a> {
    fn of(produced: &'a [Quad]) -> Self {
        let mut classes: HashMap<NamedOrBlankNode, Vec<&str>> = HashMap::new();
        let mut record_of: HashMap<NamedOrBlankNode, NamedOrBlankNode> = HashMap::new();
        let mut version_of: HashMap<NamedOrBlankNode, NamedOrBlankNode> = HashMap::new();
        let mut selected_at: HashMap<NamedOrBlankNode, NamedOrBlankNode> = HashMap::new();
        let mut value_of: HashMap<NamedOrBlankNode, &str> = HashMap::new();
        let mut children: HashMap<NamedOrBlankNode, Vec<NamedOrBlankNode>> = HashMap::new();
        for quad in produced {
            let predicate = quad.predicate.as_str();
            match (predicate, &quad.object) {
                (RDF_TYPE, Term::NamedNode(class)) => {
                    classes
                        .entry(quad.subject.clone())
                        .or_default()
                        .push(class.as_str());
                }
                (RDF_VALUE, Term::Literal(value)) => {
                    value_of.insert(quad.subject.clone(), value.value());
                }
                (_, object) => {
                    let Some(object) = as_subject(object) else {
                        continue;
                    };
                    match predicate {
                        PROV_SPECIALIZATION_OF => {
                            record_of.insert(quad.subject.clone(), object);
                        }
                        BRIDGE_ARRIVED_AS => {
                            version_of.insert(quad.subject.clone(), object);
                        }
                        _ => {
                            if predicate == OA_HAS_SELECTOR {
                                selected_at.insert(quad.subject.clone(), object.clone());
                            }
                            children
                                .entry(quad.subject.clone())
                                .or_default()
                                .push(object);
                        }
                    }
                }
            }
        }
        let records: HashSet<&NamedOrBlankNode> = record_of.values().collect();
        let mut owners: HashMap<NamedOrBlankNode, HashSet<NamedOrBlankNode>> = HashMap::new();
        let mut pending: Vec<NamedOrBlankNode> = Vec::new();
        let mut own = |node: &NamedOrBlankNode, record: &NamedOrBlankNode| {
            if owners
                .entry(node.clone())
                .or_default()
                .insert(record.clone())
            {
                pending.push(node.clone());
            }
        };
        for (version, record) in &record_of {
            own(record, record);
            own(version, record);
        }
        for (arrival, version) in &version_of {
            if let Some(record) = record_of.get(version) {
                own(arrival, record);
            }
        }
        while let Some(node) = pending.pop() {
            let of: Vec<NamedOrBlankNode> = owners[&node].iter().cloned().collect();
            for child in children.get(&node).into_iter().flatten().filter(|child| {
                !records.contains(*child)
                    && !record_of.contains_key(*child)
                    && !version_of.contains_key(*child)
            }) {
                let held = owners.entry(child.clone()).or_default();
                let before = held.len();
                held.extend(of.iter().cloned());
                if held.len() != before {
                    pending.push(child.clone());
                }
            }
        }
        let mut carried: HashMap<NamedOrBlankNode, HashSet<&str>> = HashMap::new();
        for quad in produced {
            if matches!(&quad.object, Term::Literal(value) if holds_no_text(value.value())) {
                continue;
            }
            for record in owners.get(&quad.subject).into_iter().flatten() {
                carried
                    .entry(record.clone())
                    .or_default()
                    .insert(quad.predicate.as_str());
            }
        }
        let selected = version_of
            .iter()
            .filter_map(|(arrival, version)| {
                let record = record_of.get(version)?;
                let value = value_of.get(selected_at.get(arrival)?).copied()?;
                Some((record.clone(), value))
            })
            .collect();
        Self {
            classes,
            selected,
            carried,
        }
    }

    fn is_of(&self, record: &NamedOrBlankNode, class: &str) -> bool {
        self.classes
            .get(record)
            .is_some_and(|classes| classes.contains(&class))
    }

    fn carries(&self, record: &NamedOrBlankNode, property: &str) -> bool {
        self.carried
            .get(record)
            .is_some_and(|properties| properties.contains(property))
    }
}

/// Where each of a unit's findings was addressed, as steps, and its body; and where each
/// said the element there was not carried, nor anything in it.
struct About<'a> {
    addressed: Vec<(Vec<String>, &'a str)>,
    dropped: Vec<Vec<String>>,
}

impl<'a> About<'a> {
    fn of(findings: &'a [Quad], record: &Record<'_>, not_carried_whole: &HashSet<String>) -> Self {
        let mut step: HashMap<(&str, &NamedOrBlankNode), NamedOrBlankNode> = HashMap::new();
        let mut text: HashMap<&NamedOrBlankNode, &str> = HashMap::new();
        let mut body_of: Vec<(&NamedOrBlankNode, &str)> = Vec::new();
        for quad in findings {
            let predicate = quad.predicate.as_str();
            match (predicate, &quad.object) {
                (OA_HAS_BODY, Term::NamedNode(body)) => {
                    body_of.push((&quad.subject, body.as_str()));
                }
                (RDF_VALUE, Term::Literal(value)) => {
                    text.insert(&quad.subject, value.value());
                }
                (OA_HAS_TARGET | OA_HAS_SELECTOR | OA_REFINED_BY, object) => {
                    if let Some(object) = as_subject(object) {
                        step.insert((predicate, &quad.subject), object);
                    }
                }
                _ => {}
            }
        }
        let addressed: Vec<(Vec<String>, &str)> = body_of
            .into_iter()
            .filter_map(|(annotation, body)| {
                let target = step.get(&(OA_HAS_TARGET, annotation))?;
                let selector = step.get(&(OA_HAS_SELECTOR, target))?;
                let at = text.get(selector).copied().unwrap_or(record.selector);
                let refined = step
                    .get(&(OA_REFINED_BY, selector))
                    .and_then(|refinement| text.get(refinement).copied());
                let address = joined(at, refined);
                Some((
                    steps(&address).into_iter().map(str::to_owned).collect(),
                    body,
                ))
            })
            .collect();
        let dropped = addressed
            .iter()
            .filter(|(_, body)| not_carried_whole.contains(*body))
            .map(|(found, _)| found.clone())
            .collect();
        Self { addressed, dropped }
    }

    /// Whether one said an element holding the occurrence, or the occurrence itself, was not
    /// carried at all: the occurrence then belongs to no record.
    fn drops(&self, at: &[&str]) -> bool {
        self.dropped.iter().any(|found| holds(found, at))
    }

    /// Whether one was addressed at the occurrence or below it, or above it with a body
    /// its path's entry names.
    fn meets(&self, at: &[&str], owed: &Owed) -> bool {
        self.addressed.iter().any(|(found, body)| {
            let shared = found
                .iter()
                .zip(at)
                .take_while(|(one, two)| one.as_str() == **two)
                .count();
            (shared == at.len() && found.len() >= at.len())
                || (shared == found.len()
                    && found.len() < at.len()
                    && owed.gaps.iter().any(|gap| gap == body))
        })
    }
}

/// Whether two addresses of the document select one node, read as steps (name, namespace,
/// position) however each is spelled.
pub(crate) fn same_place(spelling: &dyn Respelling, one: &str, other: &str) -> bool {
    one == other
        || matches!(
            (spelling.spelled(one), spelling.spelled(other)),
            (Ok(one), Ok(other)) if steps(&one) == steps(&other)
        )
}

/// An address's steps: split at each `/` outside a predicate's brackets.
fn steps(address: &str) -> Vec<&str> {
    let mut steps = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, character) in address.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') if depth > 0 => quote = Some(character),
            (None, '[') => depth += 1,
            (None, ']') => depth = depth.saturating_sub(1),
            (None, '/') if depth == 0 => {
                if at > start {
                    steps.push(&address[start..at]);
                }
                start = at + 1;
            }
            _ => {}
        }
    }
    if address.len() > start {
        steps.push(&address[start..]);
    }
    steps
}

/// Whether the address of `found` is `at` or an element holding it.
fn holds<A: AsRef<str>, B: AsRef<str>>(found: &[A], at: &[B]) -> bool {
    found.len() <= at.len()
        && found
            .iter()
            .zip(at)
            .all(|(one, two)| one.as_ref() == two.as_ref())
}

/// The record nested in this unit that the steps lie in, or none where they lie in no
/// nested record.
fn region<'s>(nested: Option<&[&str]>, steps: &'s [&'s str]) -> Option<&'s [&'s str]> {
    let nested = nested?;
    (steps.len() > nested.len() && steps.starts_with(nested)).then(|| &steps[..=nested.len()])
}

fn parent(address: &str) -> String {
    let steps = steps(address);
    steps[..steps.len().saturating_sub(1)].join("/")
}

fn joined(selector: &str, within: Option<&str>) -> String {
    match within {
        None | Some("" | ".") => selector.to_owned(),
        Some(within) if within.starts_with('/') => format!("{selector}{within}"),
        Some(within) => format!("{selector}/{within}"),
    }
}
