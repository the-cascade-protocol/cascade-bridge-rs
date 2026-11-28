use crate::accounting::Decision;
use crate::load::Prefixes;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const BRIDGE: &str = "https://ns.cascadeprotocol.org/bridge/v1-draft#";

/// The verdict of a path no entry of the accounting covers.
pub const NEVER_DECIDED: &str = "neverDecided";
const NO_HOME: &str = "noHome";
const CARRIED_IN_PART: &str = "carriedInPart";
const IGNORED: &str = "ignored";

/// What names the bare form of a place in a row's forms.
const BARE: &str = "bare";

/// A place in a record kind, as written in a form: `form` is the prefix of the form, none for
/// the bare one.
pub(crate) struct Seen<'a> {
    pub(crate) kind: Option<String>,
    pub(crate) place: String,
    pub(crate) form: Option<String>,
    pub(crate) occurrences: usize,
    pub(crate) decision: Option<&'a Decision>,
}

/// What the entry at a path says; paths of one place with different sayings are different rows.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Said {
    verdict: String,
    gap: Option<String>,
    because: Option<String>,
}

impl Said {
    fn of(decision: Option<&Decision>) -> Self {
        let verdict = match decision {
            None => NEVER_DECIDED.to_owned(),
            Some(decision) => decision
                .verdict
                .as_deref()
                .map(|verdict| verdict.strip_prefix(BRIDGE).unwrap_or(verdict).to_owned())
                .unwrap_or_default(),
        };
        Self {
            verdict,
            gap: decision.and_then(|decision| decision.gap.clone()),
            because: decision.and_then(|decision| decision.because.clone()),
        }
    }
}

type Key = (Option<String>, String, Said);

#[derive(Default)]
struct Row {
    gap_label: Option<String>,
    documents: usize,
    occurrences: usize,
    forms: BTreeSet<Option<String>>,
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

/// What the documents of a folder, read one by one, hold at each place of a record, by the record
/// kind each stands in.
#[derive(Default)]
pub struct Survey {
    rows: BTreeMap<Key, Row>,
    read: usize,
    records: usize,
    not_read: Vec<NotRead>,
    notes: BTreeSet<String>,
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

    /// A line the report says about the survey as a whole.
    pub(crate) fn note(&mut self, line: &str) {
        self.notes.insert(line.to_owned());
    }

    pub(crate) fn read<'a>(
        &mut self,
        records: usize,
        seen: impl IntoIterator<Item = Seen<'a>>,
        prefixes: &Prefixes,
    ) {
        self.read += 1;
        self.records += records;
        let mut held: BTreeMap<Key, Row> = BTreeMap::new();
        for seen in seen {
            let said = Said::of(seen.decision);
            let gap_label = said.gap.as_deref().map(|gap| compacted(gap, prefixes));
            let row = held.entry((seen.kind, seen.place, said)).or_default();
            row.gap_label = gap_label;
            row.occurrences += seen.occurrences;
            row.forms.insert(seen.form);
        }
        for (key, held) in held {
            let row = self.rows.entry(key).or_default();
            row.gap_label = held.gap_label;
            row.documents += 1;
            row.occurrences += held.occurrences;
            row.forms.extend(held.forms);
        }
    }

    pub fn report(&self, narrowing: &Narrowing) -> SurveyReport {
        let mut rows: Vec<SurveyRow> = self
            .rows
            .iter()
            .filter(|((kind, place, _), _)| narrowing.holds(kind.as_deref(), place))
            .map(|((kind, place, said), row)| SurveyRow {
                kind: kind.clone(),
                path: place.clone(),
                verdict: said.verdict.clone(),
                gap: said.gap.clone(),
                gap_label: row.gap_label.clone(),
                because: said.because.clone(),
                documents: row.documents,
                occurrences: row.occurrences,
                forms: row
                    .forms
                    .iter()
                    .map(|form| form.clone().unwrap_or_else(|| BARE.to_owned()))
                    .collect(),
            })
            .collect();
        rows.sort_by_cached_key(|row| {
            (
                row.kind.is_none(),
                row.kind.clone(),
                Reverse(row.documents),
                Reverse(row.occurrences),
                row.path.clone(),
                row.verdict.clone(),
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
            notes: self.notes.iter().cloned().collect(),
        }
    }
}

/// Which rows a report keeps: those in one record kind, those at a place or below it, or both.
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

/// `path` is the place; `forms` the prefix of each form the place was written in, `bare` for a
/// place written with none.
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
    pub forms: Vec<String>,
}

impl SurveyRow {
    /// The forms the row says it was written in: none where it was written only in the bare one.
    fn shown_forms(&self) -> &[String] {
        match self.forms.as_slice() {
            [only] if only == BARE => &[],
            all => all,
        }
    }

    fn waits_on_a_term(&self) -> bool {
        self.verdict == NO_HOME || (self.verdict == CARRIED_IN_PART && self.gap.is_some())
    }

    fn left_out(&self) -> bool {
        self.verdict == IGNORED
    }
}

#[derive(Debug, Clone)]
pub struct SurveyReport {
    pub read: usize,
    pub records: usize,
    pub not_read: Vec<NotRead>,
    pub rows: Vec<SurveyRow>,
    narrowing: Narrowing,
    notes: Vec<String>,
}

type Section = (&'static str, &'static str, fn(&SurveyRow) -> bool);

const SECTIONS: [Section; 3] = [
    (
        "Never decided",
        "no entry of the accounting covers",
        |row| row.verdict == NEVER_DECIDED,
    ),
    (
        "No term yet",
        "whose entry says no Cascade term carries them",
        SurveyRow::waits_on_a_term,
    ),
    (
        "Left out on purpose",
        "whose entry says they are not carried",
        SurveyRow::left_out,
    ),
];

/// Text on one line of a report: a control character is a space, as in the TSV.
fn cell(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

impl SurveyReport {
    pub fn text(&self) -> String {
        let mut said = format!(
            "Documents  {} read, {} not read, {} record(s)\nColumns    documents, occurrences, path\nCounts     documents is how many files hold the path; occurrences is how many times it stands in all of them\n",
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
            let _ = writeln!(said, "Narrowed   {}", cell(&narrowed.join(", ")));
        }
        for note in &self.notes {
            let _ = writeln!(said, "Note       {note}");
        }
        if self.read == 0 {
            said.push_str("No document was read.\n");
        }
        let kinded = self.rows.iter().any(|row| row.kind.is_some());
        for (title, explains, holds) in SECTIONS {
            let rows: Vec<&SurveyRow> = self.rows.iter().filter(|row| holds(row)).collect();
            let _ = write!(said, "\n{title}: {} path(s) {explains}\n", rows.len());
            let indent = if kinded { "    " } else { "  " };
            let mut at = 0;
            let mut under: Option<&Option<String>> = None;
            while at < rows.len() {
                let first = rows[at];
                if kinded && under != Some(&first.kind) {
                    let _ = writeln!(said, "  {}", first.kind.as_deref().unwrap_or("(no kind)"));
                    under = Some(&first.kind);
                }
                let sharing = rows[at..]
                    .iter()
                    .take_while(|next| next.kind == first.kind && next.because == first.because)
                    .count();
                let above = first
                    .because
                    .as_ref()
                    .filter(|_| sharing > 1 && first.left_out());
                let (run, indent) = match above {
                    Some(because) => {
                        let _ = writeln!(said, "{indent}because {}", cell(because));
                        (sharing, format!("{indent}  "))
                    }
                    None => (1, indent.to_owned()),
                };
                for row in &rows[at..at + run] {
                    let _ = write!(
                        said,
                        "{indent}{:>8}  {:>8}  {}",
                        row.documents,
                        row.occurrences,
                        cell(&row.path)
                    );
                    if !row.shown_forms().is_empty() {
                        let _ = write!(said, "  forms {}", cell(&row.shown_forms().join(", ")));
                    }
                    if let Some(gap) = row.gap_label.as_ref().filter(|_| row.waits_on_a_term()) {
                        let _ = write!(said, "  gap {}", cell(gap));
                        if row.verdict == CARRIED_IN_PART {
                            said.push_str(" (carried in part)");
                        }
                    }
                    if let Some(because) = row
                        .because
                        .as_ref()
                        .filter(|_| above.is_none() && row.left_out())
                    {
                        let _ = write!(said, "  because {}", cell(because));
                    }
                    said.push('\n');
                }
                at += run;
            }
        }
        let _ = write!(said, "\nNot read: {} document(s)\n", self.not_read.len());
        for not_read in &self.not_read {
            let _ = writeln!(
                said,
                "  {}  {}",
                cell(&not_read.name),
                cell(&not_read.reason)
            );
        }
        said
    }

    pub fn tsv(&self) -> String {
        let mut written =
            String::from("kind\tpath\tverdict\tgap\treason\tdocuments\toccurrences\tforms\n");
        for row in &self.rows {
            let _ = writeln!(
                written,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                cell(row.kind.as_deref().unwrap_or("")),
                cell(&row.path),
                row.verdict,
                cell(row.gap.as_deref().unwrap_or("")),
                cell(row.because.as_deref().unwrap_or("")),
                row.documents,
                row.occurrences,
                cell(&row.shown_forms().join(","))
            );
        }
        written
    }
}
