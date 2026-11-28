use crate::finding_rows::Finding;
use crate::terms::{
    BRIDGE_ADDRESS_NOT_ONE_NODE, BRIDGE_PREDICATE_NOT_DECLARED, CASCADE_SOURCE_PATH_NOT_ACCOUNTED,
    SH_VIOLATION,
};
use crate::validate::names_a_schema_rule;
use std::collections::BTreeSet;

const SHACL: &str = "http://www.w3.org/ns/shacl#";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Census,
    SourceSchema,
    OutputValidation,
    UndeclaredPredicate,
    UnresolvedAddress,
    Note,
}

fn kind_of(finding: &Finding) -> Kind {
    let body = finding.body.as_str();
    if body == CASCADE_SOURCE_PATH_NOT_ACCOUNTED {
        Kind::Census
    } else if names_a_schema_rule(body) {
        Kind::SourceSchema
    } else if body == BRIDGE_PREDICATE_NOT_DECLARED {
        Kind::UndeclaredPredicate
    } else if body == BRIDGE_ADDRESS_NOT_ONE_NODE {
        Kind::UnresolvedAddress
    } else if body.starts_with(SHACL) && finding.severity.as_deref() == Some(SH_VIOLATION) {
        Kind::OutputValidation
    } else {
        Kind::Note
    }
}

fn said(finding: &Finding) -> String {
    let mut said = match &finding.row {
        Ok(row) => row.to_string(),
        Err(_) => format!("<{}>", finding.body),
    };
    for (name, carried) in [
        ("sh:resultPath", &finding.path),
        ("sh:focusNode", &finding.focus),
    ] {
        if let Some(carried) = carried {
            said.push_str(&format!(", {name} {carried}"));
        }
    }
    said
}

/// What a conversion test fails on whatever else it compares. A census finding is judged
/// against the paths the test names; a finding that means something is wrong, unless the
/// test lists the findings it expects.
pub(crate) fn problems(
    findings: &[Finding],
    named: &BTreeSet<String>,
    lists_findings: bool,
) -> Vec<String> {
    let mut problems = BTreeSet::new();
    let mut drawn = BTreeSet::new();
    for finding in findings {
        let kind = kind_of(finding);
        if kind == Kind::Census {
            if let Some(path) = &finding.value {
                drawn.insert(path.clone());
            }
            continue;
        }
        let what = match kind {
            Kind::SourceSchema => "the input breaks its source schema",
            Kind::OutputValidation => "the output breaks the vocabulary's shapes at sh:Violation",
            Kind::UndeclaredPredicate => {
                "the output writes a predicate no vocabulary file of the adapter declares"
            }
            Kind::UnresolvedAddress => {
                "an address of the adapter's findings queries selects other than one node"
            }
            Kind::Census | Kind::Note => continue,
        };
        if !lists_findings {
            problems.insert(format!(
                "{what}: {} (a test lists it in a bridge:expectedFindingRows)",
                said(finding)
            ));
        }
    }
    for path in drawn.difference(named) {
        problems.insert(format!(
            "the conversion draws cascade:SourcePathNotAccounted at {path}, which the test does not name by bridge:testsUnaccountedSourcePath"
        ));
    }
    for path in named.difference(&drawn) {
        problems.insert(format!(
            "the test names {path} by bridge:testsUnaccountedSourcePath, and the conversion draws no cascade:SourcePathNotAccounted at it"
        ));
    }
    problems.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::problems;
    use crate::finding_rows::Finding;
    use crate::terms::{
        BRIDGE_ADDRESS_NOT_ONE_NODE, BRIDGE_PREDICATE_NOT_DECLARED, BRIDGE_SCHEMA_RULE_UNNAMED,
        BRIDGE_VERSION_NOT_KEPT, CASCADE_SOURCE_PATH_NOT_ACCOUNTED, SH_INFO, SH_VIOLATION,
        SH_WARNING,
    };
    use std::collections::BTreeSet;

    fn finding(body: &str, severity: &str, value: Option<&str>) -> Finding {
        Finding {
            body: body.to_owned(),
            severity: Some(severity.to_owned()),
            value: value.map(str::to_owned),
            path: None,
            focus: None,
            row: Err("not a row".to_owned()),
        }
    }

    fn unlisted(findings: &[Finding]) -> Vec<String> {
        problems(findings, &BTreeSet::new(), false)
    }

    /// Running an adapter's test manifest · test(), TestOptions, TestReport
    mod running_an_adapter_s_test_manifest {
        use super::*;

        /// `test(adapter, vocabulary, options)`
        mod test {
            use super::*;

            #[test]
            fn fails_each_kind_of_finding_that_means_something_is_wrong_where_no_findings_are_listed(
            ) {
                let wrong = [
                    "https://www.w3.org/TR/xmlschema-1/#cvc-elt",
                    "https://www.w3.org/TR/xmlschema-2/#cvc-datatype-valid",
                    "https://datatracker.ietf.org/doc/html/draft-wright-json-schema-validation-01#section-6.28",
                    BRIDGE_SCHEMA_RULE_UNNAMED,
                    BRIDGE_PREDICATE_NOT_DECLARED,
                    BRIDGE_ADDRESS_NOT_ONE_NODE,
                    "http://www.w3.org/ns/shacl#MinCountConstraintComponent",
                ];
                for body in wrong {
                    let said = unlisted(&[finding(body, SH_VIOLATION, None)]);
                    assert_eq!(said.len(), 1, "{body}: {said:?}");
                    assert!(said[0].contains(body), "{said:?}");
                    assert!(
                        problems(&[finding(body, SH_VIOLATION, None)], &BTreeSet::new(), true)
                            .is_empty(),
                        "{body} is fine where the test lists its findings"
                    );
                }
            }

            #[test]
            fn fails_nothing_that_is_a_note() {
                let notes = [
                    finding(
                        "https://ns.cascadeprotocol.org/adapter/x#gap",
                        SH_VIOLATION,
                        None,
                    ),
                    finding(BRIDGE_VERSION_NOT_KEPT, SH_WARNING, Some("kept")),
                    finding(
                        "http://www.w3.org/ns/shacl#MaxLengthConstraintComponent",
                        SH_WARNING,
                        None,
                    ),
                    finding(
                        "http://www.w3.org/ns/shacl#MaxLengthConstraintComponent",
                        SH_INFO,
                        None,
                    ),
                ];
                assert!(unlisted(&notes).is_empty(), "{:?}", unlisted(&notes));
            }

            #[test]
            fn judges_a_census_path_against_the_paths_the_test_names_whether_or_not_it_lists_findings(
            ) {
                let census = |path| finding(CASCADE_SOURCE_PATH_NOT_ACCOUNTED, SH_INFO, Some(path));
                let named: BTreeSet<String> = ["/a".to_owned()].into();
                for lists in [false, true] {
                    assert!(problems(&[census("/a")], &named, lists).is_empty());
                    assert_eq!(problems(&[census("/b")], &named, lists).len(), 2);
                    assert_eq!(problems(&[], &named, lists).len(), 1);
                }
            }
        }
    }
}
