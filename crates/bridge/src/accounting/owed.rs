use super::{Accounting, Carry, Owed};
use crate::annotation::Record;
use crate::lift::Unit;
use crate::load::as_subject;
use crate::terms::{
    BRIDGE_ARRIVED_AS, OA_HAS_BODY, OA_HAS_SELECTOR, OA_HAS_TARGET, OA_REFINED_BY,
    PROV_SPECIALIZATION_OF, RDF_TYPE, RDF_VALUE,
};
use oxigraph::model::{NamedOrBlankNode, Quad, Term};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// An occurrence of a path the accounting says is carried, owed by the records it
/// belongs to, with none of `owed` written on them and no finding about it.
pub(crate) struct Unmet {
    pub(crate) path: String,
    pub(crate) address: String,
    /// Each class owed and the property looked for on it.
    pub(crate) owed: Vec<(String, String)>,
}

impl fmt::Display for Unmet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let looked: Vec<String> = self
            .owed
            .iter()
            .map(|(class, property)| format!("{property} on its {class}"))
            .collect();
        write!(
            f,
            "{} at {:?}: no {} is written, and no finding is about it",
            self.path,
            self.address,
            looked.join(" or ")
        )
    }
}

impl Accounting {
    /// `nested` is where, in this unit, the records nested in it stand, each one step
    /// below it.
    pub(crate) fn unmet(
        &self,
        record: &Record<'_>,
        unit: &Unit,
        nested: Option<&str>,
        produced: &[Quad],
        findings: &[Quad],
    ) -> Vec<Unmet> {
        let mut unmet = Vec::new();
        if self.owed.is_empty() || unit.standings().is_empty() {
            return unmet;
        }
        let written = Written::of(produced);
        if written.places.is_empty() {
            return unmet;
        }
        let about = About::of(findings, record);
        let nested = nested.map(steps);
        let nested = nested.as_deref();
        // A value written from several members of one element is not written where one is empty.
        let mut hollow: HashSet<(String, &str)> = HashSet::new();
        for standing in unit.standings().iter().filter(|standing| standing.empty) {
            let address = joined(record.selector, standing.within.as_deref());
            let element = parent(&address);
            for carry in self
                .owed
                .get(&standing.path)
                .into_iter()
                .flat_map(|owed| &owed.carries)
            {
                hollow.insert((element.clone(), carry.property.as_str()));
            }
        }
        for standing in unit.standings().iter().filter(|standing| !standing.empty) {
            let Some(owed) = self.owed.get(&standing.path) else {
                continue;
            };
            let address = joined(record.selector, standing.within.as_deref());
            let element = parent(&address);
            if owed
                .carries
                .iter()
                .any(|carry| hollow.contains(&(element.clone(), carry.property.as_str())))
            {
                continue;
            }
            let at = steps(&address);
            let inside = region(nested, &at);
            let mut nearest: Vec<&NamedOrBlankNode> = Vec::new();
            let mut best = (0, false);
            for (holder, place) in &written.places {
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
                    nearest.push(holder);
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
            unmet.push(Unmet {
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
    /// Each record with the steps of an address one of its arrivals was selected at.
    places: Vec<(NamedOrBlankNode, Vec<&'a str>)>,
    /// Each property written on a record, its versions, their arrivals, or a node they
    /// point at that is no record, version or arrival of its own.
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
            for record in owners.get(&quad.subject).into_iter().flatten() {
                carried
                    .entry(record.clone())
                    .or_default()
                    .insert(quad.predicate.as_str());
            }
        }
        let places = version_of
            .iter()
            .filter_map(|(arrival, version)| {
                let record = record_of.get(version)?;
                let value = value_of.get(selected_at.get(arrival)?).copied()?;
                Some((record.clone(), steps(value)))
            })
            .collect();
        Self {
            classes,
            places,
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

/// Where each of a unit's findings was addressed, as steps, and its body.
struct About<'a> {
    addressed: Vec<(Vec<String>, &'a str)>,
}

impl<'a> About<'a> {
    fn of(findings: &'a [Quad], record: &Record<'_>) -> Self {
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
        let addressed = body_of
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
        Self { addressed }
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
        None | Some("") => selector.to_owned(),
        Some(within) if within.starts_with('/') => format!("{selector}{within}"),
        Some(within) => format!("{selector}/{within}"),
    }
}
