use crate::fixtures::{tiny, tiny_json, Variant, ACCOUNTING, CRATE};
use crate::harness::{run_manifest, EntryResult, RunOptions};
use crate::load::load_adapter;

const PREFIXES: &str = "PREFIX rdf:    <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX xsd:    <http://www.w3.org/2001/XMLSchema#>
PREFIX sh:     <http://www.w3.org/ns/shacl#>
PREFIX oa:     <http://www.w3.org/ns/oa#>
PREFIX fx:     <http://sparql.xyz/facade-x/ns/>
PREFIX xyz:    <http://sparql.xyz/facade-x/data/>
PREFIX prov:   <http://www.w3.org/ns/prov#>
PREFIX pav:    <http://purl.org/pav/>
PREFIX bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#>
";

/// Each item a record of its own, with a version arriving with its id; `title` is
/// the pattern that writes its title, or nothing.
fn items(title: &str) -> String {
    format!(
        "{PREFIXES}PREFIX ex: <urn:example:catalog#>
CONSTRUCT {{
  ?s a ex:Item .
  ?v prov:specializationOf ?s ; ex:title ?title .
  [] bridge:arrivedAs ?v ; pav:version ?id .
}}
WHERE {{
  ?item a fx:root, xyz:item ; xyz:id ?id .
  BIND(IRI(CONCAT(\"urn:example:item:\", ?id)) AS ?s)
  BIND(IRI(CONCAT(\"urn:example:draft:\", ?id)) AS ?v)
  {title}
}}"
    )
}

const TITLE_WRITTEN: &str = "OPTIONAL { ?item ?slot ?t . ?t a xyz:title ; rdf:_1 ?title . }";

/// An item's box, a record of its own standing at the box.
fn boxes() -> String {
    format!(
        "{PREFIXES}PREFIX ex: <urn:example:catalog#>
CONSTRUCT {{
  ?b a ex:Box .
  ?bv prov:specializationOf ?b .
  [] bridge:arrivedAs ?bv ; pav:version ?id ;
    oa:hasSelector [ a oa:XPathSelector ; rdf:value ?at ] .
}}
WHERE {{
  ?item a fx:root, xyz:item ; xyz:id ?id ; ?slot ?box .
  ?box a xyz:box .
  bridge:thisRecord bridge:selector ?record .
  BIND(IRI(CONCAT(\"urn:example:box:\", ?id)) AS ?b)
  BIND(IRI(CONCAT(\"urn:example:box-draft:\", ?id)) AS ?bv)
  BIND(CONCAT(?record, \"/box[1]\") AS ?at)
}}"
    )
}

/// `title` is the verdict and what more the entry for `/item/title` says.
fn accounting(title: &str) -> String {
    format!(
        "@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
@prefix ex:     <urn:example:catalog#> .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/@id\" ; bridge:verdict bridge:carried .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/title\" ; {title} ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:title ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/box\" ; bridge:verdict bridge:consumed .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/box/bin\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/bin\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/tag\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
"
    )
}

const CARRIED: &str = "bridge:verdict bridge:carried";

const ENTRIES: [(&str, &str); 5] = [
    (
        "hollow",
        r#"<catalog><item id="6"><title>Sixth</title><bin> </bin><tag>red</tag></item></catalog>"#,
    ),
    (
        "titled",
        r#"<catalog><item id="1"><title>First</title></item></catalog>"#,
    ),
    ("untitled", r#"<catalog><item id="2"/></catalog>"#),
    (
        "blank",
        r#"<catalog><item id="3"><title> </title></item><item id="4"><title/></item></catalog>"#,
    ),
    (
        "boxed",
        r#"<catalog><item id="5"><title>Fifth</title><box><bin>7</bin></box></item></catalog>"#,
    ),
];

/// The tiny adapter, its manifest an input-only entry for each of `ENTRIES`.
fn catalog(title: &str, verdict: &str) -> Variant {
    let names: Vec<String> = ENTRIES
        .iter()
        .map(|(name, _)| format!("<#{name}>"))
        .collect();
    let mut manifest = format!(
        "@prefix mf: <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#> .
@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
<> a mf:Manifest ; bridge:adapter <../> ; mf:entries ( {} ) .
",
        names.join(" ")
    );
    let mut variant = Variant::of(tiny())
        .with("mapping/item.rq", items(title))
        .with("mapping/item-shelf.rq", boxes())
        .with(ACCOUNTING, accounting(verdict));
    for (name, input) in ENTRIES {
        manifest.push_str(&format!(
            "<#{name}> a bridge:InputOnlyTest ; mf:name \"{name}\" ;
  mf:action [ bridge:input <in/{name}.xml> ; bridge:envelope <../ro-crate-metadata.json#envelope-catalog> ] .
"
        ));
        variant = variant.with(&format!("fixtures/in/{name}.xml"), input);
    }
    variant.with("fixtures/manifest.ttl", manifest)
}

fn ran(resolver: &Variant) -> Vec<EntryResult> {
    let adapter = load_adapter(resolver).expect("adapter");
    run_manifest(&adapter, resolver, RunOptions::default()).expect("manifest")
}

fn entry<'a>(results: &'a [EntryResult], name: &str) -> &'a EntryResult {
    results
        .iter()
        .find(|result| result.name == name)
        .unwrap_or_else(|| panic!("no entry {name}"))
}

/// The outcome of the named entry, and what it says.
fn judged(results: &[EntryResult], name: &str) -> (&'static str, String) {
    let entry = entry(results, name);
    (entry.outcome.as_str(), entry.description.clone())
}

mod tests {
    use super::*;

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        /// given a conversion that does not write what the accounting says it carries
        mod given_a_conversion_that_does_not_write_what_the_accounting_says_it_carries {
            use super::*;

            #[test]
            fn fails_an_entry_whose_input_holds_the_path_naming_its_address_the_class_owed_and_the_property(
            ) {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "titled");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains(
                        "the accounting is not met at 1 occurrence(s): /item/title at \
                         \"/catalog/item[1]/title[1]\": no urn:example:catalog#title on its \
                         urn:example:catalog#Item is written, and no finding is about it"
                    ),
                    "{description}"
                );
            }

            #[test]
            fn fails_no_entry_whose_input_does_not_hold_the_path() {
                let results = ran(&catalog("", CARRIED));
                assert_eq!(judged(&results, "untitled").0, "cantTell");
            }

            #[test]
            fn owes_nothing_for_whitespace_or_an_element_with_no_content() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "blank");
                assert_eq!(outcome, "cantTell", "{description}");
            }

            /// owes nothing for a member carried as the property an empty member of the same
            /// element is carried as, which no value can then be written for
            #[test]
            fn owes_nothing_for_a_member_carried_as_an_empty_member_of_the_same_element_is() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "hollow");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains("not met at 1 occurrence(s): /item/title at"),
                    "{description}"
                );
            }

            /// owes a path only to the record standing nearest it, here the box its
            /// `bin` is in, of no class the path is carried on
            #[test]
            fn owes_a_path_only_to_the_record_standing_nearest_it() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "boxed");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains("not met at 1 occurrence(s): /item/title at"),
                    "{description}"
                );
            }

            #[test]
            fn fails_nothing_where_the_mapping_writes_the_path() {
                let results = ran(&catalog(TITLE_WRITTEN, CARRIED));
                for (name, _) in ENTRIES {
                    let (outcome, description) = judged(&results, name);
                    assert_eq!(outcome, "cantTell", "{name}: {description}");
                }
            }

            #[test]
            fn is_met_by_a_finding_naming_the_gap_its_entry_names() {
                let in_part =
                    "bridge:verdict bridge:carriedInPart ; bridge:namesGap ex:summaryLosesItsMarkup";
                let unreported = ran(&catalog("", in_part));
                assert_eq!(judged(&unreported, "titled").0, "failed");
                let reported = catalog("", in_part).with(
                    "mapping/item-findings.rq",
                    format!(
                        "{PREFIXES}PREFIX ex: <urn:example:catalog#>
CONSTRUCT {{
  [] a oa:Annotation ;
    oa:hasTarget [ oa:hasSource bridge:thisRecord ] ;
    oa:hasBody ex:summaryLosesItsMarkup ;
    oa:motivatedBy oa:classifying ;
    sh:resultSeverity sh:Warning .
}}
WHERE {{ ?item a fx:root, xyz:item ; ?slot ?t . ?t a xyz:title . }}"
                    ),
                );
                let (outcome, description) = judged(&ran(&reported), "titled");
                assert_eq!(outcome, "cantTell", "{description}");
            }

            /// is met by a finding at the occurrence whatever its gap, and by one above it only
            /// where its gap is one the path's entry names
            #[test]
            fn is_met_by_a_finding_at_the_occurrence_or_above_it_naming_the_gap_its_entry_names(
            ) {
                // The box's bin carried on the box, its entry saying `verdict`, and a finding
                // of `gap` addressed at `at`.
                let reported = |verdict: &str, gap: &str, at: &str| {
                    catalog(TITLE_WRITTEN, CARRIED)
                        .with(
                            ACCOUNTING,
                            accounting(CARRIED).replacen(
                                "\"/item/box/bin\" ; bridge:verdict bridge:carried ;\n  \
                                 bridge:carriedAs [ bridge:inRecordClass ex:Item",
                                &format!(
                                    "\"/item/box/bin\" ; {verdict} ;\n  \
                                     bridge:carriedAs [ bridge:inRecordClass ex:Box"
                                ),
                                1,
                            ),
                        )
                        .with(
                            "mapping/item-findings.rq",
                            format!(
                                "{PREFIXES}PREFIX ex: <urn:example:catalog#>
CONSTRUCT {{
  [] a oa:Annotation ;
    oa:hasTarget [
      oa:hasSource bridge:thisRecord ;
      oa:hasSelector [ a oa:XPathSelector ; rdf:value \"{at}\" ]
    ] ;
    oa:hasBody ex:{gap} ;
    oa:motivatedBy oa:classifying ;
    sh:resultSeverity sh:Info .
}}
WHERE {{ ?item a fx:root, xyz:item ; ?slot ?box . ?box a xyz:box . }}"
                            ),
                        )
                };
                let naming = "bridge:verdict bridge:carriedInPart ; \
                              bridge:namesGap ex:summaryLosesItsMarkup";
                for (verdict, gap, at, expected) in [
                    (CARRIED, "noteHasNoTerm", "box[1]/bin[1]", "cantTell"),
                    (naming, "summaryLosesItsMarkup", "box[1]", "cantTell"),
                    (CARRIED, "summaryLosesItsMarkup", "box[1]", "failed"),
                    (naming, "noteHasNoTerm", "box[1]", "failed"),
                ] {
                    let (outcome, description) = judged(&ran(&reported(verdict, gap, at)), "boxed");
                    assert_eq!(outcome, expected, "{verdict}, {gap} at {at}: {description}");
                }
            }

            /// given a record nested in another, as the envelope declares
            mod given_a_record_nested_in_another {
                use super::*;

                /// The tiny JSON adapter reading an item with a titled item contained in
                /// it; `contained` is the pattern that also writes a record of the contained
                /// one, or nothing.
                fn nesting(contained: &str) -> Variant {
                    nesting_titled(contained, "Inner")
                }

                fn nesting_titled(contained: &str, title: &str) -> Variant {
                    Variant::of(tiny_json())
                        .replacing(
                            CRATE,
                            r#""bridge:jsonPathOfEachRecord": "$""#,
                            r#""bridge:jsonPathOfEachRecord": "$", "bridge:pathOfContainedContentBelowContent": "/contained""#,
                        )
                        .with(
                            "vocab/list-accounting.ttl",
                            "@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
@prefix ex:     <urn:example:list#> .
[] a bridge:PathEntry ; bridge:sourcePath \"/kind\" ; bridge:verdict bridge:ignored .
[] a bridge:PathEntry ; bridge:sourcePath \"/id\" ; bridge:verdict bridge:carried .
[] a bridge:PathEntry ; bridge:sourcePath \"/contained\" ; bridge:verdict bridge:consumed .
[] a bridge:PathEntry ; bridge:sourcePath \"/contained/kind\" ; bridge:verdict bridge:ignored .
[] a bridge:PathEntry ; bridge:sourcePath \"/contained/id\" ; bridge:verdict bridge:carried .
[] a bridge:PathEntry ; bridge:sourcePath \"/contained/title\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:title ] .
",
                        )
                        .with(
                            "mapping/item.rq",
                            format!(
                                "{PREFIXES}PREFIX ex: <urn:example:list#>
CONSTRUCT {{
  ?s a ex:Item .
  ?v prov:specializationOf ?s .
  ?arrival bridge:arrivedAs ?v ; pav:version ?id ; oa:hasSelector ?selector .
  ?selector a oa:FragmentSelector ; rdf:value ?at .
}}
WHERE {{
  bridge:thisRecord bridge:selector ?record .
  ?root a fx:root .
  {{ ?root xyz:id ?id . }}
  {contained}
  BIND(IRI(CONCAT(\"urn:example:item:\", ?id)) AS ?s)
  BIND(IRI(CONCAT(\"urn:example:draft:\", ?id)) AS ?v)
  BIND(BNODE() AS ?arrival)
  BIND(CONCAT(?record, ?step) AS ?at)
  BIND(IF(BOUND(?at), BNODE(), 1/0) AS ?selector)
}}"
                            ),
                        )
                        .with(
                            "fixtures/in/nested.json",
                            format!(
                                r#"{{"kind": "item", "id": "1", "contained": [{{"kind": "item", "id": "c", "title": {title:?}}}]}}"#
                            ),
                        )
                        .with(
                            "fixtures/manifest.ttl",
                            "@prefix mf: <http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#> .
@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
<> a mf:Manifest ; bridge:adapter <../> ; mf:entries ( <#nested> ) .
<#nested> a bridge:InputOnlyTest ; mf:name \"nested\" ;
  mf:action [ bridge:input <in/nested.json> ; bridge:envelope <../ro-crate-metadata.json#envelope-item> ] .
",
                        )
                }

                /// owes nothing for a path in a nested record no record was written from,
                /// though the record it is nested in is of the class the path is carried on
                #[test]
                fn owes_nothing_for_a_path_in_a_nested_record_no_record_was_written_from() {
                    let (outcome, description) = judged(&ran(&nesting("")), "nested");
                    assert_eq!(outcome, "cantTell", "{description}");
                }

                const CONTAINED: &str = "UNION {
    ?root xyz:contained ?list . ?list ?slot ?inner . ?inner xyz:id ?id .
    BIND(CONCAT(\"/contained/\", STR(xsd:integer(STRAFTER(STR(?slot), \"#_\")) - 1)) AS ?step)
  }";

                /// owes nothing for a string that holds only markup and white space
                #[test]
                fn owes_nothing_for_a_string_that_holds_only_markup_and_white_space() {
                    let markup = r#"<div xmlns="http://www.w3.org/1999/xhtml"><p> </p></div>"#;
                    let (outcome, description) =
                        judged(&ran(&nesting_titled(CONTAINED, markup)), "nested");
                    assert_eq!(outcome, "cantTell", "{description}");
                }

                #[test]
                fn owes_a_path_in_a_nested_record_to_the_record_written_from_it() {
                    let (outcome, description) = judged(&ran(&nesting(CONTAINED)), "nested");
                    assert_eq!(outcome, "failed", "{description}");
                    assert!(
                        description.contains("/contained/title at \"/contained/0/title\""),
                        "{description}"
                    );
                }
            }
        }
    }
}
