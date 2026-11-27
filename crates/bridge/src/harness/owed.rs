use crate::fixtures::{tiny, tiny_json, Variant, ACCOUNTING, CRATE, GAP_SCHEME};
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

/// An item's shelf, a record of its own standing at the shelf.
fn shelves() -> String {
    shelves_at("/shelf[1]")
}

/// An item's shelf, a record of its own whose arrival is selected at `step` below the item.
fn shelves_at(step: &str) -> String {
    format!(
        "{PREFIXES}PREFIX ex: <urn:example:catalog#>
CONSTRUCT {{
  ?b a ex:Shelf .
  ?bv prov:specializationOf ?b .
  [] bridge:arrivedAs ?bv ; pav:version ?id ;
    oa:hasSelector [ a oa:XPathSelector ; rdf:value ?at ] .
}}
WHERE {{
  ?item a fx:root, xyz:item ; xyz:id ?id ; ?slot ?shelf .
  ?shelf a xyz:shelf .
  bridge:thisRecord bridge:selector ?record .
  BIND(IRI(CONCAT(\"urn:example:shelf:\", ?id)) AS ?b)
  BIND(IRI(CONCAT(\"urn:example:shelf-draft:\", ?id)) AS ?bv)
  BIND(CONCAT(?record, \"{step}\") AS ?at)
}}"
    )
}

/// No record of a shelf: the tiny adapter's own shelf mapping, which reads boxes.
const NO_SHELF_RECORD: &str = "PREFIX fx:  <http://sparql.xyz/facade-x/ns/>
PREFIX xyz: <http://sparql.xyz/facade-x/data/>
PREFIX ex:  <urn:example:catalog#>
CONSTRUCT { ?s ex:shelf [ a ex:Shelf ] . }
WHERE {
  ?item a fx:root, xyz:item ; xyz:id ?id ; ?slot ?box .
  ?box a xyz:box .
  BIND(IRI(CONCAT(\"urn:example:item:\", ?id)) AS ?s)
}";

/// `title` is the verdict and what more the entry for `/item/title` says.
fn accounting(title: &str) -> String {
    format!(
        "@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
@prefix ex:     <urn:example:catalog#> .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/@id\" ; bridge:verdict bridge:carried .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/title\" ; {title} ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:title ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/shelf\" ; bridge:verdict bridge:consumed .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/shelf/mark\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/bin\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
[] a bridge:PathEntry ; bridge:sourcePath \"/item/tag\" ; bridge:verdict bridge:carried ;
  bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .
"
    )
}

const CARRIED: &str = "bridge:verdict bridge:carried";

const ENTRIES: [(&str, &str); 9] = [
    (
        "no_break_space",
        r#"<catalog><item id="12"><title>&#160;</title></item></catalog>"#,
    ),
    (
        "em_space",
        r#"<catalog><item id="13"><title>&#8195;</title></item></catalog>"#,
    ),
    (
        "escaped_markup",
        r#"<catalog><item id="14"><title>&lt;p&gt; &lt;/p&gt;</title></item></catalog>"#,
    ),
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
        "shelved",
        r#"<catalog><item id="5"><title>Fifth</title><shelf><mark>7</mark></shelf></item></catalog>"#,
    ),
    (
        "many",
        r#"<catalog><item id="8"><title>A</title></item><item id="9"><title>B</title></item><item id="10"><title>C</title></item><item id="11"><title>D</title></item></catalog>"#,
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
        .with("mapping/item-shelf.rq", shelves())
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

/// The tiny adapter writing each title, with the shelf's mark carried on the shelf, its entry
/// saying `verdict`.
fn mark_on_the_shelf(verdict: &str) -> Variant {
    let mark = "\"/item/shelf/mark\" ; bridge:verdict bridge:carried ;\n  \
                bridge:carriedAs [ bridge:inRecordClass ex:Item";
    let written = accounting(CARRIED);
    assert!(written.contains(mark), "{written}");
    catalog(TITLE_WRITTEN, CARRIED).with(
        ACCOUNTING,
        written.replacen(
            mark,
            &format!(
                "\"/item/shelf/mark\" ; {verdict} ;\n  \
                 bridge:carriedAs [ bridge:inRecordClass ex:Shelf"
            ),
            1,
        ),
    )
}

/// The tiny adapter writing each title, the entry for the shelf's mark saying `said` after its
/// path.
fn mark_said(said: &str) -> Variant {
    let mark = "\"/item/shelf/mark\" ; bridge:verdict bridge:carried ;\n  \
                bridge:carriedAs [ bridge:inRecordClass ex:Item ; bridge:asProperty ex:code ] .";
    let written = accounting(CARRIED);
    assert!(written.contains(mark), "{written}");
    catalog(TITLE_WRITTEN, CARRIED).with(
        ACCOUNTING,
        written.replacen(mark, &format!("\"/item/shelf/mark\" ; {said} ."), 1),
    )
}

/// `variant` writing no record at all: no item, and no shelf.
fn writing_no_record(variant: Variant) -> Variant {
    variant
        .with("mapping/item.rq", items("FILTER(false)"))
        .with("mapping/item-shelf.rq", NO_SHELF_RECORD)
}

/// `variant` with the gap `ex:shelfNotMapped` of `bridge:elementNotCarried`, and a finding of
/// `gap` addressed at `at` in each item holding a shelf.
fn reporting(variant: Variant, gap: &str, at: &str) -> Variant {
    variant
        .replacing(
            GAP_SCHEME,
            "ex:noteHasNoTerm a skos:Concept ;",
            "ex:shelfNotMapped a skos:Concept ;\n  skos:inScheme ex:gaps ;\n  \
             skos:broader bridge:elementNotCarried .\n\n\
             ex:noteHasNoTerm a skos:Concept ;",
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
WHERE {{ ?item a fx:root, xyz:item ; ?slot ?shelf . ?shelf a xyz:shelf . }}"
            ),
        )
}

/// The tiny adapter writing each item's title as a node of its own, nested in the version as
/// `?title`, with `?deeper` a second such node, `members` being what the mapping writes of them.
fn titled_by_a_node(members: &str) -> Variant {
    let titled = r##"BIND(IRI(CONCAT(STR(?v), "#title")) AS ?title)
  BIND(IRI(CONCAT(STR(?v), "#deeper")) AS ?deeper)"##;
    catalog(titled, CARRIED).replacing(
        "mapping/item.rq",
        "?v prov:specializationOf ?s ; ex:title ?title .",
        format!("?v prov:specializationOf ?s ; ex:title ?title . {members}"),
    )
}

mod tests {
    use super::*;

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        /// given a conversion that does not write what the accounting says it carries
        mod given_a_conversion_that_does_not_write_what_the_accounting_says_it_carries {
            use super::*;

            /// fails an entry whose input holds the path, naming the path, the property looked
            /// for and the class owed before the occurrence's address
            #[test]
            fn fails_an_entry_whose_input_holds_the_path_naming_what_is_owed_before_where() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "titled");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains(
                        "the accounting is not met at 1 occurrence(s): /item/title: no \
                         urn:example:catalog#title on its urn:example:catalog#Item is written, \
                         and no finding is about it, at \"/catalog/item[1]/title[1]\""
                    ),
                    "{description}"
                );
            }

            /// says the first three occurrences in full, then how many more `--owed` lists
            #[test]
            fn says_the_first_three_occurrences_in_full_then_how_many_more_owed_lists() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "many");
                assert_eq!(outcome, "failed", "{description}");
                assert_eq!(entry(&results, "many").unmet.len(), 4, "{description}");
                let said = "/item/title: no urn:example:catalog#title on its \
                            urn:example:catalog#Item is written, and no finding is about it, at";
                assert_eq!(description.matches(said).count(), 3, "{description}");
                assert!(
                    description
                        .contains("; and 1 more: run `test` with `--owed` to list every one"),
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

            /// is met by a title written as it stands where an XML element holds only a
            /// character that is no XML white space, or markup written as text, since each is
            /// an occurrence that is not empty
            #[test]
            fn is_met_by_a_value_written_as_it_stands_that_is_not_empty_in_the_source_syntax() {
                let results = ran(&catalog(TITLE_WRITTEN, CARRIED));
                for name in ["no_break_space", "em_space", "escaped_markup"] {
                    let (outcome, description) = judged(&results, name);
                    assert_eq!(outcome, "cantTell", "{name}: {description}");
                }
            }

            /// is not met by a property whose only written value is empty, and owes nothing
            /// for an occurrence that is itself empty
            #[test]
            fn is_not_met_by_a_property_written_only_as_an_empty_value() {
                for written in [r#"BIND("" AS ?title)"#, r#"BIND(" " AS ?title)"#] {
                    let results = ran(&catalog(written, CARRIED));
                    let (outcome, description) = judged(&results, "titled");
                    assert_eq!(outcome, "failed", "{written}: {description}");
                    assert!(
                        description.contains("not met at 1 occurrence(s): /item/title:"),
                        "{written}: {description}"
                    );
                    let (outcome, description) = judged(&results, "blank");
                    assert_eq!(outcome, "cantTell", "{written}: {description}");
                }
            }

            /// is met by one non-empty value written beside an empty one: the check does not
            /// count values
            #[test]
            fn is_met_by_a_non_empty_value_written_beside_an_empty_one() {
                let written = catalog(r#"VALUES ?title { "" "First" }"#, CARRIED);
                let (outcome, description) = judged(&ran(&written), "titled");
                assert_eq!(outcome, "cantTell", "{description}");
            }

            /// is not met by a property whose only value written is a node nested in the version
            /// whose every member is empty, however deep, a node that is its own member and a
            /// typed node whose other members are empty included, and owes nothing for an
            /// occurrence that is itself empty
            #[test]
            fn is_not_met_by_a_property_written_only_as_a_node_of_nothing() {
                for members in [
                    r#"?title ex:part "" ."#,
                    r#"?title ex:part "", " " ."#,
                    r#"?title ex:part ?deeper . ?deeper ex:part " " ."#,
                    "?title ex:part ?title .",
                    r#"?title a ex:Code ; ex:part "" ."#,
                ] {
                    let results = ran(&titled_by_a_node(members));
                    let (outcome, description) = judged(&results, "titled");
                    assert_eq!(outcome, "failed", "{members}: {description}");
                    assert!(
                        description.contains("not met at 1 occurrence(s): /item/title:"),
                        "{members}: {description}"
                    );
                    let (outcome, description) = judged(&results, "blank");
                    assert_eq!(outcome, "cantTell", "{members}: {description}");
                }
            }

            /// is met by a property written as a node holding something: a literal with text, or
            /// a node with no members, at any depth; a node whose only member is its type is a
            /// value, and so is a typed node holding a code; an IRI outside the version is a
            /// value by its name, whatever is said of it; and a member that is empty only in
            /// another syntax, as a no-break space is in JSON, is text
            #[test]
            fn is_met_by_a_property_written_as_a_node_that_holds_something() {
                for members in [
                    "",
                    "?title ex:part \"\u{a0}\" .",
                    r#"?title ex:part "First" ."#,
                    r#"?title ex:part "", "First" ."#,
                    "?title ex:part ?deeper .",
                    r#"?title ex:part ?deeper . ?deeper ex:part "First" ."#,
                    r#"?title ex:part ?title, "First" ."#,
                    "?title a ex:Code .",
                    "?title a ex:Code ; ex:code <urn:example:code> .",
                    r#"?title ex:code <urn:example:code> . <urn:example:code> ex:part "" ."#,
                    "?title ex:code <urn:example:code> . <urn:example:code> a ex:Code .",
                ] {
                    let results = ran(&titled_by_a_node(members));
                    let (outcome, description) = judged(&results, "titled");
                    assert_eq!(outcome, "cantTell", "{members}: {description}");
                }
            }

            /// owes nothing for a member carried as the property an empty member of the same
            /// element is carried as, which no value can then be written for
            #[test]
            fn owes_nothing_for_a_member_carried_as_an_empty_member_of_the_same_element_is() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "hollow");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains("not met at 1 occurrence(s): /item/title:"),
                    "{description}"
                );
            }

            /// owes a member beside an empty repeat of its own path, which is no other member
            /// of its element
            #[test]
            fn owes_a_member_beside_an_empty_repeat_of_its_own_path() {
                let repeated = mark_on_the_shelf(CARRIED).with(
                    "fixtures/in/shelved.xml",
                    r#"<catalog><item id="5"><title>Fifth</title><shelf><mark>7</mark><mark> </mark></shelf></item></catalog>"#,
                );
                let (outcome, description) = judged(&ran(&repeated), "shelved");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains("not met at 1 occurrence(s): /item/shelf/mark:"),
                    "{description}"
                );
            }

            /// owes a path only to the record standing nearest it, here the shelf its
            /// `mark` is in, of no class the path is carried on
            #[test]
            fn owes_a_path_only_to_the_record_standing_nearest_it() {
                let results = ran(&catalog("", CARRIED));
                let (outcome, description) = judged(&results, "shelved");
                assert_eq!(outcome, "failed", "{description}");
                assert!(
                    description.contains("not met at 1 occurrence(s): /item/title:"),
                    "{description}"
                );
            }

            /// places a record at the node its selector selects, however the selector spells
            /// it, and fails an entry for a record selector that selects no one node of the
            /// unit, naming it
            #[test]
            fn places_a_record_at_the_node_its_selector_selects_and_fails_one_that_selects_none() {
                for (step, said) in [
                    (
                        "/*[namespace-uri()='' and local-name()='shelf'][1]",
                        "not met at 1 occurrence(s): /item/title:",
                    ),
                    (
                        "/shelf[2]",
                        "the record selector \"/catalog/item[1]/shelf[2]\" selects no node",
                    ),
                    (
                        "/..",
                        "the record selector \"/catalog/item[1]/..\" selects a node outside the \
                         unit",
                    ),
                    (
                        "/*",
                        "the record selector \"/catalog/item[1]/*\" selects 2 nodes",
                    ),
                    (
                        "1 +",
                        "the record selector \"/catalog/item[1]1 +\" is no XPath, or selects no \
                         set of nodes",
                    ),
                ] {
                    let shelved =
                        catalog("", CARRIED).with("mapping/item-shelf.rq", shelves_at(step));
                    let (outcome, description) = judged(&ran(&shelved), "shelved");
                    assert_eq!(outcome, "failed", "{step}: {description}");
                    assert!(description.contains(said), "{step}: {description}");
                    if said.starts_with("the record selector") {
                        // A selector is no occurrence, and is counted as what it is.
                        assert!(
                            description.contains("cannot be checked for 1 record selector(s): "),
                            "{step}: {description}"
                        );
                        assert!(
                            !description.contains("occurrence(s)"),
                            "{step}: {description}"
                        );
                    }
                }
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
            /// where its gap is one the path's entry names; a finding of a gap of
            /// `bridge:elementNotCarried` above it, where a record stands, meets nothing
            #[test]
            fn is_met_by_a_finding_at_the_occurrence_or_above_it_naming_the_gap_its_entry_names() {
                let naming = "bridge:verdict bridge:carriedInPart ; \
                              bridge:namesGap ex:summaryLosesItsMarkup";
                for (verdict, gap, at, expected) in [
                    (CARRIED, "noteHasNoTerm", "shelf[1]/mark[1]", "cantTell"),
                    (naming, "summaryLosesItsMarkup", "shelf[1]", "cantTell"),
                    (CARRIED, "summaryLosesItsMarkup", "shelf[1]", "failed"),
                    (naming, "noteHasNoTerm", "shelf[1]", "failed"),
                    (CARRIED, "shelfNotMapped", "shelf[1]", "failed"),
                ] {
                    let reported = reporting(mark_on_the_shelf(verdict), gap, at);
                    let (outcome, description) = judged(&ran(&reported), "shelved");
                    assert_eq!(outcome, expected, "{verdict}, {gap} at {at}: {description}");
                }
            }

            /// owes nothing below a finding of a gap of `bridge:elementNotCarried` where no
            /// record stands at its element or below it
            #[test]
            fn owes_nothing_below_an_element_reported_not_carried_where_no_record_stands_in_it() {
                let unshelved = || {
                    catalog(TITLE_WRITTEN, CARRIED).with("mapping/item-shelf.rq", NO_SHELF_RECORD)
                };
                let (outcome, description) = judged(&ran(&unshelved()), "shelved");
                assert_eq!(outcome, "failed", "{description}");
                assert!(description.contains("/item/shelf/mark:"), "{description}");
                let reported = reporting(unshelved(), "shelfNotMapped", "shelf[1]");
                let (outcome, description) = judged(&ran(&reported), "shelved");
                assert_eq!(outcome, "cantTell", "{description}");
            }

            /// given an entry naming a gap the adapter reports itself
            mod given_an_entry_naming_a_gap_the_adapter_reports_itself {
                use super::*;

                const NO_HOME_THE_ADAPTER_REPORTS: &str =
                    "bridge:verdict bridge:noHome ; bridge:adapterReportsGap ex:noteHasNoTerm";

                const UNREPORTED: &str = "not met at 1 occurrence(s): /item/shelf/mark: \
                                          no finding of urn:example:catalog#noteHasNoTerm is \
                                          addressed at it or above it, at \
                                          \"/catalog/item[1]/shelf[1]/mark[1]\"";

                /// fails an entry whose input holds the path where no finding of that gap is
                /// addressed at it or above it, naming the gap before the address
                #[test]
                fn fails_an_entry_where_no_finding_of_that_gap_is_at_the_path_or_above_it() {
                    let (outcome, description) =
                        judged(&ran(&mark_said(NO_HOME_THE_ADAPTER_REPORTS)), "shelved");
                    assert_eq!(outcome, "failed", "{description}");
                    assert!(description.contains(UNREPORTED), "{description}");
                }

                /// is met by a finding of that gap at the occurrence or above it, and neither by
                /// one beside it nor by a finding of another gap, even at the occurrence
                #[test]
                fn is_met_by_a_finding_of_that_gap_at_the_occurrence_or_above_it_and_no_other() {
                    for (gap, at, expected) in [
                        ("noteHasNoTerm", "shelf[1]/mark[1]", "cantTell"),
                        ("noteHasNoTerm", "shelf[1]", "cantTell"),
                        ("noteHasNoTerm", "title[1]", "failed"),
                        ("summaryLosesItsMarkup", "shelf[1]/mark[1]", "failed"),
                    ] {
                        let reported = reporting(mark_said(NO_HOME_THE_ADAPTER_REPORTS), gap, at);
                        let (outcome, description) = judged(&ran(&reported), "shelved");
                        assert_eq!(outcome, expected, "{gap} at {at}: {description}");
                    }
                }

                /// owes the path in a unit no record was written from, as in any other
                #[test]
                fn owes_the_path_in_a_unit_no_record_was_written_from() {
                    let unwritten = || writing_no_record(mark_said(NO_HOME_THE_ADAPTER_REPORTS));
                    let (outcome, description) = judged(&ran(&unwritten()), "shelved");
                    assert_eq!(outcome, "failed", "{description}");
                    assert!(description.contains(UNREPORTED), "{description}");
                    let reported = reporting(unwritten(), "noteHasNoTerm", "shelf[1]");
                    let (outcome, description) = judged(&ran(&reported), "shelved");
                    assert_eq!(outcome, "cantTell", "{description}");
                }

                /// on a `bridge:carriedInPart` entry, owes the path where no record of a class
                /// it is carried on holds it only a finding of that gap, and is met where one
                /// does by a property written or by a finding of either gap above it
                #[test]
                fn owes_a_carried_in_part_path_outside_its_classes_only_a_finding_of_that_gap() {
                    let in_part = "bridge:verdict bridge:carriedInPart ; \
                                   bridge:namesGap ex:summaryLosesItsMarkup ; \
                                   bridge:adapterReportsGap ex:noteHasNoTerm ;\n  \
                                   bridge:carriedAs [ bridge:inRecordClass ex:Shelf ; \
                                   bridge:asProperty ex:code ]";
                    let unshelved =
                        || mark_said(in_part).with("mapping/item-shelf.rq", NO_SHELF_RECORD);
                    for (variant, expected) in [
                        (unshelved(), "failed"),
                        (reporting(unshelved(), "noteHasNoTerm", "shelf[1]"), "cantTell"),
                        (
                            reporting(unshelved(), "summaryLosesItsMarkup", "shelf[1]/mark[1]"),
                            "failed",
                        ),
                        (reporting(mark_said(in_part), "noteHasNoTerm", "shelf[1]"), "cantTell"),
                        (
                            reporting(mark_said(in_part), "summaryLosesItsMarkup", "shelf[1]"),
                            "cantTell",
                        ),
                    ] {
                        let (outcome, description) = judged(&ran(&variant), "shelved");
                        assert_eq!(outcome, expected, "{description}");
                    }
                    let (_, description) = judged(&ran(&unshelved()), "shelved");
                    assert!(description.contains(UNREPORTED), "{description}");
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

                /// `variant` with a gap of `bridge:elementNotCarried` and a finding of it
                /// addressed at `at`.
                fn reporting_nothing_carried_at(variant: Variant, at: &str) -> Variant {
                    variant
                        .replacing(
                            CRATE,
                            r#""bridge:mapping": [{ "@id": "mapping/item.rq" }],"#,
                            r#""bridge:mapping": [{ "@id": "mapping/item.rq" }], "bridge:gapScheme": { "@id": "vocab/list-gaps.ttl" }, "bridge:findingsQuery": [{ "@id": "mapping/item-findings.rq" }],"#,
                        )
                        .with(
                            "vocab/list-gaps.ttl",
                            "@prefix skos:   <http://www.w3.org/2004/02/skos/core#> .
@prefix bridge: <https://ns.cascadeprotocol.org/bridge/v1-draft#> .
@prefix ex:     <urn:example:list#> .
ex:gaps a skos:ConceptScheme .
ex:itemNotMapped a skos:Concept ;
  skos:inScheme ex:gaps ;
  skos:broader bridge:elementNotCarried .
",
                        )
                        .with(
                            "mapping/item-findings.rq",
                            format!(
                                "{PREFIXES}PREFIX ex: <urn:example:list#>
CONSTRUCT {{
  [] a oa:Annotation ;
    oa:hasTarget [
      oa:hasSource bridge:thisRecord ;
      oa:hasSelector [ a oa:FragmentSelector ; rdf:value \"{at}\" ]
    ] ;
    oa:hasBody ex:itemNotMapped ;
    oa:motivatedBy oa:classifying ;
    sh:resultSeverity sh:Info .
}}
WHERE {{ ?root a fx:root ; xyz:contained ?list . }}"
                            ),
                        )
                }

                /// owes a path in a nested record no record was written from to the record it
                /// is nested in, as it owes any path by where it stands
                #[test]
                fn owes_a_path_in_a_nested_record_nothing_was_written_from_to_its_container() {
                    let (outcome, description) = judged(&ran(&nesting("")), "nested");
                    assert_eq!(outcome, "failed", "{description}");
                    assert!(
                        description.contains("/contained/title: no urn:example:list#title on its"),
                        "{description}"
                    );
                }

                /// owes nothing in a nested record no record was written from where a finding of
                /// a gap of `bridge:elementNotCarried` is addressed at it, and still owes it where
                /// that finding is addressed at a member of it
                #[test]
                fn owes_nothing_in_a_nested_record_reported_not_carried_at_its_element() {
                    let cases = [
                        ("/contained/0", "cantTell"),
                        ("/contained/0/kind", "failed"),
                    ];
                    for (at, expected) in cases {
                        let reported = reporting_nothing_carried_at(nesting(""), at);
                        let (outcome, description) = judged(&ran(&reported), "nested");
                        assert_eq!(outcome, expected, "{at}: {description}");
                    }
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

                /// is not met by a title written as an empty string, a white space or markup
                /// holding no text, where the string read is not empty
                #[test]
                fn is_not_met_by_a_title_written_as_nothing_where_the_string_read_is_not() {
                    for written in ["", " ", "<p> </p>"] {
                        let writing = format!("{CONTAINED}\n  BIND({written:?} AS ?title)");
                        let writes = nesting_titled(&writing, "First").replacing(
                            "mapping/item.rq",
                            "?v prov:specializationOf ?s .",
                            "?v prov:specializationOf ?s ; ex:title ?title .",
                        );
                        let (outcome, description) = judged(&ran(&writes), "nested");
                        assert_eq!(outcome, "failed", "{written:?}: {description}");
                        assert!(
                            description.contains("/contained/title: no urn:example:list#title"),
                            "{written:?}: {description}"
                        );
                    }
                }

                #[test]
                fn owes_a_path_in_a_nested_record_to_the_record_written_from_it() {
                    let (outcome, description) = judged(&ran(&nesting(CONTAINED)), "nested");
                    assert_eq!(outcome, "failed", "{description}");
                    assert!(
                        description.contains("/contained/title: no urn:example:list#title on its"),
                        "{description}"
                    );
                    assert!(
                        description.contains("at \"/contained/0/title\""),
                        "{description}"
                    );
                }
            }
        }
    }
}
