use crate::annotation::{self, Record};
use crate::decode::XML_SPACE;
use crate::error::{Error, Result};
use crate::lift::{Paths, Unit, Valued};
use crate::load::{as_subject, instances, objects, subjects, turtle, Envelope, Prefixes};
use crate::resolver::Resolver;
use crate::run::Asked;
use crate::terms::{
    BRIDGE_ADAPTER_REPORTS_GAP, BRIDGE_AS_PROPERTY, BRIDGE_BECAUSE, BRIDGE_CARRIED,
    BRIDGE_CARRIED_AS, BRIDGE_CARRIED_IN_PART, BRIDGE_ELEMENT_NOT_CARRIED, BRIDGE_IN_RECORD_CLASS,
    BRIDGE_LOOKUP_IN, BRIDGE_LOOKUP_NAMES_GAP, BRIDGE_NAMES_GAP, BRIDGE_NO_HOME,
    BRIDGE_NO_PREDICATE, BRIDGE_PATH_ENTRY, BRIDGE_SOURCE_LACKS_REQUIRED, BRIDGE_SOURCE_PATH,
    BRIDGE_VALUE_NOT_MAPPED, BRIDGE_VERDICT, SH_INFO, SH_RESULT_SEVERITY, SH_VIOLATION, SH_WARNING,
    SKOS_BROADER, SKOS_CONCEPT_SCHEME, SKOS_NOTATION,
};
use oxigraph::model::{NamedOrBlankNode, Quad, Term};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

mod owed;

pub(crate) use owed::{same_place, Unmet};

#[derive(Clone)]
struct Entry {
    path: String,
    verdict: Option<String>,
    gap: Option<String>,
    /// The concept map a value is looked up in; `miss` is the gap for a value it lacks.
    map: Option<String>,
    miss: Option<String>,
    carries: Vec<Carry>,
    /// Each `bridge:adapterReportsGap`.
    reported_by_the_adapter: Vec<String>,
    because: Option<String>,
}

/// What an entry says of the path it is written for, as the entry writes it.
pub(crate) struct Decision {
    pub(crate) verdict: Option<String>,
    pub(crate) gap: Option<String>,
    pub(crate) because: Option<String>,
}

struct Accounted {
    /// The path of the entry that says so.
    written_for: String,
    decision: Decision,
}

/// One `bridge:carriedAs`.
#[derive(Clone)]
struct Carry {
    class: String,
    property: String,
}

/// What an entry that says its path is carried says a record written from it holds:
/// one of `carries`, or else a finding naming one of `gaps`; and, where no record of a class
/// `carries` names holds an occurrence, a finding of one of `reported`, the gaps its adapter
/// reports itself, and nothing else.
#[derive(Default)]
struct Owed {
    carries: Vec<Carry>,
    gaps: Vec<String>,
    reported: Vec<String>,
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

/// A form an envelope writes a place in: a content prefix, possibly empty, followed or not by
/// the contained prefix. Its `prefix` is never empty.
struct Form {
    prefix: String,
    content: String,
    contained: Option<String>,
}

/// The non-empty forms an adapter's envelopes write a place in, longest first.
pub(crate) struct Forms(Vec<Form>);

impl Forms {
    pub(crate) fn declared(envelopes: &[Envelope]) -> Self {
        let mut forms: Vec<Form> = Vec::new();
        for envelope in envelopes {
            let content = envelope.path_of_content_in_record.as_deref().unwrap_or("");
            let contained = envelope.path_of_contained_content_below_content.as_deref();
            let wholes = [(content.to_owned(), None)]
                .into_iter()
                .chain(contained.map(|c| (format!("{content}{c}"), Some(c.to_owned()))));
            for (prefix, contained) in wholes {
                if !prefix.is_empty() && !forms.iter().any(|form| form.prefix == prefix) {
                    forms.push(Form {
                        prefix,
                        content: content.to_owned(),
                        contained,
                    });
                }
            }
        }
        forms.sort_by(|one, other| {
            (other.prefix.len(), &one.prefix).cmp(&(one.prefix.len(), &other.prefix))
        });
        Self(forms)
    }

    /// The form a path is written in: the longest prefix a `/` follows.
    fn of(&self, path: &str) -> Option<&Form> {
        self.0.iter().find(|form| {
            path.strip_prefix(form.prefix.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    /// Each entry copied to the forms it stands for, and the path of the entry each copy was
    /// made from. The most specific entry wins at a path: one standing there; then one
    /// written with the contained prefix and no content prefix, which stands for its place in
    /// the contained form under every content prefix; then the place's own entry, which stands
    /// for it in every form. An entry written with a content prefix stands for itself only.
    fn copied(&self, entries: Vec<Entry>) -> (Vec<Entry>, HashMap<String, String>) {
        let standing: HashSet<String> = entries.iter().map(|entry| entry.path.clone()).collect();
        let mut made = Vec::new();
        let mut from: HashMap<String, String> = HashMap::new();
        let mut overridden: HashSet<String> = HashSet::new();
        for entry in &entries {
            let Some(written) = self.of(&entry.path).filter(|form| form.content.is_empty()) else {
                continue;
            };
            let place = &entry.path[written.prefix.len()..];
            for form in self
                .0
                .iter()
                .filter(|form| !form.content.is_empty() && form.contained == written.contained)
            {
                let path = format!("{}{place}", form.prefix);
                if standing.contains(&path) {
                    continue;
                }
                overridden.insert(path.clone());
                from.insert(path.clone(), entry.path.clone());
                made.push(Entry {
                    path,
                    ..entry.clone()
                });
            }
        }
        for entry in entries
            .iter()
            .filter(|entry| self.of(&entry.path).is_none())
        {
            for form in &self.0 {
                let path = format!("{}{}", form.prefix, entry.path);
                if standing.contains(&path)
                    || overridden.contains(&path)
                    || self.of(&path).map(|read| &read.prefix) != Some(&form.prefix)
                {
                    continue;
                }
                from.insert(path.clone(), entry.path.clone());
                made.push(Entry {
                    path,
                    ..entry.clone()
                });
            }
        }
        let mut entries = entries;
        entries.extend(made);
        (entries, from)
    }
}

pub(crate) struct Accounting {
    /// Each path an entry accounts for, with the entry that says so.
    accounted_by: HashMap<String, Accounted>,
    /// The prefixes the accounting's file binds.
    pub(crate) prefixes: Prefixes,
    reported: HashMap<String, Vec<Reported>>,
    lookups: HashMap<String, Vec<Lookup>>,
    owed: HashMap<String, Owed>,
    /// Whether an occurrence is owed even in a unit no record was written from.
    owes_where_nothing_is_written: bool,
    /// Each gap the adapter declares of `bridge:elementNotCarried`.
    not_carried_whole: HashSet<String>,
}

impl Accounting {
    pub(crate) fn read(
        resolver: &dyn Resolver,
        iri: &str,
        scheme: &HashMap<String, Gap>,
        forms: &Forms,
    ) -> Result<Self> {
        let (entries, prefixes) = entries(resolver, iri)?;
        let (entries, copied_from) = forms.copied(entries);
        Self::of(entries, prefixes, copied_from, scheme, iri, resolver)
    }

    /// The path of the entry that accounts for `path` as a record writes it: its own, or
    /// the entry written for its place.
    pub(crate) fn entry_for(&self, path: &str) -> Option<&str> {
        self.accounted_by
            .get(path)
            .map(|accounted| accounted.written_for.as_str())
    }

    /// What the entry that `entry_for` finds says.
    pub(crate) fn decision_for(&self, path: &str) -> Option<&Decision> {
        self.accounted_by
            .get(path)
            .map(|accounted| &accounted.decision)
    }

    /// `owing` keeps where each occurrence of a carried path stands, which only
    /// `Accounting::unmet` reads.
    pub(crate) fn paths(&self, owing: bool) -> Paths {
        Paths::Kept {
            valued: self.lookups.keys().cloned().collect(),
            carried: match owing {
                true => self.owed.keys().cloned().collect(),
                false => HashSet::new(),
            },
            kinds: false,
        }
    }

    fn of(
        entries: Vec<Entry>,
        prefixes: Prefixes,
        copied_from: HashMap<String, String>,
        scheme: &HashMap<String, Gap>,
        iri: &str,
        resolver: &dyn Resolver,
    ) -> Result<Self> {
        let mut accounted_by = HashMap::new();
        let mut reported: HashMap<String, Vec<Reported>> = HashMap::new();
        let mut lookups: HashMap<String, Vec<Lookup>> = HashMap::new();
        let mut maps: HashMap<String, Arc<HashSet<String>>> = HashMap::new();
        let mut owed: HashMap<String, Owed> = HashMap::new();
        for entry in entries {
            let carrying = matches!(
                entry.verdict.as_deref(),
                Some(BRIDGE_CARRIED | BRIDGE_CARRIED_IN_PART)
            );
            let carries: &[Carry] = if carrying { &entry.carries } else { &[] };
            let adapter_reports = &entry.reported_by_the_adapter;
            if !carries.is_empty() || !adapter_reports.is_empty() {
                let owing = owed.entry(entry.path.clone()).or_default();
                owing.carries.extend(carries.iter().cloned());
                owing.gaps.extend(entry.gap.iter().cloned());
                owing.gaps.extend(entry.miss.iter().cloned());
                owing.reported.extend(adapter_reports.iter().cloned());
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
            let written_for = copied_from.get(&entry.path).unwrap_or(&entry.path).clone();
            accounted_by.insert(
                entry.path,
                Accounted {
                    written_for,
                    decision: Decision {
                        verdict: entry.verdict,
                        gap: entry.gap,
                        because: entry.because,
                    },
                },
            );
        }
        // Sorted, so a run's output does not depend on which way a hash fell.
        for gaps in reported.values_mut() {
            gaps.sort_by(|one, two| one.gap.cmp(&two.gap));
        }
        for found in lookups.values_mut() {
            found.sort_by(|one, two| one.gap.cmp(&two.gap));
        }
        for owing in owed.values_mut() {
            owing.reported.sort();
            owing.reported.dedup();
        }
        let owes_where_nothing_is_written = owed.values().any(|owing| !owing.reported.is_empty());
        Ok(Self {
            accounted_by,
            prefixes,
            reported,
            lookups,
            owed,
            owes_where_nothing_is_written,
            not_carried_whole: scheme
                .iter()
                .filter(|(_, gap)| gap.kind.as_deref() == Some(BRIDGE_ELEMENT_NOT_CARRIED))
                .map(|(concept, _)| concept.clone())
                .collect(),
        })
    }

    pub(crate) fn findings(
        &self,
        record: &Record<'_>,
        unit: &Unit,
        asked: Asked,
    ) -> Result<Vec<Quad>> {
        let mut findings = Vec::new();
        for occurrence in unit.occurrences() {
            if asked.census && self.entry_for(&occurrence.path).is_none() {
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
fn entries(resolver: &dyn Resolver, iri: &str) -> Result<(Vec<Entry>, Prefixes)> {
    let (graph, prefixes) = turtle(&resolver.read(iri)?, iri)?;
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
                    return Err(Error::adapter(format!(
                        "{iri}: the entry for {path} declares the bridge:carriedAs {carried}, \
                         which is no node"
                    )));
                };
                let of = |predicate: &str, name: &str| {
                    declared(&objects(&graph, &node, predicate)?, iri, path, name)
                };
                if let (Some(class), Some(property)) = (
                    of(BRIDGE_IN_RECORD_CLASS, "bridge:inRecordClass")?,
                    of(BRIDGE_AS_PROPERTY, "bridge:asProperty")?,
                ) {
                    carries.push(Carry { class, property });
                }
            }
            let mut reported_by_the_adapter = Vec::new();
            for gap in objects(&graph, &s, BRIDGE_ADAPTER_REPORTS_GAP)? {
                let Term::NamedNode(named) = &gap else {
                    return Err(Error::adapter(format!(
                        "{iri}: the entry for {path} declares the bridge:adapterReportsGap {gap}, \
                         which is no IRI"
                    )));
                };
                reported_by_the_adapter.push(named.as_str().to_owned());
            }
            let reasons: Vec<String> = objects(&graph, &s, BRIDGE_BECAUSE)?
                .iter()
                .filter_map(|reason| match reason {
                    Term::Literal(reason) => Some(reason.value().to_owned()),
                    _ => None,
                })
                .collect();
            entries.push(Entry {
                carries,
                reported_by_the_adapter,
                verdict: said(BRIDGE_VERDICT, "verdict")?,
                gap: said(BRIDGE_NAMES_GAP, "gap")?,
                map: said(BRIDGE_LOOKUP_IN, "bridge:lookupIn")?,
                miss: said(BRIDGE_LOOKUP_NAMES_GAP, "bridge:lookupNamesGap")?,
                path: path.to_owned(),
                because: (!reasons.is_empty()).then(|| reasons.join(" ")),
            });
        }
    }
    Ok((entries, prefixes))
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
