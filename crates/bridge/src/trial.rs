use crate::accounting::ForwardMiss;
use crate::budget::Budgets;
use crate::compiled::Executor;
use crate::error::Result;
use crate::harness::whole_graph_entries;
use crate::library::Named;
use crate::load::load_adapter;
use crate::query::Profile;
use crate::records::Supplied;
use crate::resolver::Maps;
use crate::run::{convert_with, prepare, Asked, Prepared, Source};
use crate::vocabulary::require_vocabularies;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;
use std::time::Duration;
use web_time::Instant;

struct Row {
    name: String,
    without: Duration,
    with: Duration,
    check: Duration,
    misses: Vec<ForwardMiss>,
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort();
    times.get(times.len() / 2).copied().unwrap_or_default()
}

fn milliseconds(time: Duration) -> String {
    format!("{:.2} ms", time.as_secs_f64() * 1000.0)
}

fn percent(more: Duration, base: Duration) -> String {
    if base.is_zero() {
        return "n/a".to_owned();
    }
    format!("{:+.1}%", (more.as_secs_f64() / base.as_secs_f64() - 1.0) * 100.0)
}

fn percent_of(part: Duration, whole: Duration) -> String {
    if whole.is_zero() {
        return "n/a".to_owned();
    }
    format!("{:.1}%", part.as_secs_f64() / whole.as_secs_f64() * 100.0)
}

fn once(
    prepared: &Prepared,
    entry: &crate::harness::WholeGraphEntry,
    forward: bool,
) -> Result<(Duration, Duration, Vec<ForwardMiss>)> {
    prepared.forward.store(forward, Ordering::Relaxed);
    prepared.forward_nanos.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let run = convert_with(
        prepared,
        Source {
            iri: &entry.input,
            envelope: entry.envelope.as_deref(),
            bytes: &entry.bytes,
            facts: entry
                .facts
                .as_ref()
                .map(|(turtle, iri)| Supplied { iri, turtle }),
        },
        &Profile::new(Executor::default(), false, Budgets::default()),
        Asked::ALL,
    )?;
    let elapsed = started.elapsed();
    let check = Duration::from_nanos(prepared.forward_nanos.load(Ordering::Relaxed));
    Ok((elapsed, check, run.forward_misses))
}

/// A trial, not for merge: every whole-graph entry converted with and without the
/// forward accounting check, `repetitions` times each after one round left out.
pub fn trial_forward_accounting(
    adapter: Named<'_>,
    vocabulary: Option<Named<'_>>,
    repetitions: usize,
) -> Result<String> {
    let maps = Maps {
        adapter,
        vocabulary,
    };
    let loaded = load_adapter(&maps)?;
    require_vocabularies(&loaded, &maps)?;
    let prepared = prepare(&loaded, &maps)?;
    let entries = whole_graph_entries(&loaded, &maps)?;
    let mut rows: Vec<Row> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    'entries: for entry in &entries {
        let (mut without, mut with, mut check) = (Vec::new(), Vec::new(), Vec::new());
        let mut misses = Vec::new();
        for round in 0..=repetitions {
            let order = if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for forward in order {
                match once(&prepared, entry, forward) {
                    Ok((elapsed, counted, found)) => {
                        if round == 0 {
                            continue;
                        }
                        if forward {
                            with.push(elapsed);
                            check.push(counted);
                            misses = found;
                        } else {
                            without.push(elapsed);
                        }
                    }
                    Err(error) => {
                        failed.push(format!("{}: {error}", entry.name));
                        continue 'entries;
                    }
                }
            }
        }
        rows.push(Row {
            name: entry.name.clone(),
            without: median(&mut without),
            with: median(&mut with),
            check: median(&mut check),
            misses,
        });
    }

    let mut said = String::new();
    let _ = writeln!(
        said,
        "{} whole-graph entries timed, {} failed to convert; {repetitions} timed rounds each of both modes, after one round left out, the order alternating; compiled executor, findings and census asked.\n",
        rows.len(),
        failed.len()
    );
    for failure in &failed {
        let _ = writeln!(said, "- failed: {failure}");
    }
    let mut without: Vec<Duration> = rows.iter().map(|row| row.without).collect();
    let mut with: Vec<Duration> = rows.iter().map(|row| row.with).collect();
    let mut check: Vec<Duration> = rows.iter().map(|row| row.check).collect();
    let mut overhead: Vec<f64> = rows
        .iter()
        .filter(|row| !row.without.is_zero())
        .map(|row| row.with.as_secs_f64() / row.without.as_secs_f64() - 1.0)
        .collect();
    overhead.sort_by(f64::total_cmp);
    let total_without: Duration = without.iter().sum();
    let total_with: Duration = with.iter().sum();
    let total_check: Duration = check.iter().sum();
    let slowest = |pick: fn(&Row) -> Duration| {
        rows.iter()
            .max_by_key(|row| pick(row))
            .map(|row| format!("{} ({})", milliseconds(pick(row)), row.name))
            .unwrap_or_default()
    };
    let (median_without, median_with, median_check) =
        (median(&mut without), median(&mut with), median(&mut check));
    let _ = writeln!(said, "| | median entry | slowest entry | all entries |");
    let _ = writeln!(said, "|---|---|---|---|");
    let _ = writeln!(
        said,
        "| without the check | {} | {} | {} |",
        milliseconds(median_without),
        slowest(|row| row.without),
        milliseconds(total_without)
    );
    let _ = writeln!(
        said,
        "| with the check | {} | {} | {} |",
        milliseconds(median_with),
        slowest(|row| row.with),
        milliseconds(total_with)
    );
    let _ = writeln!(
        said,
        "| the check's own clock | {} | {} | {} |",
        milliseconds(median_check),
        slowest(|row| row.check),
        milliseconds(total_check)
    );
    let _ = writeln!(
        said,
        "\nAdded by the check: median entry {}, all entries {}; the check's own clock is {} of the time without it.",
        percent(median_with, median_without),
        percent(total_with, total_without),
        percent_of(total_check, total_without)
    );
    if let (Some(low), Some(high)) = (overhead.first(), overhead.last()) {
        let middle = overhead[overhead.len() / 2];
        let _ = writeln!(
            said,
            "Per entry (wall clock, with against without): lowest {:+.1}%, median {:+.1}%, highest {:+.1}%.",
            low * 100.0,
            middle * 100.0,
            high * 100.0
        );
    }

    let missed: Vec<&Row> = rows.iter().filter(|row| !row.misses.is_empty()).collect();
    let _ = writeln!(
        said,
        "\n{} entries report a miss; {} misses in all.\n",
        missed.len(),
        missed.iter().map(|row| row.misses.len()).sum::<usize>()
    );
    let mut by_path: BTreeMap<(&str, String), Vec<&str>> = BTreeMap::new();
    for row in &missed {
        for miss in &row.misses {
            let wanted: Vec<String> = miss
                .wanted
                .iter()
                .map(|(class, property)| {
                    format!(
                        "{}/{}",
                        class.rsplit(['#', '/']).next().unwrap_or(class),
                        property.rsplit(['#', '/']).next().unwrap_or(property)
                    )
                })
                .collect();
            by_path
                .entry((miss.path.as_str(), wanted.join(" | ")))
                .or_default()
                .push(row.name.as_str());
        }
    }
    if !by_path.is_empty() {
        let _ = writeln!(said, "| path | carried as | entries | the entries |");
        let _ = writeln!(said, "|---|---|---|---|");
        for ((path, wanted), names) in &by_path {
            let mut names = names.clone();
            names.sort_unstable();
            names.dedup();
            let shown: Vec<&str> = names.iter().copied().take(6).collect();
            let more = if names.len() > shown.len() {
                format!(" and {} more", names.len() - shown.len())
            } else {
                String::new()
            };
            let length = path.chars().count();
            let path = if length > 170 {
                let tail: String = path.chars().skip(length - 160).collect();
                format!("...{tail}")
            } else {
                (*path).to_owned()
            };
            let _ = writeln!(
                said,
                "| `{path}` | {wanted} | {} | {}{more} |",
                names.len(),
                shown.join(", ")
            );
        }
    }
    Ok(said)
}
