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
