use crate::accounting::Decision;
use crate::load::Prefixes;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write as _;

const BRIDGE: &str = "https://ns.cascadeprotocol.org/bridge/v1-draft#";

/// The verdict of a path no entry of the accounting covers.
pub const NEVER_DECIDED: &str = "neverDecided";
const NO_HOME: &str = "noHome";
const IGNORED: &str = "ignored";

/// A path in the record kind it stands in.
pub(crate) type Place = (Option<String>, String);

pub(crate) struct Seen<'a> {
    pub(crate) place: Place,
    pub(crate) occurrences: usize,
    pub(crate) decision: Option<&'a Decision>,
}

struct Row {
    verdict: String,
    gap: Option<String>,
    gap_label: Option<String>,
    because: Option<String>,
    documents: usize,
    occurrences: usize,
}

impl Row {
    fn of(decision: Option<&Decision>, prefixes: &Prefixes) -> Self {
        let verdict = match decision {
            None => NEVER_DECIDED.to_owned(),
            Some(decision) => decision
                .verdict
                .as_deref()
                .map(|verdict| verdict.strip_prefix(BRIDGE).unwrap_or(verdict).to_owned())
                .unwrap_or_default(),
        };
        let gap = decision.and_then(|decision| decision.gap.clone());
        Self {
            verdict,
            gap_label: gap.as_deref().map(|gap| compacted(gap, prefixes)),
            gap,
            because: decision.and_then(|decision| decision.because.clone()),
            documents: 0,
            occurrences: 0,
        }
    }
}

/// An IRI under a prefix the accounting binds, written with it.
fn compacted(iri: &str, prefixes: &Prefixes) -> String {
    prefixes
        .iter()
        .filter_map(|(name, namespace)| {
            let local = iri.strip_prefix(namespace.as_str())?;
            let plain = !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
            plain.then(|| (namespace.len(), format!("{name}:{local}")))
        })
        .max_by_key(|(length, _)| *length)
        .map_or_else(|| iri.to_owned(), |(_, compact)| compact)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRead {
    pub name: String,
    pub reason: String,
}

/// What the documents of a folder, read one by one, hold at each path of a record, by the record
/// kind each stands in.
#[derive(Default)]
pub struct Survey {
    rows: BTreeMap<Place, Row>,
    read: usize,
    records: usize,
    not_read: Vec<NotRead>,
}

impl Survey {
    pub fn new() -> Self {
        Self::default()
    }

    /// A document that could not be read, by the name it goes by, and why.
    pub fn not_read(&mut self, name: &str, reason: &str) {
        self.not_read.push(NotRead {
            name: name.to_owned(),
            reason: reason.to_owned(),
        });
    }

    pub(crate) fn read<'a>(
        &mut self,
        records: usize,
        seen: impl IntoIterator<Item = Seen<'a>>,
        prefixes: &Prefixes,
    ) {
        self.read += 1;
        self.records += records;
        for seen in seen {
            let row = self
                .rows
                .entry(seen.place)
                .or_insert_with(|| Row::of(seen.decision, prefixes));
            row.documents += 1;
            row.occurrences += seen.occurrences;
        }
    }

    pub fn report(&self, narrowing: &Narrowing) -> SurveyReport {
        let mut rows: Vec<SurveyRow> = self
            .rows
            .iter()
            .filter(|((kind, path), _)| narrowing.holds(kind.as_deref(), path))
            .map(|((kind, path), row)| SurveyRow {
                kind: kind.clone(),
                path: path.clone(),
                verdict: row.verdict.clone(),
                gap: row.gap.clone(),
                gap_label: row.gap_label.clone(),
                because: row.because.clone(),
                documents: row.documents,
                occurrences: row.occurrences,
            })
            .collect();
        rows.sort_by_cached_key(|row| {
            (
                row.kind.is_none(),
                row.kind.clone(),
                Reverse(row.documents),
                Reverse(row.occurrences),
                row.path.clone(),
            )
        });
        let mut not_read = self.not_read.clone();
        not_read.sort_by(|one, other| one.name.cmp(&other.name));
        SurveyReport {
            read: self.read,
            records: self.records,
            not_read,
            rows,
            narrowing: narrowing.clone(),
        }
    }
}

/// Which rows a report keeps: those in one record kind, those at a path or below it, or both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Narrowing {
    pub kind: Option<String>,
    pub under: Option<String>,
}

impl Narrowing {
    fn holds(&self, kind: Option<&str>, path: &str) -> bool {
        self.kind
            .as_deref()
            .is_none_or(|wanted| kind == Some(wanted))
            && self.under.as_deref().is_none_or(|under| {
                let under = under.trim_end_matches('/');
                path.strip_prefix(under)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyRow {
    pub kind: Option<String>,
    pub path: String,
    /// The local name of the entry's `bridge:verdict`, or `neverDecided` where no entry covers the path.
    pub verdict: String,
    pub gap: Option<String>,
    pub gap_label: Option<String>,
    pub because: Option<String>,
    pub documents: usize,
    pub occurrences: usize,
}

#[derive(Debug, Clone)]
pub struct SurveyReport {
    pub read: usize,
    pub records: usize,
    pub not_read: Vec<NotRead>,
    pub rows: Vec<SurveyRow>,
    narrowing: Narrowing,
}

const SECTIONS: [(&str, &str, &str); 3] = [
    (
        NEVER_DECIDED,
        "Never decided",
        "no entry of the accounting covers",
    ),
    (
        NO_HOME,
        "No term yet",
        "whose entry says no Cascade term carries them",
    ),
    (
        IGNORED,
        "Left out on purpose",
        "whose entry says they are not carried",
    ),
];

fn cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

impl SurveyReport {
    pub fn text(&self) -> String {
        let mut said = format!(
            "Documents  {} read, {} not read, {} record(s)\nColumns    documents, occurrences, path\n",
            self.read,
            self.not_read.len(),
            self.records
        );
        let narrowed: Vec<String> = [
            self.narrowing
                .kind
                .as_ref()
                .map(|kind| format!("kind {kind}")),
            self.narrowing
                .under
                .as_ref()
                .map(|under| format!("under {under}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !narrowed.is_empty() {
            let _ = writeln!(said, "Narrowed   {}", narrowed.join(", "));
        }
        let kinded = self.rows.iter().any(|row| row.kind.is_some());
        for (verdict, title, explains) in SECTIONS {
            let rows: Vec<&SurveyRow> = self
                .rows
                .iter()
                .filter(|row| row.verdict == verdict)
                .collect();
            let _ = write!(said, "\n{title}: {} path(s) {explains}\n", rows.len());
            let mut under: Option<&Option<String>> = None;
            for row in rows {
                if kinded && under != Some(&row.kind) {
                    let _ = writeln!(said, "  {}", row.kind.as_deref().unwrap_or("(no kind)"));
                    under = Some(&row.kind);
                }
                let indent = if kinded { "    " } else { "  " };
                let _ = write!(
                    said,
                    "{indent}{:>8}  {:>8}  {}",
                    row.documents, row.occurrences, row.path
                );
                match (verdict, &row.gap_label, &row.because) {
                    (NO_HOME, Some(gap), _) => {
                        let _ = write!(said, "  gap {gap}");
                    }
                    (IGNORED, _, Some(because)) => {
                        let _ = write!(said, "  because {because}");
                    }
                    _ => {}
                }
                said.push('\n');
            }
        }
        let _ = write!(said, "\nNot read: {} document(s)\n", self.not_read.len());
        for not_read in &self.not_read {
            let _ = writeln!(said, "  {}  {}", not_read.name, not_read.reason);
        }
        said
    }

    pub fn tsv(&self) -> String {
        let mut written =
            String::from("kind\tpath\tverdict\tgap\treason\tdocuments\toccurrences\n");
        for row in &self.rows {
            let _ = writeln!(
                written,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                cell(row.kind.as_deref().unwrap_or("")),
                cell(&row.path),
                row.verdict,
                cell(row.gap.as_deref().unwrap_or("")),
                cell(row.because.as_deref().unwrap_or("")),
                row.documents,
                row.occurrences
            );
        }
        written
    }
}
