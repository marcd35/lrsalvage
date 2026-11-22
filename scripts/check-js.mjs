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

const scanned = await files();
for (const file of scanned) parse(file, await readFile(file, 'utf8'));
await diagnosticsMatchTheSpec();

if (problems.length) {
  for (const problem of problems) console.error(problem);
  console.error(`\n${problems.length} problem(s) in ${scanned.length} modules`);
  process.exit(1);
}
console.log(`${scanned.length} modules parse, and the diagnostics field list matches its spec`);
