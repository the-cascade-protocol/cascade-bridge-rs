use crate::annotation::SelectorType;
use crate::error::Result;
use crate::terms::{
    CASCADE_OCCURRENCE_COUNT, OA_ANNOTATION, OA_HAS_BODY, OA_HAS_SELECTOR, OA_HAS_TARGET,
    OA_REFINED_BY, RDF_TYPE, RDF_VALUE, SH_FOCUS_NODE, SH_RESULT_PATH, SH_RESULT_SEVERITY,
    SH_VALUE,
};
use oxrdf::vocab::xsd;
use oxrdf::{BlankNode, GraphName, Literal, NamedNode, NamedOrBlankNode, Quad, Term};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Write as _};

const COLUMNS: [&str; 4] = ["?body", "?record", "?at", "?value"];
const COUNT_COLUMN: &str = "?count";

/// What a finding says that a test about it compares.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Row {
    pub(crate) body: String,
    pub(crate) record: String,
    pub(crate) at: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) count: Option<u64>,
}

impl fmt::Display for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}> at record {:?}", self.body, self.record)?;
        match self.detail() {
            Some(detail) => write!(f, ", {detail}"),
            None => Ok(()),
        }
    }
}

impl Row {
    /// What the row says beyond its body and record.
    fn detail(&self) -> Option<String> {
        let parts: Vec<String> = [
            self.at.as_ref().map(|at| format!("within {at:?}")),
            self.value.as_ref().map(|value| format!("value {value:?}")),
            self.count.map(|count| format!("count {count}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// A finding read from its quads: the row, where the finding can be said as one, and what
/// else it carries.
pub(crate) struct Finding {
    pub(crate) body: String,
    pub(crate) severity: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) focus: Option<String>,
    pub(crate) row: std::result::Result<Row, String>,
}

type Description<'a> = HashMap<&'a NamedOrBlankNode, Vec<(&'a str, &'a Term)>>;

fn described(quads: &[Quad]) -> Description<'_> {
    let mut description: Description<'_> = HashMap::new();
    for quad in quads {
        description
            .entry(&quad.subject)
            .or_default()
            .push((quad.predicate.as_str(), &quad.object));
    }
    description
}

fn objects_of<'a>(description: &Description<'a>, node: &Term, predicate: &str) -> Vec<&'a Term> {
    let subject = match node {
        Term::NamedNode(named) => NamedOrBlankNode::from(named.clone()),
        Term::BlankNode(blank) => NamedOrBlankNode::from(blank.clone()),
        _ => return Vec::new(),
    };
    description
        .get(&subject)
        .into_iter()
        .flatten()
        .filter(|(said, _)| *said == predicate)
        .map(|(_, object)| *object)
        .collect()
}

fn text(term: &Term) -> String {
    match term {
        Term::NamedNode(named) => named.as_str().to_owned(),
        Term::BlankNode(blank) => blank.as_str().to_owned(),
        Term::Literal(literal) => literal.value().to_owned(),
        Term::Triple(triple) => triple.to_string(),
    }
}

fn at_most_one<'a>(
    these: &[&'a Term],
    what: &str,
) -> std::result::Result<Option<&'a Term>, String> {
    match these {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        several => Err(format!("it has {} {what}", several.len())),
    }
}

fn exactly_one<'a>(these: &[&'a Term], what: &str) -> std::result::Result<&'a Term, String> {
    at_most_one(these, what)?.ok_or_else(|| format!("it has no {what}"))
}

/// Each finding of the quads, in the order its annotation is first met.
pub(crate) fn read(quads: &[Quad]) -> Vec<Finding> {
    let description = described(quads);
    let mut met: Vec<&NamedOrBlankNode> = Vec::new();
    for quad in quads {
        let annotation = quad.predicate.as_str() == RDF_TYPE
            && matches!(&quad.object, Term::NamedNode(n) if n.as_str() == OA_ANNOTATION);
        if annotation && !met.contains(&&quad.subject) {
            met.push(&quad.subject);
        }
    }
    met.into_iter()
        .map(|annotation| {
            let node = match annotation {
                NamedOrBlankNode::NamedNode(named) => Term::from(named.clone()),
                NamedOrBlankNode::BlankNode(blank) => Term::from(blank.clone()),
            };
            finding(&description, &node)
        })
        .collect()
}

fn finding(description: &Description<'_>, annotation: &Term) -> Finding {
    let said = |predicate: &str| objects_of(description, annotation, predicate);
    let kept = |predicate: &str| -> Option<String> {
        at_most_one(&said(predicate), predicate)
            .ok()
            .flatten()
            .map(text)
    };
    let row = row_of(description, annotation);
    Finding {
        body: kept(OA_HAS_BODY).unwrap_or_default(),
        severity: kept(SH_RESULT_SEVERITY),
        value: kept(SH_VALUE),
        path: kept(SH_RESULT_PATH),
        focus: kept(SH_FOCUS_NODE),
        row: row.map_err(|why| format!("a finding that cannot be said as a row: {why}")),
    }
}

fn row_of(description: &Description<'_>, annotation: &Term) -> std::result::Result<Row, String> {
    let said = |node: &Term, predicate: &str| objects_of(description, node, predicate);
    let body = text(exactly_one(&said(annotation, OA_HAS_BODY), "oa:hasBody")?);
    let target = exactly_one(&said(annotation, OA_HAS_TARGET), "oa:hasTarget")?;
    let selector = exactly_one(&said(target, OA_HAS_SELECTOR), "oa:hasSelector")?;
    let record = text(exactly_one(
        &said(selector, RDF_VALUE),
        "rdf:value on its selector",
    )?);
    let at = match at_most_one(&said(selector, OA_REFINED_BY), "oa:refinedBy")? {
        None => None,
        Some(refinement) => Some(text(exactly_one(
            &said(refinement, RDF_VALUE),
            "rdf:value on its refinement",
        )?)),
    };
    let value = at_most_one(&said(annotation, SH_VALUE), "sh:value")?.map(text);
    let count = match at_most_one(
        &said(annotation, CASCADE_OCCURRENCE_COUNT),
        "cascade:occurrenceCount",
    )?
    .map(text)
    {
        None => None,
        Some(written) => Some(
            written
                .parse::<u64>()
                .map_err(|_| format!("its cascade:occurrenceCount {written:?} is no count"))?,
        ),
    };
    Ok(Row {
        body,
        record,
        at,
        value,
        count,
    })
}

/// The rows of findings, each finding sayable as one; otherwise why not.
pub(crate) fn rows_of(findings: &[Finding]) -> std::result::Result<Vec<Row>, String> {
    findings.iter().map(|one| one.row.clone()).collect()
}

/// The findings the rows say, in the form a conversion writes them, so that the same
/// respelling of addresses reads both.
pub(crate) fn as_findings(rows: &[Row], selectors: SelectorType) -> Result<Vec<Quad>> {
    let about = |subject: &BlankNode, predicate: &str, object: Term| -> Result<Quad> {
        Ok(Quad::new(
            subject.clone(),
            NamedNode::new(predicate)?,
            object,
            GraphName::DefaultGraph,
        ))
    };
    let mut quads = Vec::new();
    for row in rows {
        let (annotation, target) = (BlankNode::default(), BlankNode::default());
        let selector = selectors.write_as(BlankNode::default(), &row.record, &mut quads)?;
        if let Some(at) = &row.at {
            let refinement = selectors.write_as(BlankNode::default(), at, &mut quads)?;
            quads.push(about(&selector, OA_REFINED_BY, Term::from(refinement))?);
        }
        let body = Term::from(NamedNode::new(&row.body)?);
        quads.push(about(
            &annotation,
            RDF_TYPE,
            Term::from(NamedNode::new(OA_ANNOTATION)?),
        )?);
        quads.push(about(
            &annotation,
            OA_HAS_TARGET,
            Term::from(target.clone()),
        )?);
        quads.push(about(&target, OA_HAS_SELECTOR, Term::from(selector))?);
        quads.push(about(&annotation, OA_HAS_BODY, body)?);
        if let Some(value) = &row.value {
            let value = Term::from(Literal::new_simple_literal(value));
            quads.push(about(&annotation, SH_VALUE, value)?);
        }
        if let Some(count) = row.count {
            let count = Literal::new_typed_literal(count.to_string(), xsd::INTEGER);
            quads.push(about(
                &annotation,
                CASCADE_OCCURRENCE_COUNT,
                Term::from(count),
            )?);
        }
    }
    Ok(quads)
}

fn unescaped(cell: &str) -> std::result::Result<String, String> {
    let inner = cell
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .filter(|_| cell.len() >= 2)
        .ok_or_else(|| format!("{cell:?} is not a quoted string"))?;
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Err(format!("{cell:?} holds a quote that is not written \\\"")),
            '\\' => {
                let escaped = chars
                    .next()
                    .ok_or_else(|| format!("{cell:?} ends in a lone backslash"))?;
                let digits = match escaped {
                    '\\' | '"' | '\'' => {
                        text.push(escaped);
                        continue;
                    }
                    't' => '\t',
                    'n' => '\n',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'u' | 'U' => {
                        let length = if escaped == 'u' { 4 } else { 8 };
                        let hex: String = chars.by_ref().take(length).collect();
                        let decoded = u32::from_str_radix(&hex, 16)
                            .ok()
                            .filter(|_| hex.len() == length)
                            .and_then(char::from_u32)
                            .ok_or_else(|| {
                                format!("{cell:?} holds \\{escaped}{hex}, no character")
                            })?;
                        text.push(decoded);
                        continue;
                    }
                    other => return Err(format!("{cell:?} holds the escape \\{other}")),
                };
                text.push(digits);
            }
            c => text.push(c),
        }
    }
    Ok(text)
}

fn escaped(text: &str) -> String {
    let mut cell = String::from('"');
    for c in text.chars() {
        match c {
            '\\' => cell.push_str("\\\\"),
            '"' => cell.push_str("\\\""),
            '\t' => cell.push_str("\\t"),
            '\n' => cell.push_str("\\n"),
            '\r' => cell.push_str("\\r"),
            c => cell.push(c),
        }
    }
    cell.push('"');
    cell
}

fn header(with_count: bool) -> String {
    let mut columns = COLUMNS.to_vec();
    if with_count {
        columns.push(COUNT_COLUMN);
    }
    columns.join("\t")
}

/// The rows a file holds; otherwise the line that cannot be read and why.
pub(crate) fn parse(text: &str) -> std::result::Result<Vec<Row>, String> {
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let first = lines.next().unwrap_or_default();
    let with_count = first == header(true);
    if !with_count && first != header(false) {
        return Err(format!(
            "line 1 is {first:?}, where the header is {:?}, with {COUNT_COLUMN:?} after it where a row says a count",
            header(false)
        ));
    }
    let width = COLUMNS.len() + usize::from(with_count);
    let mut lines: Vec<(usize, &str)> = lines.enumerate().map(|(n, line)| (n + 2, line)).collect();
    if lines.last().is_some_and(|(_, last)| last.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|(number, line)| {
            let cells: Vec<&str> = line.split('\t').collect();
            if cells.len() != width {
                return Err(format!(
                    "line {number} has {} cell(s) separated by tabs, where the header has {width}",
                    cells.len()
                ));
            }
            let said = |why: String| format!("line {number}: {why}");
            let body = cells[0]
                .strip_prefix('<')
                .and_then(|rest| rest.strip_suffix('>'))
                .filter(|iri| !iri.is_empty() && !iri.contains(['<', '>', '"', ' ', '\t']))
                .ok_or_else(|| said(format!("{:?} is not an IRI in angle brackets", cells[0])))?;
            let optional = |cell: &str| {
                if cell.is_empty() {
                    Ok(None)
                } else {
                    unescaped(cell).map(Some).map_err(said)
                }
            };
            let count = match cells.get(4).copied().unwrap_or_default() {
                "" => None,
                written => Some(
                    written
                        .parse::<u64>()
                        .ok()
                        .filter(|count| *count > 0 && written.chars().all(|c| c.is_ascii_digit()))
                        .ok_or_else(|| said(format!("{written:?} is no count")))?,
                ),
            };
            Ok(Row {
                body: body.to_owned(),
                record: unescaped(cells[1]).map_err(said)?,
                at: optional(cells[2])?,
                value: optional(cells[3])?,
                count,
            })
        })
        .collect()
}

/// The file the rows are written as, sorted so that a rewrite changes only what changed.
pub(crate) fn write(rows: &[Row]) -> String {
    let mut sorted = rows.to_vec();
    sorted.sort();
    let with_count = sorted.iter().any(|row| row.count.is_some());
    let mut file = header(with_count);
    file.push('\n');
    for row in &sorted {
        let optional = |cell: &Option<String>| cell.as_deref().map(escaped).unwrap_or_default();
        let _ = write!(
            file,
            "<{}>\t{}\t{}\t{}",
            row.body,
            escaped(&row.record),
            optional(&row.at),
            optional(&row.value)
        );
        if with_count {
            let _ = write!(
                file,
                "\t{}",
                row.count.map(|c| c.to_string()).unwrap_or_default()
            );
        }
        file.push('\n');
    }
    file
}

/// Rows the file wants and the conversion did not write, and rows it wrote and the file did
/// not want: a row written twice is two rows.
pub(crate) struct Difference {
    pub(crate) missing: Vec<Row>,
    pub(crate) extra: Vec<Row>,
}

pub(crate) fn difference(wanted: &[Row], got: &[Row]) -> Difference {
    let beyond = |these: &[Row], those: &[Row]| {
        let mut spare: BTreeMap<&Row, usize> = BTreeMap::new();
        for one in those {
            *spare.entry(one).or_default() += 1;
        }
        these
            .iter()
            .filter(|one| match spare.get_mut(one) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    false
                }
                _ => true,
            })
            .cloned()
            .collect::<Vec<Row>>()
    };
    Difference {
        missing: beyond(wanted, got),
        extra: beyond(got, wanted),
    }
}

impl Difference {
    pub(crate) fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty()
    }

    /// In the words of the rows themselves: for a missing row, what the output holds at its
    /// record and body; for a row both have, how many times each does.
    pub(crate) fn said(&self, wanted: &[Row], got: &[Row]) -> String {
        const SAID: usize = 3;
        let times = |rows: &[Row], row: &Row| rows.iter().filter(|one| *one == row).count();
        let mut said = Vec::new();
        for (lead, rows) in [("missing", &self.missing), ("extra", &self.extra)] {
            for row in rows.iter().take(SAID) {
                let mut line = format!("{lead} row {row}");
                let (listed, written) = (times(wanted, row), times(got, row));
                let _ = if listed > 0 && written > 0 {
                    write!(
                        line,
                        " (the rows list it {listed} time(s), the output writes it {written})"
                    )
                } else if lead == "missing" {
                    let beside: Vec<String> = got
                        .iter()
                        .filter(|one| one.body == row.body && one.record == row.record)
                        .map(|one| one.detail().unwrap_or_else(|| "nothing within".to_owned()))
                        .collect();
                    if beside.is_empty() {
                        write!(line, " (the output has nothing at that record and body)")
                    } else {
                        write!(
                            line,
                            " (the output has at that record and body: {})",
                            beside.join("; ")
                        )
                    }
                } else {
                    Ok(())
                };
                said.push(line);
            }
            if rows.len() > SAID {
                said.push(format!("and {} more {lead} row(s)", rows.len() - SAID));
            }
        }
        said.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::{difference, parse, write, Row};

    fn row(record: &str, at: Option<&str>, value: Option<&str>, count: Option<u64>) -> Row {
        Row {
            body: "urn:example:gap".to_owned(),
            record: record.to_owned(),
            at: at.map(str::to_owned),
            value: value.map(str::to_owned),
            count,
        }
    }

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        /// `test(adapter, vocabulary, options)`
        mod test {
            use super::*;

            #[test]
            fn reads_back_every_row_it_writes_whatever_the_characters_of_its_cells() {
                let rows = vec![
                    row("", None, None, None),
                    row(
                        "/a",
                        Some("b[1]"),
                        Some("a \"quoted\"\tvalue\nof \\ two lines\r"),
                        Some(3),
                    ),
                    row("/a", None, Some(""), None),
                    row("/ünï/😀", Some("x"), None, Some(1)),
                ];
                let mut sorted = rows.clone();
                sorted.sort();
                assert_eq!(parse(&write(&rows)).expect("the rows read back"), sorted);
            }

            #[test]
            fn writes_a_count_column_only_where_a_row_has_a_count() {
                let plain = write(&[row("/a", None, None, None)]);
                assert_eq!(
                    plain,
                    "?body\t?record\t?at\t?value\n<urn:example:gap>\t\"/a\"\t\t\n"
                );
                let counted = write(&[row("/a", None, None, Some(2))]);
                assert!(
                    counted.starts_with("?body\t?record\t?at\t?value\t?count\n"),
                    "{counted}"
                );
            }

            #[test]
            fn reads_a_value_that_is_empty_apart_from_none() {
                let rows =
                    parse("?body\t?record\t?at\t?value\n<urn:example:gap>\t\"/a\"\t\t\"\"\n")
                        .expect("one row");
                assert_eq!(rows, [row("/a", None, Some(""), None)]);
            }

            #[test]
            fn names_the_line_and_the_cell_it_cannot_read() {
                let header = "?body\t?record\t?at\t?value\n";
                for (written, said) in [
                    ("", "line 1"),
                    ("?body\t?record\t?at\n", "line 1"),
                    ("?body\t?record\t?at\t?value\t?count\t?more\n", "line 1"),
                    (
                        &format!("{header}<urn:example:gap>\t\"/a\"\t\n") as &str,
                        "line 2 has 3 cell(s)",
                    ),
                    (
                        &format!("{header}urn:example:gap\t\"/a\"\t\t\n"),
                        "line 2: \"urn:example:gap\" is not an IRI",
                    ),
                    (
                        &format!("{header}<urn:example:gap>\t/a\t\t\n"),
                        "line 2: \"/a\" is not a quoted string",
                    ),
                    (
                        &format!("{header}<urn:example:gap>\t\"/a\"\t\t\"a\\q\"\n"),
                        "holds the escape \\q",
                    ),
                    (
                        &format!("{header}<urn:example:gap>\t\"/a\"\t\t\"a\"b\"\n"),
                        "holds a quote",
                    ),
                    (
                        &format!("{header}<urn:example:gap>\t\"/a\"\t\t\n\n"),
                        "line 3 has 1 cell(s)",
                    ),
                    (
                        "?body\t?record\t?at\t?value\t?count\n<urn:example:gap>\t\"/a\"\t\t\t0\n",
                        "line 2: \"0\" is no count",
                    ),
                ] {
                    let refused = parse(written).expect_err(written);
                    assert!(refused.contains(said), "{written:?}: {refused}");
                }
            }

            #[test]
            fn counts_a_row_written_twice_and_listed_once_as_one_extra() {
                let once = [row("/a", None, None, None)];
                let twice = [row("/a", None, None, None), row("/a", None, None, None)];
                assert_eq!(difference(&once, &twice).extra, once);
                assert!(difference(&once, &twice).missing.is_empty());
                assert_eq!(difference(&twice, &once).missing, once);
                assert!(difference(&twice, &twice).is_empty());
            }
        }
    }
}
