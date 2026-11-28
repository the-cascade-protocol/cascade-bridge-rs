// The cascade-bridge command on Node, over the package's Node entry, which
// `sh package/build.sh` builds.
import {
  closeSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join } from "node:path";
import { Adapter, describe, Survey, test } from "../../package/dist/node.js";
import { at, complete, fileIri, Folder, iriOf, KINDS, METADATA, reason, Refusal } from "./host.mjs";

const USAGE = `usage: cascade-bridge test <adapter-dir> [--vocabularies <directory>] [--earl <out.ttl>] [--datasets] [--owed]
       cascade-bridge convert <adapter-dir> <document> [--envelope <iri>] [--facts <file>] [--vocabularies <directory>] [--out <file>] [--findings <file>] [--format turtle|ntriples] [--owed]
       cascade-bridge survey <adapter-dir> <folder> [--envelope <iri>] [--tsv <file>] [--kind <kind>] [--under <path>]
       cascade-bridge library <calls file> <results directory>

--owed lists each occurrence of a path the accounting says is carried, or whose gap the adapter reports, that a conversion owes and neither writes nor reports, one per line: after test's entries, every one an entry failed for; in place of convert's graph, the document's.
survey converts nothing: it lifts each file directly in <folder> and reads each path of its records against the adapter's accounting, then prints the paths no entry covers, the paths whose entry says no Cascade term carries them, and the paths whose entry says they are left out on purpose, ranked by the documents that hold each, and lists each file it could not read. A path is a place: one written in a bare record and in an envelope's content member is one row, saying the forms it was written in. --tsv writes every place it saw to <file>, the forms as its last column; --kind keeps one record kind; --under keeps a place and the places below it; --envelope reads every document in that envelope. It exits 0 once the adapter was read, and 2 where it was not, an empty folder included.`;

// What a survey leaves unloaded: it lifts and reads the accounting, and checks and maps nothing.
const SURVEY_SKIPS = ["schema", "shapes", "findings"];

const FAILING = ["failed", "inapplicable"];

// The IRI the file at `path` will have once it is written, read as it would be then.
function iriOfWritten(path) {
  try {
    return fileIri(realpathSync.native(path));
  } catch {
    return fileIri(join(at(path, () => realpathSync.native(dirname(path))), basename(path)));
  }
}

function opened(directory, vocabularies) {
  const adapter = new Folder(directory);
  const vocabulary = vocabularies === undefined ? undefined : new Folder(vocabularies);
  const description = describe(adapter.iri, adapter.read(METADATA));
  vocabulary?.with(description.vocabulary?.files ?? []);
  return [{ adapter, vocabulary }, description];
}

function parse(argv) {
  const [command, ...rest] = argv;
  const next = () => {
    if (rest.length === 0) throw new Refusal(USAGE);
    return rest.shift();
  };
  if (command === "test") {
    const parsed = { command, directory: next(), datasets: false };
    while (rest.length > 0) {
      const flag = rest.shift();
      if (flag === "--vocabularies") parsed.vocabularies = next();
      else if (flag === "--earl") parsed.earl = next();
      else if (flag === "--datasets") parsed.datasets = true;
      else if (flag === "--owed") parsed.owed = true;
      else return undefined;
    }
    return parsed;
  }
  if (command === "convert") {
    const parsed = { command, directory: next(), document: next(), format: "turtle" };
    while (rest.length > 0) {
      const flag = rest.shift();
      if (flag === "--envelope") parsed.envelope = next();
      else if (flag === "--facts") parsed.facts = next();
      else if (flag === "--vocabularies") parsed.vocabularies = next();
      else if (flag === "--out") parsed.out = next();
      else if (flag === "--findings") parsed.findings = next();
      else if (flag === "--owed") parsed.owed = true;
      else if (flag === "--format") {
        parsed.format = next();
        if (!["turtle", "ntriples"].includes(parsed.format)) return undefined;
      } else return undefined;
    }
    return parsed;
  }
  if (command === "survey") {
    const parsed = { command, directory: next(), folder: next() };
    while (rest.length > 0) {
      const flag = rest.shift();
      if (flag === "--envelope") parsed.envelope = next();
      else if (flag === "--tsv") parsed.tsv = next();
      else if (flag === "--kind") parsed.kind = next();
      else if (flag === "--under") parsed.under = next();
      else return undefined;
    }
    return parsed;
  }
  if (command === "library") {
    const parsed = { command, calls: next(), results: next() };
    return rest.length === 0 ? parsed : undefined;
  }
  return undefined;
}

// Once the module traps, its instance cannot be replaced in this process, so every
// later call that would use it is a failure of the Bridge too.
let trapped;

function guarded(call) {
  if (trapped !== undefined) {
    return { failure: { kind: "bridgeFailure", message: `the module trapped earlier: ${trapped}` } };
  }
  try {
    return { answer: call() };
  } catch (error) {
    if (error?.name === "BridgeError") {
      const failure = { kind: KINDS[error.kind], message: error.message };
      if (error.kind === "missing") Object.assign(failure, { map: error.map, path: error.path });
      return { failure };
    }
    if (error instanceof WebAssembly.RuntimeError) {
      trapped = error.message;
      return { failure: { kind: "bridgeFailure", message: error.message } };
    }
    throw error;
  }
}

function mapOf({ iri, files }) {
  return {
    iri,
    files: new Map(
      Object.entries(files).map(([key, path]) => [key, new Uint8Array(at(path, () => readFileSync(path)))]),
    ),
  };
}

function mapsOf(listed) {
  return {
    adapter: mapOf(listed.adapter),
    vocabulary: listed.vocabulary === undefined ? undefined : mapOf(listed.vocabulary),
  };
}

function documentOf({ document }) {
  return {
    iri: document.iri,
    bytes: new Uint8Array(at(document.path, () => readFileSync(document.path))),
    envelope: document.envelope,
    facts:
      document.facts === undefined
        ? undefined
        : {
            iri: document.facts.iri,
            bytes: new Uint8Array(at(document.facts.path, () => readFileSync(document.facts.path))),
          },
  };
}

function runLibrary(parsed) {
  const listed = JSON.parse(at(parsed.calls, () => readFileSync(parsed.calls, "utf8")));
  for (const { name, calls } of listed.cases) {
    const directory = join(parsed.results, name);
    at(directory, () => mkdirSync(directory, { recursive: true }));
    // The latest load of the case that did not fail, and the adapter it loaded while it stands.
    let maps;
    let loaded;
    const adapter = () => {
      if (loaded === undefined) {
        const reloaded = guarded(() => Adapter.load(maps.adapter, maps.vocabulary));
        if (reloaded.failure !== undefined) return reloaded;
        loaded = reloaded.answer;
      }
      return { answer: loaded };
    };
    calls.forEach((call, index) => {
      const n = index + 1;
      const put = (extension, bytes) => writeFileSync(join(directory, `${n}.${extension}`), bytes);
      const [[operation, given]] = Object.entries(call);
      let result;
      if (operation === "describe") {
        const metadata = new Uint8Array(readFileSync(given.metadata));
        result = guarded(() => describe(given.adapter, metadata, given.format));
        if (result.failure === undefined) put("graph", result.answer.graph);
      } else if (operation === "load") {
        const listedMaps = mapsOf(given);
        result = guarded(() => Adapter.load(listedMaps.adapter, listedMaps.vocabulary));
        if (result.failure === undefined) {
          loaded?.free();
          maps = listedMaps;
          loaded = result.answer;
        }
      } else if (operation === "ask" || operation === "convert") {
        if (maps === undefined) return;
        const document = documentOf(given);
        result = adapter();
        if (result.failure === undefined) {
          const on = result.answer;
          result =
            operation === "ask"
              ? guarded(() => on.accepts(document))
              : guarded(() =>
                  on.convert(document, { format: given.format, findings: given.findings, census: given.census }),
                );
        }
        if (result.failure === undefined && operation === "convert") {
          put("graph", result.answer.graph);
          if (result.answer.findings !== undefined) put("findings", result.answer.findings);
        }
        if (result.failure?.kind === "bridgeFailure") loaded = undefined;
      } else if (operation === "test") {
        const listedMaps = mapsOf(given);
        result = guarded(() => test(listedMaps.adapter, listedMaps.vocabulary));
        if (result.failure === undefined) put("report", result.answer.earl);
      } else {
        throw new Refusal(`${parsed.calls}: the calls file names the operation ${operation}`);
      }
      const answer =
        result.failure !== undefined
          ? { failure: result.failure }
          : operation === "ask"
            ? { answer: result.answer }
            : {};
      put("json", `${JSON.stringify(answer)}\n`);
    });
    loaded?.free();
  }
  return 0;
}

function adapterLine(identifier, iri) {
  return `Adapter  ${identifier ?? iri}  (${iri})`;
}

function tally(outcomes) {
  const counts = new Map();
  for (const outcome of outcomes) counts.set(outcome, (counts.get(outcome) ?? 0) + 1);
  return [...counts].map(([outcome, n]) => `${n} ${outcome}`).join(", ");
}

function runTest(parsed) {
  const [inputs, description] = opened(parsed.directory, parsed.vocabularies);
  inputs.adapter.withEveryFile();
  const report = complete(inputs, (adapter, vocabulary) =>
    test(adapter, vocabulary, { datasets: parsed.datasets }),
  );
  const offers = report.bridge.profiles.map((p) => p.split("#")[1] ?? p).join(", ");
  let said = `${adapterLine(description.identifier, description.iri)}\nBridge   ${report.bridge.name} ${report.bridge.version}, offers ${offers}\n\n`;
  const width = Math.max(4, ...report.entries.map(({ name }) => name.length));
  for (const entry of report.entries) {
    said += `  ${entry.outcome.padEnd(12)} ${entry.name.padEnd(width)}  ${entry.seconds.toFixed(2).padStart(6)} s  ${entry.description}\n`;
  }
  if (parsed.owed) {
    said += "\n";
    for (const entry of report.entries) {
      for (const line of entry.unmet) said += `Owed     ${entry.name}  ${line}\n`;
    }
  }
  said += `\n${tally(report.entries.map(({ outcome }) => outcome))}\n`;
  process.stdout.write(said);
  if (parsed.earl !== undefined) {
    at(parsed.earl, () => writeFileSync(parsed.earl, report.earl));
    process.stdout.write(`EARL     ${parsed.earl}\n`);
  }
  return report.entries.some(({ outcome }) => FAILING.includes(outcome)) ? 1 : 0;
}

function runSurvey(parsed) {
  const [inputs, description] = opened(parsed.directory, undefined);
  inputs.adapter.withEveryFile();
  const adapter = complete(inputs, (named, vocabulary) =>
    Adapter.loadForMeasurement(named, vocabulary, SURVEY_SKIPS),
  );
  const survey = new Survey();
  try {
    let envelope;
    if (parsed.envelope !== undefined) {
      try {
        envelope = new URL(parsed.envelope, `${inputs.adapter.iri}${METADATA}`).href;
      } catch (error) {
        throw new Refusal(`--envelope ${parsed.envelope}: ${error.message}`);
      }
    }
    adapter.checkSurvey(envelope);
    const folder = at(parsed.folder, () => realpathSync.native(parsed.folder));
    for (const entry of at(folder, () => readdirSync(folder, { withFileTypes: true }))) {
      if (entry.isDirectory()) continue;
      if (entry.isSymbolicLink()) {
        survey.notRead(entry.name, "a symbolic link, which is not read");
        continue;
      }
      const path = join(folder, entry.name);
      let bytes;
      let iri;
      try {
        bytes = new Uint8Array(readFileSync(path));
        iri = iriOf(path);
      } catch (error) {
        survey.notRead(entry.name, reason(error));
        continue;
      }
      adapter.survey(survey, entry.name, { iri, bytes, envelope });
    }
    const report = survey.report({ kind: parsed.kind, under: parsed.under });
    if (parsed.tsv !== undefined) at(parsed.tsv, () => writeFileSync(parsed.tsv, report.tsv));
    const tsv = parsed.tsv === undefined ? "" : `TSV      ${parsed.tsv}\n`;
    process.stdout.write(`${adapterLine(description.identifier, description.iri)}\n${report.text}${tsv}`);
    return 0;
  } finally {
    survey.free();
    adapter.free();
  }
}

function runConvert(parsed) {
  const [inputs, description] = opened(parsed.directory, parsed.vocabularies);
  inputs.adapter.withEveryFile();
  const adapter = complete(inputs, (named, vocabulary) => Adapter.load(named, vocabulary));
  try {
    const bytes = at(parsed.document, () => new Uint8Array(readFileSync(parsed.document)));
    const iri = iriOf(parsed.document);
    let envelope;
    if (parsed.envelope !== undefined) {
      try {
        envelope = new URL(parsed.envelope, `${inputs.adapter.iri}${METADATA}`).href;
      } catch (error) {
        throw new Refusal(`--envelope ${parsed.envelope}: ${error.message}`);
      }
    }
    const facts =
      parsed.facts === undefined
        ? undefined
        : {
            iri: iriOf(parsed.facts),
            bytes: at(parsed.facts, () => new Uint8Array(readFileSync(parsed.facts))),
          };
    if (parsed.owed) {
      const unmet = adapter.unmet({ iri, bytes, envelope, facts });
      const lines = unmet.map((line) => `  ${line}\n`).join("");
      process.stdout.write(`Owed     ${iri}  ${unmet.length} unmet\n${lines}`);
      return 0;
    }
    const conversion = adapter.convert(
      { iri, bytes, envelope, facts },
      {
        format: parsed.format,
        findings: parsed.findings !== undefined,
        census: parsed.findings !== undefined,
        findingsRelativeTo: parsed.findings === undefined ? undefined : iriOfWritten(parsed.findings),
      },
    );
    if (parsed.findings !== undefined && conversion.unvalidated !== undefined) {
      throw new Refusal(conversion.unvalidated);
    }
    if (conversion.unvalidated !== undefined) {
      process.stderr.write(`cascade-bridge: ${conversion.unvalidated}\n`);
    }
    const detect =
      conversion.detected === undefined
        ? "the adapter names no bridge:detectQuery"
        : conversion.detected
          ? "true"
          : "false: this adapter does not claim this document, converted anyway";
    process.stderr.write(
      `${adapterLine(description.identifier, description.iri)}\nDocument ${iri}  ${conversion.records} record(s), ${conversion.triples} triples, ${conversion.findingCount} finding(s)\nDetect   ${detect}\n`,
    );
    // Before the graph, so a findings file that cannot be written leaves standard output empty.
    if (parsed.findings !== undefined) {
      // Not emptied first, so a run that fails leaves a committed oracle as it was.
      at(parsed.findings, () => closeSync(openSync(parsed.findings, "a")));
      at(parsed.findings, () => writeFileSync(parsed.findings, conversion.findings ?? new Uint8Array()));
      process.stderr.write(`Findings ${parsed.findings}\n`);
    }
    if (parsed.out !== undefined) {
      at(parsed.out, () => writeFileSync(parsed.out, conversion.graph));
      process.stderr.write(`Graph    ${parsed.out}\n`);
    } else {
      process.stdout.write(conversion.graph);
    }
    return 0;
  } finally {
    adapter.free();
  }
}

// A pipe's reader may go away mid-graph; the caller is owed a status, not an exception.
process.stdout.on("error", (error) => {
  process.stderr.write(`cascade-bridge: standard output: ${error.message}\n`);
  process.exitCode = 2;
});

function said(error) {
  if (error instanceof WebAssembly.RuntimeError) {
    return `a fault in the bridge, whose module trapped: ${error.message}`;
  }
  return error instanceof Error ? error.message : String(error);
}

let parsed;
try {
  parsed = parse(process.argv.slice(2));
} catch {
  parsed = undefined;
}
if (parsed === undefined) {
  process.stderr.write(`${USAGE}\n`);
  process.exitCode = 2;
} else {
  try {
    process.exitCode =
      parsed.command === "test"
        ? runTest(parsed)
        : parsed.command === "library"
          ? runLibrary(parsed)
          : parsed.command === "survey"
            ? runSurvey(parsed)
            : runConvert(parsed);
  } catch (error) {
    process.stderr.write(`cascade-bridge: ${said(error)}\n`);
    process.exitCode = 2;
  }
}
