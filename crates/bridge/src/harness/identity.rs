use crate::fixtures::{tiny, versioned, Variant};
use crate::harness::{run_manifest, EntryResult, RunOptions};
use crate::load::load_adapter;

const MANIFEST: &str = "fixtures/manifest.ttl";

fn conversion(input: &str, selector: Option<&str>) -> String {
    let at = selector.map_or_else(String::new, |at| format!(" ; bridge:selector \"{at}\""));
    format!("[ bridge:input <in/{input}> ; bridge:facts <facts/catalog.ttl>{at} ]")
}

fn relation(one: &str, two: &str, same: bool) -> String {
    format!(
        "@prefix mf:     <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#> .
@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
<> a mf:Manifest ; bridge:adapter <../> ; mf:entries ( <#relation> ) .
<#relation> a bridge:IdentityRelationTest ;
  mf:name \"relation\" ;
  mf:action [ bridge:conversion {one}, {two} ] ;
  mf:result [ bridge:sameRecord {same} ] ."
    )
}

/// The versioned adapter, each version arriving at its item's step as `conditions` spells it.
fn selecting_by(conditions: &str) -> Variant {
    versioned()
        .replacing(
            "mapping/item.rq",
            "[] bridge:arrivedAs ?v ; pav:version ?id .",
            "[] bridge:arrivedAs ?v ; pav:version ?id ;
    <http://www.w3.org/ns/oa#hasSelector> [
      a <http://www.w3.org/ns/oa#XPathSelector> ;
      <http://www.w3.org/1999/02/22-rdf-syntax-ns#value> ?at ] .",
        )
        .replacing(
            "mapping/item.rq",
            "AS ?v)",
            format!(
                "AS ?v)
  bridge:thisRecord bridge:selector ?record .
  BIND(CONCAT(\"/catalog/*[{conditions}][\", STRAFTER(?record, \"item[\")) AS ?at)"
            ),
        )
}

fn judged(adapter: Variant, manifest: String) -> EntryResult {
    let resolver = adapter.with(MANIFEST, manifest);
    let adapter = load_adapter(&resolver).expect("adapter");
    let mut results = run_manifest(&adapter, &resolver, RunOptions::default()).expect("run");
    assert_eq!(results.len(), 1);
    results.remove(0)
}

fn judged_as(adapter: Variant, manifest: String) -> (String, String) {
    let result = judged(adapter, manifest);
    (result.outcome.as_str().to_owned(), result.description)
}

mod tests {
    use super::*;

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        mod given_an_identity_entry {
            use super::*;

            #[test]
            fn passes_where_two_documents_name_one_record_and_the_result_says_they_are_one() {
                let (outcome, said) = judged_as(
                    versioned(),
                    relation(
                        &conversion("two.xml", Some("/catalog/item[1]")),
                        &conversion("output-fails-a-shape.xml", None),
                        true,
                    ),
                );
                assert_eq!(outcome, "passed", "{said}");
                assert!(said.contains("urn:example:item:1"), "{said}");
            }

            #[test]
            fn passes_where_two_records_of_one_document_are_two_and_the_result_says_so() {
                let (outcome, said) = judged_as(
                    versioned(),
                    relation(
                        &conversion("two.xml", Some("/catalog/item[1]")),
                        &conversion("two.xml", Some("/catalog/item[2]")),
                        false,
                    ),
                );
                assert_eq!(outcome, "passed", "{said}");
            }

            /// finds the record a selector names as the node it selects, however the adapter
            /// spells the selector its arrival stood at
            #[test]
            fn finds_a_record_by_the_node_its_selector_selects_not_by_how_it_is_spelled() {
                let adapter = || selecting_by("local-name()='item' and namespace-uri()=''");
                let item = |at: u8| {
                    conversion(
                        "two.xml",
                        Some(&format!(
                            "/catalog/*[namespace-uri()='' and local-name()='item'][{at}]"
                        )),
                    )
                };
                let (outcome, said) = judged_as(
                    adapter(),
                    relation(&item(1), &conversion("output-fails-a-shape.xml", None), true),
                );
                assert_eq!(outcome, "passed", "{said}");
                let (outcome, said) = judged_as(adapter(), relation(&item(1), &item(2), false));
                assert_eq!(outcome, "passed", "{said}");
            }

            #[test]
            fn fails_where_the_names_break_the_relation_the_result_states() {
                let (outcome, said) = judged_as(
                    versioned(),
                    relation(
                        &conversion("two.xml", Some("/catalog/item[1]")),
                        &conversion("two.xml", Some("/catalog/item[2]")),
                        true,
                    ),
                );
                assert_eq!(outcome, "failed", "{said}");
                assert!(said.contains("must be one record"), "{said}");
            }

            #[test]
            fn fails_a_conversion_that_names_other_than_one_record() {
                let (outcome, said) = judged_as(
                    versioned(),
                    relation(
                        &conversion("two.xml", None),
                        &conversion("output-fails-a-shape.xml", None),
                        true,
                    ),
                );
                assert_eq!(outcome, "failed", "{said}");
                assert!(said.contains("names 2 record(s)"), "{said}");

                let (outcome, said) = judged_as(
                    Variant::of(tiny()),
                    relation(
                        &conversion("output-fails-a-shape.xml", None),
                        &conversion("output-fails-a-shape.xml", None),
                        true,
                    ),
                );
                assert_eq!(
                    outcome, "failed",
                    "a record no version arrived for is named by nothing"
                );
                assert!(said.contains("names 0 record(s)"), "{said}");
            }
        }
    }
}
