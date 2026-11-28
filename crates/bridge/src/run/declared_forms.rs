use crate::fixtures::{
    accounting, address, annotations, concept, entry, findings, gap_scheme, says, tiny_json,
    Variant, CRATE, OA, PATH_NOT_ACCOUNTED, SH,
};
use oxrdf::Quad;
use std::collections::BTreeSet;

const LISTING: &str = "vocab/list-accounting.ttl";

const STATUS_GAP: &str = "urn:example:catalog#statusOutsideTheTable";

const LOOKUP_IN: &str = "bridge:lookupIn <list-statuses.ttl>";
const LOOKUP_NAMES_GAP: &str = "bridge:lookupNamesGap ex:statusOutsideTheTable";

/// A record written at the top, in a `resource` member, and in an item contained in that
/// member; `novelty` is a member no entry names.
pub(super) const WRITTEN_IN_EVERY_FORM: &str = r#"{"kind": "item", "id": "9", "novelty": "x",
  "resource": {"id": "9", "title": "nine", "novelty": "x",
    "contained": [{"id": "c", "title": "inner", "novelty": "x"}]}}"#;

/// The same record with the member no entry names left out.
const ACCOUNTED_IN_EVERY_FORM: &str = r#"{"kind": "item", "id": "9",
  "resource": {"id": "9", "title": "nine",
    "contained": [{"id": "c", "title": "inner"}]}}"#;

/// A record whose contained item holds a member `x` that holds a member `y`.
const INSIDE_ANOTHER_FORM: &str = r#"{"kind": "item",
  "resource": {"contained": [{"x": {"y": "1"}}]}}"#;

/// A `status` at the top, in the `resource` member and in the item contained in it.
const STATUSES_IN_EVERY_FORM: &str = r#"{"kind": "item", "id": "9", "status": "Retired",
  "resource": {"id": "9", "status": "Retired",
    "contained": [{"id": "c", "status": "Retired"}]}}"#;

fn carrying(paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .map(|path| entry(path, "carried", &[]))
        .collect()
}

/// Each place once, as the bare path.
pub(super) fn written_once() -> Vec<String> {
    carrying(&["/kind", "/id", "/title", "/resource", "/contained"])
}

/// Each place under each prefix, as an adapter that declares its forms was written before they were read.
fn written_for_each_form() -> Vec<String> {
    carrying(&[
        "/kind",
        "/id",
        "/title",
        "/resource",
        "/resource/id",
        "/resource/title",
        "/resource/contained",
        "/resource/contained/id",
        "/resource/contained/title",
    ])
}

/// The tiny JSON adapter whose item envelope writes content in a `resource` member and
/// contained items below it, and whose list envelope writes content at the top and contained
/// items below it: four forms, `""`, `/contained`, `/resource` and `/resource/contained`, with
/// `entries` as its accounting and `record` as `forms.json`.
pub(super) fn declaring_forms(entries: &[String], record: &str) -> Variant {
    reading(entries, record)
        .replacing(
            CRATE,
            r#""bridge:jsonPathOfEachRecord": "$""#,
            r#""bridge:jsonPathOfEachRecord": "$", "bridge:pathOfContentInRecord": "/resource", "bridge:pathOfContainedContentBelowContent": "/contained""#,
        )
        .replacing(
            CRATE,
            r#""bridge:jsonPathOfEachRecord": "$.items[*]","#,
            r#""bridge:jsonPathOfEachRecord": "$.items[*]", "bridge:pathOfContentInRecord": "", "bridge:pathOfContainedContentBelowContent": "/contained","#,
        )
}

/// The tiny JSON adapter declaring no form.
fn reading(entries: &[String], record: &str) -> Variant {
    Variant::of(tiny_json())
        .with(LISTING, accounting(entries))
        .with("fixtures/in/forms.json", record)
}

fn censuses(findings: &[Quad]) -> BTreeSet<String> {
    annotations(findings)
        .into_iter()
        .filter(|annotation| {
            says(findings, annotation, &format!("{OA}hasBody")) == PATH_NOT_ACCOUNTED
        })
        .map(|annotation| says(findings, &annotation, &format!("{SH}value")))
        .collect()
}

fn paths(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// The tiny JSON adapter whose statuses are looked up, in the forms `reading` declares.
fn looking_statuses_up(entries: &[String]) -> Variant {
    declaring_forms(entries, STATUSES_IN_EVERY_FORM)
        .replacing(
            CRATE,
            r#""bridge:sourceAccounting": { "@id": "vocab/list-accounting.ttl" },"#,
            r#""bridge:sourceAccounting": { "@id": "vocab/list-accounting.ttl" }, "bridge:gapScheme": { "@id": "vocab/list-gaps.ttl" },"#,
        )
        .with(
            "vocab/list-gaps.ttl",
            gap_scheme(&[concept("statusOutsideTheTable", "valueNotMapped", None)]),
        )
        .with(
            "vocab/list-statuses.ttl",
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
@prefix ex:   <urn:example:catalog#> .
ex:statuses a skos:ConceptScheme .
ex:status-current a skos:Concept ;
  skos:inScheme ex:statuses ;
  skos:notation \"current\" ;
  skos:exactMatch ex:Current .
",
        )
}

fn looked_up() -> String {
    entry("/status", "consumed", &[LOOKUP_IN, LOOKUP_NAMES_GAP])
}

/// The occurrence each lookup finding is addressed at.
fn missed(findings: &[Quad]) -> BTreeSet<String> {
    annotations(findings)
        .into_iter()
        .filter(|annotation| says(findings, annotation, &format!("{OA}hasBody")) == STATUS_GAP)
        .map(|annotation| address(findings, &annotation).1)
        .collect()
}

mod tests {
    use super::*;

    /// Reading an entry for a place in every declared form · Conversion::findings()
    ///
    /// An envelope declares the prefixes its records write a place under, and an entry written
    /// for the place, the bare path, accounts for it under each, unless an entry stands at the
    /// prefixed path. An adapter that declares no form reads each path as its own place.
    mod reading_an_entry_for_a_place_in_every_declared_form {
        use super::*;

        /// accounts for a place under each declared prefix where the accounting writes it once
        #[test]
        fn accounts_for_a_place_under_each_declared_prefix_where_the_accounting_writes_it_once() {
            let once = findings(
                &declaring_forms(&written_once(), ACCOUNTED_IN_EVERY_FORM),
                "forms.json",
            );
            assert_eq!(censuses(&once), paths(&[]));
        }

        /// reads an accounting written for each form as it did before forms were read
        #[test]
        fn reads_an_accounting_written_for_each_form_as_it_did_before_forms_were_read() {
            let each = findings(
                &declaring_forms(&written_for_each_form(), ACCOUNTED_IN_EVERY_FORM),
                "forms.json",
            );
            assert_eq!(censuses(&each), paths(&[]));
        }

        /// reports a place no entry names under each declared prefix
        #[test]
        fn reports_a_place_no_entry_names_under_each_declared_prefix() {
            let found = findings(
                &declaring_forms(&written_once(), WRITTEN_IN_EVERY_FORM),
                "forms.json",
            );
            assert_eq!(
                censuses(&found),
                paths(&[
                    "/novelty",
                    "/resource/novelty",
                    "/resource/contained/novelty"
                ])
            );
        }

        /// reads each path as its own place where the adapter declares no form
        #[test]
        fn reads_each_path_as_its_own_place_where_the_adapter_declares_no_form() {
            let found = findings(
                &reading(&written_once(), ACCOUNTED_IN_EVERY_FORM),
                "forms.json",
            );
            assert_eq!(
                censuses(&found),
                paths(&[
                    "/resource/id",
                    "/resource/title",
                    "/resource/contained",
                    "/resource/contained/id",
                    "/resource/contained/title",
                ])
            );
        }

        /// copies a lookup of a place to each declared prefix
        #[test]
        fn copies_a_lookup_of_a_place_to_each_declared_prefix() {
            let found = findings(&looking_statuses_up(&[looked_up()]), "forms.json");
            assert_eq!(
                missed(&found),
                paths(&[
                    "/status",
                    "/resource/status",
                    "/resource/contained/0/status"
                ])
            );
        }

        /// lets an entry written with a prefix stand for its form in place of the place's entry
        #[test]
        fn lets_an_entry_written_with_a_prefix_stand_for_its_form_in_place_of_the_place_s_entry() {
            let overriding = entry("/resource/contained/status", "consumed", &[]);
            let found = findings(
                &looking_statuses_up(&[looked_up(), overriding]),
                "forms.json",
            );
            assert_eq!(missed(&found), paths(&["/status", "/resource/status"]));
        }

        /// lets an entry written with the contained prefix and no content prefix stand for the
        /// contained form under every content prefix in place of the place's entry
        #[test]
        fn lets_an_entry_written_with_the_contained_prefix_stand_for_the_contained_form_under_every_content_prefix(
        ) {
            let overriding = entry("/contained/status", "consumed", &[]);
            let found = findings(
                &looking_statuses_up(&[looked_up(), overriding]),
                "forms.json",
            );
            assert_eq!(missed(&found), paths(&["/status", "/resource/status"]));
        }

        /// copies the lookup of an entry written with the contained prefix and no content prefix
        /// to the contained form under every content prefix
        #[test]
        fn copies_the_lookup_of_an_entry_written_with_the_contained_prefix_to_every_content_prefix()
        {
            let contained_lookup = entry(
                "/contained/status",
                "consumed",
                &[LOOKUP_IN, LOOKUP_NAMES_GAP],
            );
            let found = findings(
                &looking_statuses_up(&[carrying(&["/status"]).remove(0), contained_lookup]),
                "forms.json",
            );
            assert_eq!(missed(&found), paths(&["/resource/contained/0/status"]));
        }

        /// accounts for a place of the contained form under every content prefix where an entry
        /// is written with the contained prefix only
        #[test]
        fn accounts_for_a_place_of_the_contained_form_under_every_content_prefix() {
            let mut entries = written_once();
            entries.extend(carrying(&["/contained/novelty"]));
            let found = findings(
                &declaring_forms(&entries, WRITTEN_IN_EVERY_FORM),
                "forms.json",
            );
            assert_eq!(censuses(&found), paths(&["/novelty", "/resource/novelty"]));
        }

        /// lets an entry that stands at the exact path win over the contained prefix's entry and
        /// the place's entry
        #[test]
        fn lets_an_entry_at_the_exact_path_win_over_the_contained_prefix_s_entry_and_the_place_s() {
            let nothing_looked_up = entry("/contained/status", "consumed", &[]);
            let exact = entry(
                "/resource/contained/status",
                "consumed",
                &[LOOKUP_IN, LOOKUP_NAMES_GAP],
            );
            let found = findings(
                &looking_statuses_up(&[looked_up(), nothing_looked_up, exact]),
                "forms.json",
            );
            assert_eq!(
                missed(&found),
                paths(&[
                    "/status",
                    "/resource/status",
                    "/resource/contained/0/status"
                ])
            );
        }

        /// lets an entry written with a content prefix stand for itself only
        #[test]
        fn lets_an_entry_written_with_a_content_prefix_stand_for_itself_only() {
            let overriding = entry("/resource/status", "consumed", &[]);
            let found = findings(
                &looking_statuses_up(&[looked_up(), overriding]),
                "forms.json",
            );
            assert_eq!(
                missed(&found),
                paths(&["/status", "/resource/contained/0/status"])
            );
        }

        /// copies an entry written with the contained prefix only where the copy reads as the
        /// form it is copied to
        #[test]
        fn copies_an_entry_written_with_the_contained_prefix_only_where_the_copy_reads_as_the_form_it_is_copied_to(
        ) {
            let mut entries = written_once();
            entries.extend(carrying(&["/contained/x/y"]));
            let inside_another_form = declaring_forms(&entries, INSIDE_ANOTHER_FORM)
                .replacing(
                    CRATE,
                    r#""@graph": ["#,
                    r##""@graph": [{ "@id": "#envelope-nested", "@type": "bridge:Envelope", "name": "nested", "bridge:docRootMemberName": "kind", "bridge:docRootMemberValue": "nested", "bridge:jsonPathOfEachRecord": "$.nested[*]", "bridge:pathOfContentInRecord": "/resource/contained/x" },"##,
                )
                .replacing(
                    CRATE,
                    r##"{ "@id": "#envelope-list" }]"##,
                    r##"{ "@id": "#envelope-list" }, { "@id": "#envelope-nested" }]"##,
                );
            let found = findings(&inside_another_form, "forms.json");
            assert_eq!(
                censuses(&found),
                paths(&["/resource/contained/x", "/resource/contained/x/y"])
            );
        }
    }
}
