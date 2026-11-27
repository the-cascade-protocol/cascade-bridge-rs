use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::Quad;
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use std::path::PathBuf;
use std::process::Command;

/// The specification's checkout, beside this one or wherever
/// `CASCADE_BRIDGE_SPEC` names it.
fn specification() -> PathBuf {
    match std::env::var_os("CASCADE_BRIDGE_SPEC") {
        Some(named) => PathBuf::from(named),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../cascade-bridge-spec"),
    }
}

/// Passing the specification's own adapters · cascade-bridge test
///
/// The Bridge specification ships two small adapters whose test manifests hold an entry of every
/// test type. A Bridge that passes them judges an adapter's tests as the specification says, so
/// these run the whole of each through `test`.
mod passing_the_specifications_adapters {
    use super::*;

    /// passes every entry of the synthetic XML adapter that a Bridge can judge
    #[test]
    #[ignore = "reads a checkout of the-cascade-protocol/cascade-bridge-spec beside this one"]
    fn passes_every_entry_of_the_synthetic_xml_adapter_that_a_bridge_can_judge() {
        passes_every_entry_of("synthetic-adapter");
    }

    /// passes every entry of the synthetic JSON adapter that a Bridge can judge
    #[test]
    #[ignore = "reads a checkout of the-cascade-protocol/cascade-bridge-spec beside this one"]
    fn passes_every_entry_of_the_synthetic_json_adapter_that_a_bridge_can_judge() {
        passes_every_entry_of("synthetic-json-adapter");
    }
}

/// Failing the entries the specification's adapter lists as failing · cascade-bridge test
///
/// One of the specification's adapters names, on each entry a Bridge must fail, the check that
/// fails it. The report of `test` is judged by the specification's own query, which finds each
/// entry reported otherwise than its manifest lists.
mod failing_the_entries_the_specification_lists {
    use super::*;

    /// reports each entry of the synthetic adapter with failing entries as its manifest lists
    #[test]
    #[ignore = "reads a checkout of the-cascade-protocol/cascade-bridge-spec beside this one"]
    fn reports_each_entry_of_the_synthetic_adapter_with_failing_entries_as_its_manifest_lists() {
        let specification = specification();
        let adapter = specification.join("fixtures/synthetic-json-adapter-with-failing-entries");
        let report = std::env::temp_dir().join(format!(
            "cascade-bridge-failing-entries-{}.earl.ttl",
            std::process::id()
        ));
        let run = Command::new(env!("CARGO_BIN_EXE_cascade-bridge"))
            .arg("test")
            .arg(&adapter)
            .arg("--vocabularies")
            .arg(specification.join("fixtures/synthetic-vocabularies"))
            .arg("--earl")
            .arg(&report)
            .output()
            .expect("run the command");
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let store = Store::new().expect("a store");
        for (file, base) in [
            (
                adapter.join("fixtures/manifest.ttl"),
                "https://example.org/adapter/fixtures/manifest.ttl",
            ),
            (report.clone(), "https://example.org/report.ttl"),
        ] {
            let bytes = std::fs::read(&file)
                .unwrap_or_else(|e| panic!("{}: {e}\n{printed}", file.display()));
            let quads: Vec<Quad> = RdfParser::from_format(RdfFormat::Turtle)
                .with_base_iri(base)
                .expect("a base")
                .for_slice(&bytes)
                .collect::<Result<_, _>>()
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            store.extend(quads).expect("loaded");
        }
        let _ = std::fs::remove_file(&report);
        let judge = std::fs::read_to_string(specification.join("engine/faults-of-a-report.rq"))
            .expect("the specification's judge");
        let QueryResults::Solutions(solutions) = SparqlEvaluator::new()
            .parse_query(&judge)
            .expect("the judge parses")
            .on_store(&store)
            .execute()
            .expect("the judge runs")
        else {
            panic!("the judge is a SELECT");
        };
        let faults: Vec<String> = solutions
            .map(|solution| {
                let solution = solution.expect("a row");
                solution
                    .get("fault")
                    .map(ToString::to_string)
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(faults, Vec::<String>::new(), "{printed}");
        assert_eq!(run.status.code(), Some(1), "{printed}");
    }
}

fn passes_every_entry_of(synthetic: &str) {
    let specification = specification();
    let adapter = specification.join("fixtures").join(synthetic);
    let vocabularies = specification.join("fixtures/synthetic-vocabularies");
    assert!(
        adapter.is_dir(),
        "no synthetic adapter at {}",
        adapter.display()
    );
    let run = Command::new(env!("CARGO_BIN_EXE_cascade-bridge"))
        .arg("test")
        .arg(&adapter)
        .arg("--vocabularies")
        .arg(&vocabularies)
        .output()
        .expect("run the command");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let printed = format!("{stdout}{}", String::from_utf8_lossy(&run.stderr));
    // An entry's line: its outcome, its name, its time, and what the run said.
    let entries: Vec<(&str, &str, &str)> = stdout
        .lines()
        .filter(|line| line.starts_with("  "))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let outcome = fields.next()?;
            let name = fields.next()?;
            let (_, said) = line.split_once(" s  ")?;
            Some((outcome, name, said))
        })
        .collect();
    // An entry this Bridge cannot judge is let through only for the reason its kind gives.
    assert!(!entries.is_empty(), "{printed}");
    for (outcome, name, said) in &entries {
        let excused = match *outcome {
            "passed" => true,
            "cantTell" => said.contains("bridge:InputOnlyTest"),
            "untested" => said.contains("datasets are not fetched"),
            _ => false,
        };
        assert!(excused, "{name} is {outcome}: {said}\n{printed}");
    }
    assert!(
        entries.iter().any(|(outcome, ..)| *outcome == "passed"),
        "{printed}"
    );
    assert_eq!(run.status.code(), Some(0), "{printed}");
}
