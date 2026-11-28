use crate::fixtures::{
    accounting, annotations, entry, findings, says, tiny, tiny_json, unaccounted, Variant, CRATE,
    OA, PATH_NOT_ACCOUNTED, SH,
};
use crate::load::load_adapter;
use crate::resolver::Resolver;
use crate::run::{prepare, survey, Source};
use crate::survey::{Narrowing, Survey, SurveyReport, NEVER_DECIDED};
use crate::Result;
use std::collections::BTreeSet;

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
                (Some("Medication"), "/resource/contained/novelty", 2, 3),
                (Some("Observation"), "/resource/novelty", 1, 1),
                (Some("Practitioner"), "/resource/contained/novelty", 1, 1),
                (None, "/resource/novelty", 1, 1),
            ]
        );
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
        assert_eq!(
            rows(&report, NEVER_DECIDED),
            [(None, "/resource/novelty", 1, 1)]
        );
        assert_eq!(report.read, 1);
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
