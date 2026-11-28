use crate::accounting::Decision;
use crate::fixtures::{
    accounting, annotations, entry, findings, says, tiny, tiny_json, unaccounted, Variant, CRATE,
    OA, PATH_NOT_ACCOUNTED, SH,
};
use crate::load::load_adapter;
use crate::resolver::Resolver;
use crate::run::declared_forms::{declaring_forms, written_once, WRITTEN_IN_EVERY_FORM};
use crate::run::{prepare, survey, Source};
use crate::survey::{Narrowing, Seen, Survey, SurveyReport, NEVER_DECIDED};
use crate::Result;
use std::collections::BTreeSet;

const BRIDGE: &str = "https://ns.cascadeprotocol.org/bridge/v1-draft#";

const CATALOG_INPUTS: [&str; 8] = [
    "two.xml",
    "every-verdict.xml",
    "namespaced.xml",
    "unaccounted-attribute.xml",
    "unaccounted-child.xml",
    "unaccounted-five-times.xml",
    "unaccounted-in-each-record.xml",
    "unaccounted-under-a-repeated-parent.xml",
];

fn surveyed_with(
    resolver: &dyn Resolver,
    inputs: &[&str],
    envelope: Option<&str>,
) -> Result<SurveyReport> {
    let adapter = load_adapter(resolver).expect("adapter");
    let prepared = prepare(&adapter, resolver).expect("prepared");
    let mut surveyed = Survey::new();
    for input in inputs {
        let iri = format!("{}fixtures/in/{input}", resolver.root());
        let bytes = resolver.read(&iri).expect("the input");
        survey(
            &prepared,
            Source {
                iri: &iri,
                envelope,
                bytes: &bytes,
                facts: None,
            },
            input,
            &mut surveyed,
        )?;
    }
    Ok(surveyed.report(&Narrowing::default()))
}

fn surveyed(resolver: &dyn Resolver, inputs: &[&str]) -> SurveyReport {
    surveyed_with(resolver, inputs, None).expect("a survey")
}

/// Each row of one verdict as its kind, its path, the documents and the occurrences.
fn rows<'a>(
    report: &'a SurveyReport,
    verdict: &str,
) -> Vec<(Option<&'a str>, &'a str, usize, usize)> {
    report
        .rows
        .iter()
        .filter(|row| row.verdict == verdict)
        .map(|row| {
            (
                row.kind.as_deref(),
                row.path.as_str(),
                row.documents,
                row.occurrences,
            )
        })
        .collect()
}

/// The paths the census of a conversion of each input names, and the survey of the inputs.
fn censused(inputs: &[&str]) -> BTreeSet<String> {
    inputs
        .iter()
        .flat_map(|input| {
            let found = findings(&tiny(), input);
            annotations(&found)
                .into_iter()
                .filter(|annotation| {
                    says(&found, annotation, &format!("{OA}hasBody")) == PATH_NOT_ACCOUNTED
                })
                .map(|annotation| says(&found, &annotation, &format!("{SH}value")))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The tiny JSON adapter whose records are written in a `resource` member, with items contained
/// in it, and told apart by their `kind`.
fn telling_kinds_apart() -> Variant {
    Variant::of(tiny_json())
        .with(
            "vocab/list-accounting.ttl",
            accounting(&[
                entry("/kind", "ignored", &[]),
                entry("/id", "carried", &[]),
                entry("/title", "carried", &[]),
                entry("/resource", "carried", &[]),
                entry("/contained", "carried", &[]),
            ]),
        )
        .replacing(
            CRATE,
            r#""bridge:jsonPathOfEachRecord": "$""#,
            r#""bridge:jsonPathOfEachRecord": "$", "bridge:pathOfContentInRecord": "/resource", "bridge:pathOfContainedContentBelowContent": "/contained", "bridge:pathOfRecordKindBelowContent": "/kind""#,
        )
        .with(
            "fixtures/in/observation.json",
            r#"{"kind": "item", "resource": {"kind": "Observation", "id": "9", "novelty": "x",
  "contained": [{"kind": "Medication", "id": "c1", "novelty": "y"},
                {"kind": "Practitioner", "id": "c2", "novelty": "z"}]}}"#,
        )
        .with(
            "fixtures/in/another-observation.json",
            r#"{"kind": "item", "resource": {"kind": "Observation", "id": "8",
  "contained": [{"kind": "Medication", "id": "c3", "novelty": "w"},
                {"kind": "Medication", "id": "c4", "novelty": "v"}]}}"#,
        )
        .with(
            "fixtures/in/unnamed-contained.json",
            r#"{"kind": "item", "resource": {"kind": "Observation", "id": "5",
  "contained": [{"id": "c9", "novelty": "u"}]}}"#,
        )
        .with(
            "fixtures/in/no-kind.json",
            r#"{"kind": "item", "resource": {"id": "7", "novelty": "q"}}"#,
        )
}

/// Surveying documents against an accounting · Loaded::survey()
///
/// A survey lifts each document and reads each path of its records against the adapter's source
/// accounting, by the record kind the path stands in, and converts none. It is ranked by the
/// documents that hold a path.
mod tests {
    use super::*;

    #[test]
    fn names_as_never_decided_exactly_the_paths_a_conversions_census_names() {
        let report = surveyed(&tiny(), &CATALOG_INPUTS);
        let never: BTreeSet<String> = rows(&report, NEVER_DECIDED)
            .into_iter()
            .map(|(_, path, _, _)| path.to_owned())
            .collect();
        let census = censused(&CATALOG_INPUTS);
        assert!(!census.is_empty());
        assert_eq!(never, census);
    }

    #[test]
    fn counts_a_path_apart_in_each_record_kind_it_stands_in_down_to_a_contained_record() {
        let report = surveyed(
            &telling_kinds_apart(),
            &[
                "observation.json",
                "another-observation.json",
                "no-kind.json",
            ],
        );
        assert_eq!(
            rows(&report, NEVER_DECIDED),
            [
                (Some("Medication"), "/novelty", 2, 3),
                (Some("Observation"), "/novelty", 1, 1),
                (Some("Practitioner"), "/novelty", 1, 1),
                (None, "/novelty", 1, 1),
            ]
        );
    }

    #[test]
    fn counts_a_path_in_a_contained_record_that_names_no_kind_in_the_kind_of_its_container() {
        let report = surveyed(&telling_kinds_apart(), &["unnamed-contained.json"]);
        assert_eq!(
            rows(&report, NEVER_DECIDED),
            [(Some("Observation"), "/novelty", 1, 1)]
        );
    }

    #[test]
    fn files_the_contained_member_itself_under_the_kind_of_its_container() {
        let report = surveyed(&telling_kinds_apart(), &["observation.json"]);
        let carried = rows(&report, "carried");
        assert!(
            carried.contains(&(Some("Observation"), "/contained", 1, 2)),
            "{carried:?}"
        );
        assert!(
            !carried.contains(&(Some("Medication"), "/contained", 1, 1)),
            "{carried:?}"
        );
    }

    #[test]
    fn ranks_a_place_written_in_several_forms_as_one_row_of_the_documents_that_hold_it() {
        let report = surveyed(
            &declaring_forms(&written_once(), WRITTEN_IN_EVERY_FORM),
            &["forms.json"],
        );
        assert_eq!(rows(&report, NEVER_DECIDED), [(None, "/novelty", 1, 3)]);
    }

    #[test]
    fn keeps_apart_the_forms_of_a_place_whose_entries_say_different_things() {
        let mut entries = written_once();
        entries.push(entry("/resource/novelty", "ignored", &[]));
        let report = surveyed(
            &declaring_forms(&entries, WRITTEN_IN_EVERY_FORM),
            &["forms.json"],
        );
        assert_eq!(rows(&report, NEVER_DECIDED), [(None, "/novelty", 1, 2)]);
        assert_eq!(rows(&report, "ignored"), [(None, "/novelty", 1, 1)]);
    }

    #[test]
    fn lists_a_document_no_record_can_be_read_from_as_not_read_and_goes_on() {
        let broken = telling_kinds_apart()
            .with("fixtures/in/empty.json", "")
            .with("fixtures/in/scalar.json", "\"text\"")
            .with("fixtures/in/no-envelope.json", r#"{"other": 1}"#);
        let report = surveyed(
            &broken,
            &[
                "empty.json",
                "scalar.json",
                "no-envelope.json",
                "no-kind.json",
            ],
        );
        let names: Vec<&str> = report.not_read.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["empty.json", "no-envelope.json", "scalar.json"]);
        assert!(report.not_read.iter().all(|n| !n.reason.is_empty()));
        assert_eq!(rows(&report, NEVER_DECIDED), [(None, "/novelty", 1, 1)]);
        assert_eq!(report.read, 1);
    }

    #[test]
    fn says_why_each_document_that_holds_no_record_was_not_read() {
        let adapter = telling_kinds_apart()
            .with("fixtures/in/empty.json", "")
            .with("fixtures/in/broken.json", "{not json")
            .with("fixtures/in/no-envelope.json", r#"{"other": 1}"#)
            .with(
                "fixtures/in/empty-list.json",
                r#"{"kind": "list", "items": []}"#,
            );
        let report = surveyed(
            &adapter,
            &[
                "empty.json",
                "broken.json",
                "no-envelope.json",
                "empty-list.json",
            ],
        );
        let reasons: Vec<(&str, &str)> = report
            .not_read
            .iter()
            .map(|n| (n.name.as_str(), n.reason.as_str()))
            .collect();
        assert_eq!(reasons.len(), 4);
        assert_eq!(
            reasons
                .iter()
                .map(|(name, reason)| (*name, reason.split(':').next().unwrap_or_default()))
                .collect::<Vec<_>>(),
            [
                (
                    "broken.json",
                    "it is not JSON, the syntax this adapter reads"
                ),
                (
                    "empty-list.json",
                    "it parses, an envelope accepts it, and it holds no record"
                ),
                ("empty.json", "the file is empty"),
                (
                    "no-envelope.json",
                    "it parses, and no envelope of this adapter accepts it"
                ),
            ]
        );
    }

    #[test]
    fn says_a_text_in_an_xml_adapter_is_not_xml() {
        let adapter = Variant::of(tiny()).with("fixtures/in/text.xml", "not a document");
        let report = surveyed(&adapter, &["text.xml"]);
        assert_eq!(
            report.not_read[0].reason,
            "it is not XML, the syntax this adapter reads"
        );
    }

    #[test]
    fn says_that_kinds_are_not_read_where_an_xml_envelope_declares_a_kind_path() {
        let adapter = Variant::of(tiny()).replacing(
            CRATE,
            r#""bridge:docRootElementName": "catalog","#,
            r#""bridge:docRootElementName": "catalog", "bridge:pathOfRecordKindBelowContent": "/kind","#,
        );
        let text = surveyed(&adapter, &["two.xml"]).text();
        assert!(
            text.lines()
                .any(|line| line.starts_with("Note       record kinds are read for JSON")),
            "{text}"
        );
        assert!(!surveyed(&tiny(), &["two.xml"]).text().contains("Note  "));
    }

    fn saying(verdict: &str, gap: Option<&str>, because: Option<&str>) -> Decision {
        Decision {
            verdict: Some(format!("{BRIDGE}{verdict}")),
            gap: gap.map(str::to_owned),
            because: because.map(str::to_owned),
        }
    }

    fn found<'a>(place: &str, decision: &'a Decision) -> Seen<'a> {
        Seen {
            kind: None,
            place: place.to_owned(),
            form: None,
            occurrences: 1,
            decision: Some(decision),
        }
    }

    #[test]
    fn prints_a_reason_shared_by_consecutive_rows_once_above_them() {
        let said = saying("ignored", None, Some("A marker.\nOn two lines."));
        let mut survey = Survey::new();
        survey.read(
            1,
            ["/a", "/b", "/c"].map(|place| found(place, &said)),
            &Vec::new(),
        );
        let text = survey.report(&Narrowing::default()).text();
        assert_eq!(text.matches("A marker. On two lines.").count(), 1, "{text}");
        assert!(
            text.contains("  because A marker. On two lines.\n"),
            "{text}"
        );
    }

    #[test]
    fn lists_a_path_carried_in_part_that_names_a_gap_with_those_that_wait_on_a_term() {
        let partly = saying("carriedInPart", Some("urn:example:gap"), None);
        let whole = saying("carried", None, None);
        let mut survey = Survey::new();
        survey.read(
            1,
            [found("/partly", &partly), found("/whole", &whole)],
            &Vec::new(),
        );
        let text = survey.report(&Narrowing::default()).text();
        assert!(
            text.contains("No term yet: 1 path(s)")
                && text.contains("/partly  gap urn:example:gap (carried in part)"),
            "{text}"
        );
        assert!(text.contains("Left out on purpose: 0 path(s)"), "{text}");
    }

    #[test]
    fn escapes_a_control_character_in_a_name_or_a_reason() {
        let mut survey = Survey::new();
        survey.not_read("a\tb.xml", "first\nsecond");
        let text = survey.report(&Narrowing::default()).text();
        assert!(text.contains("  a b.xml  first second\n"), "{text}");
    }

    #[test]
    fn refuses_an_envelope_the_adapter_does_not_declare_for_the_whole_survey() {
        let refused = surveyed_with(
            &telling_kinds_apart(),
            &["no-kind.json"],
            Some("https://example.org/tiny-json-adapter/ro-crate-metadata.json#envelope-none"),
        )
        .expect_err("a refusal");
        assert!(refused.to_string().contains("declares no envelope"));
    }

    #[test]
    fn refuses_an_adapter_with_no_accounting_for_the_whole_survey() {
        let refused = surveyed_with(&unaccounted(), &["two.xml"], None).expect_err("a refusal");
        assert!(refused.to_string().contains("bridge:sourceAccounting"));
    }
}
