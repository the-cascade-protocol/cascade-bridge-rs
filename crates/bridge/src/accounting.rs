use crate::annotation::{self, Record};
use crate::decode::XML_SPACE;
use crate::error::{Error, Result};
use crate::lift::{Paths, Unit, Valued};
use crate::load::{as_subject, instances, objects, subjects, turtle, Prefixes};
use crate::resolver::Resolver;
use crate::run::Asked;
use crate::terms::{
    BRIDGE_AS_PROPERTY, BRIDGE_CARRIED, BRIDGE_CARRIED_AS, BRIDGE_CARRIED_IN_PART,
    BRIDGE_ARRIVED_AS, BRIDGE_IN_RECORD_CLASS, BRIDGE_IN_SOURCE_RECORD_TYPE, BRIDGE_LOOKUP_IN, BRIDGE_LOOKUP_NAMES_GAP, BRIDGE_NAMES_GAP,
    BRIDGE_NO_HOME, BRIDGE_NO_PREDICATE, BRIDGE_PATH_ENTRY, BRIDGE_SOURCE_LACKS_REQUIRED,
    BRIDGE_SOURCE_PATH, BRIDGE_VALUE_NOT_MAPPED, BRIDGE_VERDICT, OA_HAS_BODY, PROV_SPECIALIZATION_OF,
    RDF_TYPE, SH_INFO, SH_RESULT_SEVERITY, SH_VALUE, SH_VIOLATION, SH_WARNING, SKOS_BROADER,
    SKOS_CONCEPT_SCHEME, SKOS_NOTATION,
};
use oxigraph::model::{NamedOrBlankNode, Quad, Term};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

struct Entry {
    path: String,
    verdict: Option<String>,
    gap: Option<String>,
    /// The concept map a value is looked up in; `miss` is the gap for a value it lacks.
    map: Option<String>,
    miss: Option<String>,
    carries: Vec<Carry>,
}

/// One `bridge:carriedAs`.
#[derive(Clone)]
struct Carry {
    record_type: Option<String>,
    class: String,
    property: String,
}

/// What an entry that carries its path says the conversion writes for it.
#[derive(Default)]
struct Forward {
    carries: Vec<Carry>,
    gaps: Vec<String>,
}

pub(crate) enum ForwardKind {
    /// None of the entry's properties was written, and no finding was about the path.
    Missed,
    /// The record holds the path, but no `bridge:carriedAs` of the entry names a source record type the unit holds.
    Unlisted,
}

/// A path the accounting says is carried, found in a record that wrote none of what it
/// carries it as and reported no gap for it.
pub(crate) struct ForwardMiss {
    pub(crate) kind: ForwardKind,
    pub(crate) record: String,
    pub(crate) path: String,
    pub(crate) wanted: Vec<(String, String)>,
}

#[derive(Default)]
pub(crate) struct Gap {
    kind: Option<String>,
    pub(crate) severity: Option<String>,
}

const NAMES_A_GAP: [&str; 2] = [BRIDGE_NO_HOME, BRIDGE_CARRIED_IN_PART];

/// The kinds of gap true of the path, not of what a record holds at it.
const REPORTS: [&str; 2] = [BRIDGE_NO_PREDICATE, BRIDGE_SOURCE_LACKS_REQUIRED];

const SEVERITIES: [&str; 3] = [SH_INFO, SH_WARNING, SH_VIOLATION];

struct Reported {
    gap: String,
    severity: String,
}

struct Lookup {
    gap: String,
    severity: String,
    notations: Arc<HashSet<String>>,
}

/// The form a `skos:notation` is written in.
fn key(value: &str) -> String {
    value.trim_matches(XML_SPACE).to_lowercase()
}

pub(crate) struct Accounting {
    paths: HashSet<String>,
    reported: HashMap<String, Vec<Reported>>,
    lookups: HashMap<String, Vec<Lookup>>,
    forward: HashMap<String, Forward>,
}

impl Accounting {
    pub(crate) fn read(
        resolver: &dyn Resolver,
        iri: &str,
        scheme: &HashMap<String, Gap>,
    ) -> Result<Self> {
        Self::of(entries(resolver, iri)?, scheme, iri, resolver)
    }

    pub(crate) fn paths(&self) -> Paths {
        Paths::Kept {
            valued: self.lookups.keys().cloned().collect(),
        }
    }

    fn of(
        entries: Vec<Entry>,
        scheme: &HashMap<String, Gap>,
        iri: &str,
        resolver: &dyn Resolver,
    ) -> Result<Self> {
        let mut paths = HashSet::new();
        let mut reported: HashMap<String, Vec<Reported>> = HashMap::new();
        let mut lookups: HashMap<String, Vec<Lookup>> = HashMap::new();
        let mut maps: HashMap<String, Arc<HashSet<String>>> = HashMap::new();
        let mut forward: HashMap<String, Forward> = HashMap::new();
        for entry in entries {
            if entry
                .verdict
                .as_deref()
                .is_some_and(|verdict| verdict == BRIDGE_CARRIED || verdict == BRIDGE_CARRIED_IN_PART)
                && !entry.carries.is_empty()
            {
                let carried = forward.entry(entry.path.clone()).or_default();
                carried.carries.extend(entry.carries.iter().cloned());
                carried
                    .gaps
                    .extend(entry.gap.iter().chain(entry.miss.iter()).cloned());
            }
            if let (Some(verdict), Some(gap)) = (&entry.verdict, &entry.gap) {
                if NAMES_A_GAP.contains(&verdict.as_str()) {
                    let declared = scheme.get(gap.as_str()).ok_or_else(|| {
                        Error::adapter(format!(
                            "{iri}: the entry for {} names {gap}, which the adapter's \
                             bridge:gapScheme does not declare",
                            entry.path
                        ))
                    })?;
                    let kind = declared.kind.as_deref().ok_or_else(|| {
                        Error::adapter(format!(
                            "{iri}: the entry for {} names {gap}, which declares no skos:broader, \
                             so what kind of gap it is cannot be read",
                            entry.path
                        ))
                    })?;
                    if REPORTS.contains(&kind) {
                        reported
                            .entry(entry.path.clone())
                            .or_default()
                            .push(Reported {
                                gap: gap.clone(),
                                severity: declared
                                    .severity
                                    .clone()
                                    .unwrap_or_else(|| SH_INFO.to_owned()),
                            });
                    }
                }
            }
            match (&entry.map, &entry.miss) {
                (None, None) => {}
                (Some(_), None) | (None, Some(_)) => {
                    return Err(Error::adapter(format!(
                        "{iri}: the entry for {} declares one half of a lookup; bridge:lookupIn \
                         and bridge:lookupNamesGap are declared together or not at all",
                        entry.path
                    )))
                }
                (Some(map), Some(gap)) => {
                    let declared = scheme.get(gap.as_str()).ok_or_else(|| {
                        Error::adapter(format!(
                            "{iri}: the lookup of the entry for {} names {gap}, which the \
                             adapter's bridge:gapScheme does not declare",
                            entry.path
                        ))
                    })?;
                    let kind = declared.kind.as_deref().ok_or_else(|| {
                        Error::adapter(format!(
                            "{iri}: the lookup of the entry for {} names {gap}, which declares no \
                             skos:broader, so what kind of gap it is cannot be read",
                            entry.path
                        ))
                    })?;
                    if kind != BRIDGE_VALUE_NOT_MAPPED {
                        return Err(Error::adapter(format!(
                            "{iri}: the lookup of the entry for {} names {gap}, a gap of kind \
                             {kind}; a value a concept map holds no notation for is a gap of kind \
                             {BRIDGE_VALUE_NOT_MAPPED}",
                            entry.path
                        )));
                    }
                    if !maps.contains_key(map) {
                        let notations = concept_map(resolver, map).map_err(|e| {
                            Error::adapter(format!(
                                "{iri}: the entry for {} looks its values up in {e}",
                                entry.path
                            ))
                        })?;
                        maps.insert(map.clone(), Arc::new(notations));
                    }
                    lookups.entry(entry.path.clone()).or_default().push(Lookup {
                        gap: gap.clone(),
                        severity: declared
                            .severity
                            .clone()
                            .unwrap_or_else(|| SH_INFO.to_owned()),
                        notations: maps[map].clone(),
                    });
                }
            }
            paths.insert(entry.path);
        }
        // Sorted, so a run's output does not depend on which way a hash fell.
        for gaps in reported.values_mut() {
            gaps.sort_by(|one, two| one.gap.cmp(&two.gap));
        }
        for found in lookups.values_mut() {
            found.sort_by(|one, two| one.gap.cmp(&two.gap));
        }
        Ok(Self {
            paths,
            reported,
            lookups,
            forward,
        })
    }

    /// The accounting checked forward, for one record: each path it holds that the
    /// accounting marks carried, with none of its `bridge:carriedAs` properties written
    /// on a version of its class and no finding addressed to the path or naming one of
    /// the entry's gaps.
    pub(crate) fn forward_misses(
        &self,
        record: &Record<'_>,
        unit: &Unit,
        produced: &[Quad],
        findings: &[Quad],
    ) -> Vec<ForwardMiss> {
        let mut misses = Vec::new();
        if self.forward.is_empty() {
            return misses;
        }
        let mut classes: HashMap<NamedOrBlankNode, Vec<&str>> = HashMap::new();
        let mut record_of: HashMap<NamedOrBlankNode, NamedOrBlankNode> = HashMap::new();
        let mut version_of: HashMap<NamedOrBlankNode, NamedOrBlankNode> = HashMap::new();
        let mut children: HashMap<NamedOrBlankNode, Vec<NamedOrBlankNode>> = HashMap::new();
        for quad in produced {
            let predicate = quad.predicate.as_str();
            if predicate == RDF_TYPE {
                if let Term::NamedNode(class) = &quad.object {
                    classes
                        .entry(quad.subject.clone())
                        .or_default()
                        .push(class.as_str());
                }
            } else if let Some(object) = as_subject(&quad.object) {
                if predicate == PROV_SPECIALIZATION_OF {
                    record_of.insert(quad.subject.clone(), object);
                } else if predicate == BRIDGE_ARRIVED_AS {
                    version_of.insert(quad.subject.clone(), object);
                } else {
                    children
                        .entry(quad.subject.clone())
                        .or_default()
                        .push(object);
                }
            }
        }
        let records: HashSet<&NamedOrBlankNode> = record_of.values().collect();
        let plain = |node: &NamedOrBlankNode| {
            !records.contains(node) && !record_of.contains_key(node) && !version_of.contains_key(node)
        };
        // Each node of a record, its versions, their arrivals and what they hold in nodes of no
        // record of their own, with the classes of the record it belongs to.
        let mut owner: HashMap<NamedOrBlankNode, HashSet<&str>> = HashMap::new();
        let mut pending: Vec<NamedOrBlankNode> = Vec::new();
        for (version, record) in &record_of {
            let Some(of) = classes.get(record) else {
                continue;
            };
            for node in [version, record] {
                owner.entry(node.clone()).or_default().extend(of.iter().copied());
                pending.push(node.clone());
            }
        }
        for (arrival, version) in &version_of {
            if let Some(of) = owner.get(version).cloned() {
                owner.entry(arrival.clone()).or_default().extend(of);
            }
        }
        while let Some(node) = pending.pop() {
            let of = owner[&node].clone();
            for child in children
                .get(&node)
                .into_iter()
                .flatten()
                .filter(|child| plain(child))
            {
                let held = owner.entry(child.clone()).or_default();
                let before = held.len();
                held.extend(of.iter().copied());
                if held.len() != before {
                    pending.push(child.clone());
                }
            }
        }
        let mut written: HashSet<(&str, &str)> = HashSet::new();
        for quad in produced {
            for class in owner.get(&quad.subject).into_iter().flatten() {
                written.insert((*class, quad.predicate.as_str()));
            }
        }
        let mut holds: HashMap<&str, bool> = HashMap::new();
        let mut values: HashSet<&str> = HashSet::new();
        let mut bodies: HashSet<&str> = HashSet::new();
        for quad in findings {
            if quad.predicate.as_str() == SH_VALUE {
                if let Term::Literal(value) = &quad.object {
                    values.insert(value.value());
                }
            } else if quad.predicate.as_str() == OA_HAS_BODY {
                if let Term::NamedNode(body) = &quad.object {
                    bodies.insert(body.as_str());
                }
            }
        }
        for occurrence in unit.occurrences() {
            let Some(forward) = self.forward.get(&occurrence.path) else {
                continue;
            };
            let mut applicable: Vec<&Carry> = Vec::new();
            for carry in &forward.carries {
                let applies = match &carry.record_type {
                    None => true,
                    Some(record_type) => *holds.entry(record_type.as_str()).or_insert_with(|| {
                        unit.text.contains(&format!("\"{record_type}\""))
                            || unit.text.contains(&format!("'{record_type}'"))
                    }),
                };
                if applies {
                    applicable.push(carry);
                }
            }
            let reported = values.contains(occurrence.path.as_str())
                || forward.gaps.iter().any(|gap| bodies.contains(gap.as_str()));
            let carried = applicable.iter().any(|carry| {
                written.contains(&(carry.class.as_str(), carry.property.as_str()))
            });
            if applicable.is_empty() {
                misses.push(ForwardMiss {
                    kind: ForwardKind::Unlisted,
                    record: record.selector.to_owned(),
                    path: occurrence.path.clone(),
                    wanted: forward
                        .carries
                        .iter()
                        .map(|carry| {
                            (
                                carry.record_type.clone().unwrap_or_default(),
                                carry.property.clone(),
                            )
                        })
                        .collect(),
                });
            } else if !carried && !reported {
                misses.push(ForwardMiss {
                    kind: ForwardKind::Missed,
                    record: record.selector.to_owned(),
                    path: occurrence.path.clone(),
                    wanted: applicable
                        .iter()
                        .map(|carry| (carry.class.clone(), carry.property.clone()))
                        .collect(),
                });
            }
        }
        misses
    }

    pub(crate) fn findings(
        &self,
        record: &Record<'_>,
        unit: &Unit,
        asked: Asked,
    ) -> Result<Vec<Quad>> {
        let mut findings = Vec::new();
        for occurrence in unit.occurrences() {
            if asked.census && !self.paths.contains(&occurrence.path) {
                findings.extend(annotation::unaccounted(
                    record,
                    &occurrence.path,
                    occurrence.within.as_deref(),
                    occurrence.count,
                )?);
            }
            if !asked.findings {
                continue;
            }
            for reported in self.reported.get(&occurrence.path).into_iter().flatten() {
                findings.extend(annotation::gap(
                    record,
                    &reported.gap,
                    &occurrence.path,
                    occurrence.within.as_deref(),
                    &reported.severity,
                    occurrence.count,
                )?);
            }
        }
        if !asked.findings {
            return Ok(findings);
        }
        // Sorted, so a run's output does not depend on which way a hash fell.
        let mut held: Vec<&Valued> = unit.values().iter().collect();
        held.sort_by(|one, two| (&one.path, &one.value).cmp(&(&two.path, &two.value)));
        for valued in held {
            let key = key(&valued.value);
            if key.is_empty() {
                continue;
            }
            for lookup in self
                .lookups
                .get(&valued.path)
                .into_iter()
                .flatten()
                .filter(|lookup| !lookup.notations.contains(&key))
            {
                findings.extend(annotation::lookup(
                    record,
                    &lookup.gap,
                    &valued.value,
                    valued.within.as_deref(),
                    &lookup.severity,
                    valued.count,
                )?);
            }
        }
        Ok(findings)
    }
}

fn declared(objects: &[Term], iri: &str, path: &str, predicate: &str) -> Result<Option<String>> {
    if let [first, second, ..] = objects {
        return Err(Error::adapter(format!(
            "{iri}: the entry for {path} declares the {predicate} {first} and {second}; an entry \
             declares at most one"
        )));
    }
    let Some(object) = objects.first() else {
        return Ok(None);
    };
    let Term::NamedNode(named) = object else {
        return Err(Error::adapter(format!("{iri}: {object} is no {predicate}")));
    };
    Ok(Some(named.as_str().to_owned()))
}

/// A `bridge:sourcePath` that is no literal is refused: dropping it would report the
/// path as unaccounted.
fn entries(resolver: &dyn Resolver, iri: &str) -> Result<Vec<Entry>> {
    let (graph, _) = turtle(&resolver.read(iri)?, iri)?;
    let typed = instances(&graph, BRIDGE_PATH_ENTRY)?;
    let mut entries = Vec::new();
    for s in subjects(&graph, BRIDGE_SOURCE_PATH)? {
        for path in objects(&graph, &s, BRIDGE_SOURCE_PATH)? {
            let Term::Literal(path) = path else {
                return Err(Error::adapter(format!("{iri}: {path} is no path")));
            };
            if !typed.contains(&s) {
                continue;
            }
            let path = path.value();
            let said = |predicate: &str, name: &str| {
                declared(&objects(&graph, &s, predicate)?, iri, path, name)
            };
            let mut carries = Vec::new();
            for carried in objects(&graph, &s, BRIDGE_CARRIED_AS)? {
                let Some(node) = as_subject(&carried) else {
                    continue;
                };
                let class = declared(
                    &objects(&graph, &node, BRIDGE_IN_RECORD_CLASS)?,
                    iri,
                    path,
                    "bridge:inRecordClass",
                )?;
                let property = declared(
                    &objects(&graph, &node, BRIDGE_AS_PROPERTY)?,
                    iri,
                    path,
                    "bridge:asProperty",
                )?;
                let record_type = match objects(&graph, &node, BRIDGE_IN_SOURCE_RECORD_TYPE)?
                    .first()
                {
                    Some(Term::Literal(record_type)) => Some(record_type.value().to_owned()),
                    _ => None,
                };
                if let (Some(class), Some(property)) = (class, property) {
                    carries.push(Carry {
                        record_type,
                        class,
                        property,
                    });
                }
            }
            entries.push(Entry {
                carries,
                verdict: said(BRIDGE_VERDICT, "verdict")?,
                gap: said(BRIDGE_NAMES_GAP, "gap")?,
                map: said(BRIDGE_LOOKUP_IN, "bridge:lookupIn")?,
                miss: said(BRIDGE_LOOKUP_NAMES_GAP, "bridge:lookupNamesGap")?,
                path: path.to_owned(),
            });
        }
    }
    Ok(entries)
}

pub(crate) fn gap_scheme(
    resolver: &dyn Resolver,
    iri: &str,
) -> Result<(HashMap<String, Gap>, Prefixes)> {
    let (graph, prefixes) = turtle(&resolver.read(iri)?, iri)?;
    let mut scheme: HashMap<String, Gap> = HashMap::new();
    for predicate in [SKOS_BROADER, SH_RESULT_SEVERITY] {
        for s in subjects(&graph, predicate)? {
            let NamedOrBlankNode::NamedNode(concept) = &s else {
                continue;
            };
            let mut declared = Vec::new();
            for object in objects(&graph, &s, predicate)? {
                let Term::NamedNode(object) = object else {
                    return Err(Error::adapter(format!(
                        "{iri}: {concept} declares {predicate} {object}, which is no IRI"
                    )));
                };
                declared.push(object);
            }
            if predicate == SH_RESULT_SEVERITY {
                if let Some(object) = declared
                    .iter()
                    .find(|object| !SEVERITIES.contains(&object.as_str()))
                {
                    return Err(Error::adapter(format!(
                        "{iri}: {concept} declares sh:resultSeverity {object}; a gap's severity \
                         is sh:Info, sh:Warning or sh:Violation"
                    )));
                }
            }
            let name = if predicate == SKOS_BROADER {
                "skos:broader"
            } else {
                "sh:resultSeverity"
            };
            if let [first, second, ..] = declared.as_slice() {
                return Err(Error::adapter(format!(
                    "{iri}: {concept} declares {name} {first} and {second}; a gap declares at \
                     most one"
                )));
            }
            let Some(one) = declared.first() else {
                continue;
            };
            let gap = scheme.entry(concept.as_str().to_owned()).or_default();
            if predicate == SKOS_BROADER {
                gap.kind = Some(one.as_str().to_owned());
            } else {
                gap.severity = Some(one.as_str().to_owned());
            }
        }
    }
    Ok((scheme, prefixes))
}

fn concept_map(resolver: &dyn Resolver, iri: &str) -> Result<HashSet<String>> {
    let (graph, _) = turtle(&resolver.read(iri)?, iri)?;
    let mut notations = HashSet::new();
    for s in subjects(&graph, SKOS_NOTATION)? {
        for notation in objects(&graph, &s, SKOS_NOTATION)? {
            let Term::Literal(notation) = notation else {
                return Err(Error::adapter(format!(
                    "{iri}: {notation} is no skos:notation"
                )));
            };
            notations.insert(notation.value().to_owned());
        }
    }
    let schemes = instances(&graph, SKOS_CONCEPT_SCHEME)?.len();
    if schemes != 1 {
        return Err(Error::adapter(format!(
            "{iri}: the file holds {schemes} concept schemes; a bridge:lookupIn names a file \
             holding one"
        )));
    }
    Ok(notations)
}
