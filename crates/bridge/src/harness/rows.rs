use crate::fixtures::{tiny_with_vocabularies, Variant};
use crate::harness::{run_manifest, EntryResult, Outcome, RunOptions};
use crate::load::load_adapter;

const MANIFEST: &str = "fixtures/manifest.ttl";
const ENTRIES: &str = "mf:entries ( <#pass> <#graph-fail> <#findings-fail> <#findings-repeated> <#census> <#shapes> <#input-only> <#dataset> )";
const CATALOG: &str = "urn:example:catalog#";
const SHACL: &str = "http://www.w3.org/ns/shacl#";
const CENSUS: &str = "https://ns.cascadeprotocol.org/cascade#SourcePathNotAccounted";
const UNDECLARED: &str = "https://ns.cascadeprotocol.org/bridge/v1-draft#predicateNotDeclared";
const SCHEMA_RULE: &str = "https://www.w3.org/TR/xmlschema-1/#cvc-complex-type";

/// A conversion test of the tiny adapter that names `result` beside its graph, and `on_the_test`
/// on itself, with the files it reads.
struct Case {
    input: &'static str,
    graph: &'static str,
    facts: bool,
    on_the_test: String,
    result: String,
    files: Vec<(&'static str, String)>,
}

impl Case {
    fn of(input: &'static str, graph: &'static str) -> Self {
        Self {
            input,
            graph,
            facts: false,
            on_the_test: String::new(),
            result: String::new(),
            files: Vec::new(),
        }
    }

    fn with_the_facts(mut self) -> Self {
        self.facts = true;
        self
    }

    fn naming(mut self, property: &str) -> Self {
        self.on_the_test.push_str(property);
        self
    }

    fn rows(mut self, rows: &[&str]) -> Self {
        self.result = "bridge:expectedFindingRows <findings/subject.tsv>".to_owned();
        self.files.push((
            "fixtures/findings/subject.tsv",
            format!("?body\t?record\t?at\t?value\n{}", lines(rows)),
        ));
        self
    }

    fn counted_rows(mut self, rows: &[&str]) -> Self {
        self.result = "bridge:expectedFindingRows <findings/subject.tsv>".to_owned();
        self.files.push((
            "fixtures/findings/subject.tsv",
            format!("?body\t?record\t?at\t?value\t?count\n{}", lines(rows)),
        ));
        self
    }

    fn and_the_findings_file(mut self) -> Self {
        self.result
            .push_str(" ; bridge:expectedFindings <findings/two.ttl>");
        self
    }

    fn file(mut self, text: &str) -> Self {
        self.result = "bridge:expectedFindingRows <findings/subject.tsv>".to_owned();
        self.files
            .push(("fixtures/findings/subject.tsv", text.to_owned()));
        self
    }

    fn judged(self) -> EntryResult {
        let facts = if self.facts {
            " ; bridge:facts <facts/catalog.ttl>"
        } else {
            ""
        };
        let result = if self.result.is_empty() {
            String::new()
        } else {
            format!(" ; {}", self.result)
        };
        let entry = format!(
            "<#subject> a bridge:IsomorphicConversionTest ;\n  mf:name \"subject\" ;\n  {} mf:action [ bridge:input <in/{}> ; bridge:envelope <../ro-crate-metadata.json#envelope-catalog>{facts} ] ;\n  mf:result [ bridge:expectedGraph <expected/{}>{result} ] .\n",
            self.on_the_test, self.input, self.graph
        );
        let mut resolver = Variant::of(tiny_with_vocabularies())
            .replacing_exactly(MANIFEST, ENTRIES, "mf:entries ( <#subject> )", 1)
            .replacing_exactly(
                MANIFEST,
                "<#dataset> a bridge:DatasetCompletionTest",
                format!("{entry}\n<#dataset> a bridge:DatasetCompletionTest"),
                1,
            );
        for (file, text) in self.files {
            resolver = resolver.with(file, text);
        }
        let adapter = load_adapter(&resolver).expect("adapter");
        run_manifest(&adapter, &resolver, RunOptions::default())
            .expect("manifest")
            .into_iter()
            .find(|result| result.name == "subject")
            .expect("the subject entry")
    }
}

fn lines(rows: &[&str]) -> String {
    rows.iter().map(|row| format!("{row}\n")).collect()
}

fn row(body: &str, record: &str, at: &str, value: &str) -> String {
    let cell = |text: &str| {
        if text.is_empty() {
            String::new()
        } else {
            format!("{text:?}")
        }
    };
    format!("<{body}>\t{record:?}\t{}\t{}", cell(at), cell(value))
}

fn two() -> Case {
    Case::of("two.xml", "two.ttl").with_the_facts()
}

fn the_rows_of_two() -> Vec<String> {
    vec![
        row(
            &format!("{CATALOG}itemHasNoTitle"),
            "/catalog/item[2]",
            "",
            "",
        ),
        row(
            &format!("{CATALOG}noteHasNoTerm"),
            "/catalog/item[1]",
            "note[1]",
            "",
        ),
        row(
            &format!("{CATALOG}noteHasNoTerm"),
            "/catalog/item[1]",
            "note[1]",
            "/item/note",
        ),
    ]
}

fn borrowed(rows: &[String]) -> Vec<&str> {
    rows.iter().map(String::as_str).collect()
}

fn said(result: &EntryResult) -> &str {
    &result.description
}

mod tests {
    use super::*;

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        /// `test(adapter, vocabulary, options)`
        mod test {
            use super::*;

            #[test]
            fn passes_a_conversion_test_whose_rows_are_the_findings_the_conversion_writes() {
                let result = two().rows(&borrowed(&the_rows_of_two())).judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
                assert!(said(&result).contains("findings match the rows (3 row(s))"));
            }

            #[test]
            fn fails_naming_the_row_the_findings_lack_and_what_they_hold_at_that_record_and_body() {
                let mut rows = the_rows_of_two();
                rows[2] = row(
                    &format!("{CATALOG}noteHasNoTerm"),
                    "/catalog/item[1]",
                    "note[1]",
                    "/item/other",
                );
                let result = two().rows(&borrowed(&rows)).judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                let said = said(&result);
                assert!(
                    said.contains(&format!("missing row <{CATALOG}noteHasNoTerm> at record \"/catalog/item[1]\", within \"note[1]\", value \"/item/other\"")),
                    "{said}"
                );
                assert!(
                    said.contains("the output has at that record and body: within \"note[1]\", value \"/item/note\""),
                    "{said}"
                );
            }

            #[test]
            fn fails_naming_the_row_the_rows_lack() {
                let rows = the_rows_of_two();
                let result = two().rows(&borrowed(&rows[1..])).judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains(&format!(
                        "extra row <{CATALOG}itemHasNoTitle> at record \"/catalog/item[2]\""
                    )),
                    "{}",
                    said(&result)
                );
            }

            #[test]
            fn counts_a_row_listed_twice_as_two_rows() {
                let once = row(
                    &format!("{CATALOG}noteHasNoTerm"),
                    "/catalog/item[2]",
                    "note[1]",
                    "",
                );
                let rows = [
                    row(
                        &format!("{CATALOG}itemHasNoTitle"),
                        "/catalog/item[1]",
                        "",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[1]",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[1]",
                        "/item/note",
                    ) + "\t2",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[2]",
                        "",
                    ) + "\t",
                    once.clone() + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[2]",
                        "note[1]",
                        "/item/note",
                    ) + "\t",
                ];
                let result = Case::of("order.xml", "order.ttl")
                    .counted_rows(&borrowed(&rows))
                    .judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));

                let mut once_too_few = rows.to_vec();
                once_too_few.remove(4);
                let result = Case::of("order.xml", "order.ttl")
                    .counted_rows(&borrowed(&once_too_few))
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(said(&result).contains("extra row"), "{}", said(&result));

                let mut twice = rows.to_vec();
                twice.push(once);
                let result = Case::of("order.xml", "order.ttl")
                    .counted_rows(&borrowed(&twice))
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(said(&result).contains("missing row"), "{}", said(&result));
            }

            #[test]
            fn fails_a_row_whose_count_differs_naming_both() {
                let rows = [
                    row(
                        &format!("{CATALOG}itemHasNoTitle"),
                        "/catalog/item[1]",
                        "",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[1]",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[1]",
                        "/item/note",
                    ) + "\t3",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[1]",
                        "note[2]",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[2]",
                        "note[1]",
                        "",
                    ) + "\t",
                    row(
                        &format!("{CATALOG}noteHasNoTerm"),
                        "/catalog/item[2]",
                        "note[1]",
                        "/item/note",
                    ) + "\t",
                ];
                let result = Case::of("order.xml", "order.ttl")
                    .counted_rows(&borrowed(&rows))
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(said(&result).contains("missing row"), "{}", said(&result));
                assert!(said(&result).contains("count 3"), "{}", said(&result));
                assert!(said(&result).contains("count 2"), "{}", said(&result));
            }

            #[test]
            fn reads_a_record_and_a_place_in_it_however_the_address_is_spelled() {
                let mut rows = the_rows_of_two();
                rows[0] = row(
                    &format!("{CATALOG}itemHasNoTitle"),
                    "/catalog/item[position()=2]",
                    "",
                    "",
                );
                rows[1] = row(
                    &format!("{CATALOG}noteHasNoTerm"),
                    "//item[1]",
                    "./note",
                    "",
                );
                rows[2] = row(
                    &format!("{CATALOG}noteHasNoTerm"),
                    "/catalog/item[1]",
                    "note",
                    "/item/note",
                );
                let result = two().rows(&borrowed(&rows)).judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
                rows[0] = row(&format!("{CATALOG}itemHasNoTitle"), "//item[1]", "", "");
                let result = two().rows(&borrowed(&rows)).judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
            }

            #[test]
            fn compares_no_findings_where_the_test_names_no_rows_and_no_findings_file() {
                for case in [two(), Case::of("order.xml", "order.ttl")] {
                    let result = case.judged();
                    assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
                    assert!(
                        said(&result).contains("findings not compared: the entry names no bridge:expectedFindingRows or bridge:expectedFindings"),
                        "{}",
                        said(&result)
                    );
                }
            }

            #[test]
            fn refuses_a_test_that_names_both_a_findings_file_and_a_rows_file() {
                let result = two()
                    .rows(&borrowed(&the_rows_of_two()))
                    .and_the_findings_file()
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains(
                        "names both bridge:expectedFindings and bridge:expectedFindingRows"
                    ),
                    "{}",
                    said(&result)
                );
            }

            #[test]
            fn fails_a_rows_file_that_is_not_in_the_form_naming_the_line() {
                let result = two().file("?body\t?record\n").judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(said(&result).contains("line 1"), "{}", said(&result));
                let bad = format!(
                    "?body\t?record\t?at\t?value\n<{CATALOG}itemHasNoTitle>\t/catalog/item[2]\t\t\n"
                );
                let result = two().file(&bad).judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(said(&result).contains("line 2"), "{}", said(&result));
            }

            #[test]
            fn fails_an_unnamed_census_path_whether_or_not_the_test_names_rows() {
                let result = Case::of("unaccounted-child.xml", "unaccounted-child.ttl").judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains("draws cascade:SourcePathNotAccounted at /item/novelty, which the test does not name"),
                    "{}",
                    said(&result)
                );
                let listed = row(CENSUS, "/catalog/item[1]", "novelty[1]", "/item/novelty");
                let result = Case::of("unaccounted-child.xml", "unaccounted-child.ttl")
                    .rows(&[&listed])
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains("which the test does not name"),
                    "{}",
                    said(&result)
                );
            }

            #[test]
            fn passes_a_census_path_the_test_names_and_fails_one_it_names_that_draws_none() {
                let named = r#"bridge:testsUnaccountedSourcePath "/item/novelty" ;"#;
                let result = Case::of("unaccounted-child.xml", "unaccounted-child.ttl")
                    .naming(named)
                    .judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
                let result = two()
                    .naming(r#"bridge:testsUnaccountedSourcePath "/item/novelty" ;"#)
                    .judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains("names /item/novelty by bridge:testsUnaccountedSourcePath, and the conversion draws no cascade:SourcePathNotAccounted at it"),
                    "{}",
                    said(&result)
                );
            }

            #[test]
            fn fails_a_finding_of_the_outputs_validation_at_violation_unless_the_rows_list_it() {
                let case =
                    || Case::of("output-fails-two-shapes.xml", "output-fails-two-shapes.ttl");
                let result = case().judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains(&format!("the output breaks the vocabulary's shapes at sh:Violation: <{SHACL}MinCountConstraintComponent> at record \"/catalog/item[1]\"")),
                    "{}",
                    said(&result)
                );
                assert!(
                    !said(&result).contains("MaxLength"),
                    "a warning is a note: {}",
                    said(&result)
                );
                let rows = [
                    row(
                        &format!("{SHACL}MaxLengthConstraintComponent"),
                        "/catalog/item[1]",
                        "",
                        "",
                    ),
                    row(
                        &format!("{SHACL}MinCountConstraintComponent"),
                        "/catalog/item[1]",
                        "",
                        "",
                    ),
                ];
                let result = case().rows(&borrowed(&rows)).judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
            }

            #[test]
            fn fails_a_predicate_no_vocabulary_declares_unless_the_rows_list_it() {
                let case = || {
                    Case::of(
                        "a-predicate-no-ontology-declares.xml",
                        "a-predicate-no-ontology-declares.ttl",
                    )
                };
                let result = case().judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains(&format!("<{UNDECLARED}> at record \"/catalog/item[1]\", sh:resultPath urn:example:catalog#colour")),
                    "{}",
                    said(&result)
                );
                let rows = [row(UNDECLARED, "/catalog/item[1]", "", "")];
                let result = case().rows(&borrowed(&rows)).judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
            }

            #[test]
            fn fails_an_input_that_breaks_its_source_schema_unless_the_rows_list_the_violations() {
                let result = Case::of("invalid.xml", "invalid.ttl").judged();
                assert_eq!(result.outcome, Outcome::Failed, "{}", said(&result));
                assert!(
                    said(&result).contains(&format!("the input breaks its source schema: <{SCHEMA_RULE}> at record \"/catalog\", within \"item[2]\"")),
                    "{}",
                    said(&result)
                );
                let rows = [
                    row(SCHEMA_RULE, "/catalog", "item[2]", ""),
                    row(SCHEMA_RULE, "/catalog/item[2]", "", ""),
                ];
                let result = Case::of("invalid.xml", "invalid.ttl")
                    .rows(&borrowed(&rows))
                    .judged();
                assert_eq!(result.outcome, Outcome::Passed, "{}", said(&result));
            }
        }
    }
}
