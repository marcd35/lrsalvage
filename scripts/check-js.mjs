#!/usr/bin/env node
// Parse every page module and harness script, and hold the published diagnostics
// field list to the lists the page builds it from.
//
// `cargo` never parses `web/*.js`, and `sh -n` never sees `scripts/*.mjs`, so a
// broken string in either fails the day somebody runs the twelve-minute browser
// check — or, worse, in the deployed page. `vm.SourceTextModule` parses a module
// without executing it and needs no dependency, which is the same reason the
// `shell` gate is `sh -n`.
//
// What this gate cannot see, and does not claim to: a value read before it is
// initialised, or a string where a promise belongs. Those want a type checker,
// and adding one is a decision this file does not make. Parsing is the floor.
//
// The second job is a spec check, in the shape `readme.rs` and `links.rs` have:
// `specs/app.md` publishes the diagnostics object member by member, so the frozen
// lists in it are compared with the constants in `web/diagnostics.js`, and every
// member it publishes has to appear in the module that writes it. Without this,
// a renamed field leaves the contract describing something no copy contains — the
// drift class §6.4 already paid for once.
//
// Two more jobs sit on the seam between the page and the crate:
//
// - `web/app.js` names the phases `src/wasm.rs` numbers. One fact in two
//   languages, and a phase added on one side alone silently shifts every name
//   the page reads, so the two lists are compared here.
// - `web/rate.js` is the estimate's whole arithmetic. It is pure, so it is run
//   here on made-up reports: warm-up, a steady rate, a sustained slowdown and a
//   stall. The browser gate drives the same module for twelve minutes; this
//   costs milliseconds and fails first.
//
//   node --no-warnings --experimental-vm-modules scripts/check-js.mjs
import { readdir, readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const problems = [];

/** Every file this gate owns: the two pages' modules, and the scripts that drive them. */
async function files() {
  const found = [];
  for (const crate of await readdir(join(ROOT, 'crates'), { withFileTypes: true })) {
    if (!crate.isDirectory()) continue;
    const web = join(ROOT, 'crates', crate.name, 'web');
    if (!existsSync(web)) continue;
    for (const entry of await readdir(web, { withFileTypes: true })) {
      if (entry.isFile() && entry.name.endsWith('.js')) found.push(join(web, entry.name));
    }
  }
  for (const entry of await readdir(join(ROOT, 'scripts'), { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.mjs')) found.push(join(ROOT, 'scripts', entry.name));
  }
  return found.sort();
}

function parse(file, source) {
  try {
    new vm.SourceTextModule(source, { identifier: file });
  } catch (error) {
    // `stack` carries the line the parser stopped on, which is the whole point:
    // a bare "Unexpected token" over 400 lines is not a diagnosis.
    const where = String(error.stack ?? '').split('\n').slice(0, 3).filter(Boolean).join('\n  ');
    problems.push(`${file}: ${error.message}\n  ${where}`);
  }
}

const sorted = (values) => [...values].sort();

async function diagnosticsMatchTheSpec() {
  const spec = await readFile(join(ROOT, 'specs', 'app.md'), 'utf8');
  // A checkout on Windows may hold the fence with CRLF, so the newline is optional
  // before the capture but the fence must end the line.
  const block = spec.match(/```json\r?\n([\s\S]*?)```/);
  if (!block) {
    problems.push('specs/app.md: the diagnostics object has no ```json block to check');
    return;
  }
  let published;
  try {
    published = JSON.parse(block[1]);
  } catch (error) {
    problems.push(`specs/app.md: the diagnostics block is not JSON — ${error.message}`);
    return;
  }

  const module = await readFile(join(ROOT, 'crates', 'lrsalvage-app', 'web', 'diagnostics.js'), 'utf8');
  const declared = (name) => {
    const at = module.indexOf(`const ${name} = [`);
    if (at < 0) return null;
    const end = module.indexOf('];', at);
    return [...module.slice(at, end).matchAll(/'([a-z_]+)'/g)].map((match) => match[1]);
  };

  for (const [name, key] of [['SCAN_COUNTS', 'scan'], ['CACHE_SCAN_COUNTS', 'cache_scan'],
    ['FAILURE_CATEGORIES', 'failures']]) {
    const list = declared(name);
    if (list === null) {
      problems.push(`web/diagnostics.js: no ${name} to compare with the spec's ${key}`);
      continue;
    }
    const spec = sorted(Object.keys(published[key] ?? {}));
    if (JSON.stringify(sorted(list)) !== JSON.stringify(spec)) {
      problems.push(
        `${name} and specs/app.md disagree:\n` +
          `  in the module: ${sorted(list).join(', ')}\n` +
          `  in the spec:   ${spec.join(', ')}`,
      );
    }
  }

  // Every member the spec publishes has to exist in the module that writes it,
  // as a quoted key or a literal — the module builds the object by hand. The one
  // pass-through is `scan.tiers`: its five names are `plan::Stats`'s tier names,
  // published by `specs/modules/report.md`, and the page copies the object whole.
  const PASSTHROUGH = new Set(['tiers']);
  const members = new Set();
  const walk = (value) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return;
    for (const [key, inner] of Object.entries(value)) {
      if (PASSTHROUGH.has(key)) continue;
      members.add(key);
      walk(inner);
    }
  };
  walk(published);
  const absent = [...members].filter((key) => !module.includes(`'${key}'`) && !module.includes(`${key}:`));
  if (absent.length) {
    problems.push(
      `specs/app.md publishes members web/diagnostics.js never writes: ${absent.join(', ')}`,
    );
  }
}

/**
 * The phase names the page reads and the phase numbers the seam writes are one
 * fact kept in two languages, so the two lists are compared here rather than by
 * whoever adds the next phase.
 */
async function phasesMatchTheSeam() {
  const app = await readFile(join(ROOT, 'crates', 'lrsalvage-app', 'web', 'app.js'), 'utf8');
  const declared = /export const PHASES = \[([^\]]*)\]/.exec(app);
  if (!declared) {
    problems.push('web/app.js: no PHASES list to compare with the seam');
    return;
  }
  const names = [...declared[1].matchAll(/'([a-z]+)'/g)].map((match) => match[1]);

  const wasm = await readFile(join(ROOT, 'crates', 'lrsalvage-app', 'src', 'wasm.rs'), 'utf8');
  const numbered = [...wasm.matchAll(/const PHASE_([A-Z_]+): u32 = (\d+);/g)]
    .map(([, name, number]) => ({ name: name.toLowerCase(), number: Number(number) }))
    .sort((first, second) => first.number - second.number);
  if (numbered.length === 0) {
    problems.push('src/wasm.rs: no PHASE_* constants to compare with the page');
    return;
  }
  const contiguous = numbered.every((phase, index) => phase.number === index);
  if (!contiguous) {
    problems.push(
      `src/wasm.rs numbers its phases ${numbered.map((phase) => phase.number).join(', ')}: ` +
        'the page indexes them, so they have to run from zero without a gap',
    );
  }
  const fromRust = numbered.map((phase) => phase.name);
  if (JSON.stringify(fromRust) !== JSON.stringify(names)) {
    problems.push(
      'web/app.js PHASES and src/wasm.rs\'s PHASE_* disagree:\n' +
        `  in the page: ${names.join(', ') || '(none)'}\n` +
        `  in the seam: ${fromRust.join(', ')}`,
    );
  }
}

/**
 * The estimate, on made-up reports: the four rules that keep it from drifting.
 *
 * The browser gate drives this same module end to end, which costs minutes; the
 * arithmetic is pure, so the property that matters — a number that does not climb
 * while the rate holds — is checked here in milliseconds, and a sentence that
 * changes for the wrong reason fails before a browser starts.
 */
async function theEstimateHoldsSteady() {
  const { CALCULATING, approximate, createRate } = await import(
    '../crates/lrsalvage-app/web/rate.js'
  );
  const report = (done, seconds, total) => ({ done, at: seconds * 1000, total });
  const fail = (message) => problems.push(`web/rate.js: ${message}`);

  // Warm-up: three steady samples are still too few seconds of work to speak for
  // the stage, so the warm-up alone has to hold the estimate back.
  const early = createRate();
  early.add(0, 7000, 0);
  early.add(500, 7000, 1000);
  if (early.add(1000, 7000, 2000) !== null) {
    fail('an estimate appeared before the warm-up window had elapsed');
  }

  // A steady rate is projected from the average.
  const steady = createRate();
  let seconds = null;
  for (let second = 0; second <= 8; second += 1) {
    seconds = steady.add(second * 500, 40000, second * 1000);
  }
  if (seconds === null || seconds <= 0) {
    fail('a steady rate produced no estimate');
  }

  // A sustained slowdown of more than a factor of two is not a rate to project
  // from, and a recovery is one again.
  const slowed = createRate();
  for (let second = 0; second <= 8; second += 1) slowed.add(second * 500, 40000, second * 1000);
  let duringSlowdown = null;
  for (let second = 9; second <= 14; second += 1) {
    duringSlowdown = slowed.add(4000 + (second - 8) * 5, 40000, second * 1000);
  }
  if (duringSlowdown !== null) {
    fail('a sustained slowdown was still projected as a number');
  }
  let recovered = null;
  for (let second = 15; second <= 22; second += 1) {
    recovered = slowed.add(4030 + (second - 14) * 500, 40000, second * 1000);
  }
  if (recovered === null) fail('a recovered rate was not projected again');

  // A stall clears the history: the samples before it describe work that stopped.
  const stalled = createRate();
  for (let second = 0; second <= 8; second += 1) stalled.add(second * 500, 40000, second * 1000);
  if (stalled.add(4500, 40000, 11000) !== null) {
    fail('a report after a two-second stall was projected from the stalled history');
  }

  // The sentences, at the precision the measurement has.
  const sentences = [
    [4, 'A few seconds remaining'],
    [55, 'About 55 seconds remaining'],
    [92, 'About 2 minutes remaining'],
  ];
  for (const [value, expected] of sentences) {
    if (approximate(value) !== expected) {
      fail(`${value} seconds reads "${approximate(value)}", not "${expected}"`);
    }
  }
  if (CALCULATING !== 'Calculating time remaining…') {
    fail('the warm-up sentence changed');
  }
}

/**
 * The cache suggestion, on made-up folder listings.
 *
 * `#191` was a prefix match: `2023 Helper.lrdata` shares the catalog's stem,
 * sorted first, and was offered as the cache the app would then fail to read. A
 * name list in and a name out, so the rule is held here rather than in the
 * twelve-minute browser run that would otherwise be the only thing to notice.
 */
async function theCacheSuggestionIsExact() {
  const { isCompanionCache, suggestedCache } = await import(
    '../crates/lrsalvage-app/web/folders.js'
  );
  const fail = (message) => problems.push(`web/folders.js: ${message}`);

  for (const name of ['2023 Smart Previews.lrdata', '2023 Helper.lrdata']) {
    if (!isCompanionCache(name)) fail(`${name} is not read as a companion folder`);
  }
  if (isCompanionCache('2023 Previews.lrdata')) {
    fail('the standard previews folder is read as a companion folder');
  }

  // The reported bug: all three siblings, and `Helper` sorts first.
  const real = ['2023 Helper.lrdata', '2023 Previews.lrdata', '2023 Smart Previews.lrdata'];
  if (suggestedCache(real, '2023.lrcat') !== '2023 Previews.lrdata') {
    fail(`a real catalog suggested ${suggestedCache(real, '2023.lrcat')}`);
  }

  // A companion folder alone is never offered, however alone it is.
  for (const only of ['2023 Smart Previews.lrdata', '2023 Helper.lrdata']) {
    if (suggestedCache([only], '2023.lrcat') !== null) {
      fail(`${only} was offered as the cache`);
    }
  }

  // A renamed cache is a real backup, and the only candidate.
  if (suggestedCache(['2023 Backup.lrdata'], '2023.lrcat') !== '2023 Backup.lrdata') {
    fail('a single renamed cache was not offered');
  }

  // Two candidates and no exact match is a guess, so nothing is suggested.
  const ambiguous = ['2003 Previews.lrdata', '2024 Previews.lrdata'];
  if (suggestedCache(ambiguous, '2023.lrcat') !== null) {
    fail(`two candidate caches suggested ${suggestedCache(ambiguous, '2023.lrcat')}`);
  }

  // The exact cache wins over a same-stem sibling that is not one.
  const both = ['2023 (2) Previews.lrdata', '2023 Previews.lrdata'];
  if (suggestedCache(both, '2023.lrcat') !== '2023 Previews.lrdata') {
    fail(`an exact cache lost to a sibling: ${suggestedCache(both, '2023.lrcat')}`);
  }

  // No catalog named yet: the single candidate is still the answer, and nothing
  // is invented when there are none.
  if (suggestedCache(['2023 Previews.lrdata'], null) !== '2023 Previews.lrdata') {
    fail('a single cache was not suggested before a catalog was chosen');
  }
  if (suggestedCache(['2023 Helper.lrdata'], null) !== null) {
    fail('a companion folder was suggested before a catalog was chosen');
  }
}

/**
 * Sizes a person reads: decimal units, two places, the same rule everywhere.
 *
 * `#192` asked for decimal units because a drive's capacity and File Explorer's
 * MB are decimal; a binary reading of the same batch is a different number from
 * the one the disk reports.
 */
async function theSizesRead() {
  const { formatBytes } = await import('../crates/lrsalvage-app/web/size.js');
  const fail = (message) => problems.push(`web/size.js: ${message}`);
  const cases = [
    [0, '0 B'],
    [999, '999 B'],
    [1_000, '1.00 kB'],
    [12_345, '12.35 kB'],
    [1_500_000, '1.50 MB'],
    [999_999_999, '1000.00 MB'],
    [1_200_000_000, '1.20 GB'],
  ];
  for (const [bytes, expected] of cases) {
    if (formatBytes(bytes) !== expected) {
      fail(`${bytes} bytes reads "${formatBytes(bytes)}", not "${expected}"`);
    }
  }
  // A number the page could not have must not reach a person as "NaN MB".
  for (const odd of [-1, Number.NaN, undefined]) {
    if (formatBytes(odd) !== '0 B') fail(`${odd} reads "${formatBytes(odd)}"`);
  }
}

const scanned = await files();
for (const file of scanned) parse(file, await readFile(file, 'utf8'));
await diagnosticsMatchTheSpec();
await phasesMatchTheSeam();
await theEstimateHoldsSteady();
await theCacheSuggestionIsExact();
await theSizesRead();

if (problems.length) {
  for (const problem of problems) console.error(problem);
  console.error(`\n${problems.length} problem(s) in ${scanned.length} modules`);
  process.exit(1);
}
console.log(
  `${scanned.length} modules parse, the diagnostics list matches its spec, ` +
    'the phase list matches the seam, the estimate holds steady, the cache ' +
    'suggestion is exact, and the sizes read in decimal units',
);
