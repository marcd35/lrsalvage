#!/usr/bin/env node
// What a recorded progress log says the estimate did.
//
// `node scripts/scan-progress-trace.mjs <trace.json>` replays a recorded scan's
// matching reports through `web/rate.js` — the same arithmetic the page draws —
// and reports what a person would have watched: the first estimate and when it
// appeared, every sentence the estimate showed, and how many of those revisions
// moved the number upwards.
//
// That last number is the whole point. `#185`'s first estimator projected from a
// three-second window and moved upwards 55 times in one completed run, each time
// claiming more work was left than it had said a moment before; the replacement
// projects from average throughput and refuses to guess while the recent rate
// disagrees, which took the same trace to one revision. Replaying a trace is how
// that property is checked without a browser, and the browser gate's own run
// reports the same number from the DOM.
//
// The shape it reads is `{events: [{type, phase, done, total, at}]}` — the
// worker's own reports, as the fixture driver records them — or a bare array of
// those events. A `log` member beside `events` is ignored: it records what the
// page showed, and this file recomputes that rather than trusting it.
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  CALCULATING,
  STALL_MS,
  approximate,
  createRate,
  secondsOf,
} from '../crates/lrsalvage-app/web/rate.js';

/** The phase number the wasm module reports for matching (`web/app.js`, `PHASES`). */
const MATCHING_PHASE = 2;

/** The events that are matching reports, in order. */
export function matchingEvents(document) {
  const events = Array.isArray(document) ? document : (document.events ?? []);
  return events.filter((event) => event.phase === MATCHING_PHASE);
}

/**
 * Replay one run's matching reports as the page would have drawn them.
 *
 * A gap of a stall between two reports fires the page's own timer first, so the
 * replay inserts the *Calculating time remaining…* a person would have seen
 * before the next report lands. The completion report ends the stage and shows
 * no estimate, exactly as the page does.
 */
export function replay(document) {
  const rate = createRate();
  const revisions = [];
  let reports = 0;
  let integer = null;
  let previousAt = null;
  let startedAt = null;
  let stalls = 0;
  let upward = 0;
  let first = null;

  function show(at, text) {
    if (revisions.at(-1)?.text === text) return;
    revisions.push({ at, text });
    const seconds = secondsOf(text);
    if (seconds === null) return;
    if (integer !== null && seconds > integer) upward += 1;
    integer = seconds;
    first ??= { text, at };
  }

  for (const event of matchingEvents(document)) {
    startedAt ??= event.at;
    if (previousAt !== null && event.at - previousAt >= STALL_MS) {
      stalls += 1;
      rate.stall();
      show(previousAt + STALL_MS, CALCULATING);
    }
    previousAt = event.at;
    reports += 1;
    if (event.done === event.total) break;
    const seconds = rate.add(event.done, event.total, event.at);
    show(event.at, seconds === null ? CALCULATING : approximate(seconds));
  }

  return {
    reports,
    matching_ms: previousAt !== null && startedAt !== null ? Math.round(previousAt - startedAt) : 0,
    stalls,
    revisions: revisions.length,
    upward_revisions: upward,
    first_estimate: first?.text ?? null,
    first_estimate_after_ms:
      first !== null && startedAt !== null ? Math.round(first.at - startedAt) : null,
    last_estimate: [...revisions].reverse().find((revision) => secondsOf(revision.text) !== null)
      ?.text ?? null,
    shown: revisions,
  };
}

async function main(argv) {
  const file = argv.find((arg) => !arg.startsWith('-'));
  if (!file) {
    console.error('usage: node scripts/scan-progress-trace.mjs <trace.json> [--json]');
    process.exit(2);
  }
  const document = JSON.parse(await readFile(file, 'utf8'));
  const result = replay(document);
  if (argv.includes('--json')) {
    console.log(JSON.stringify(result));
    return;
  }
  console.log(`trace ${file}`);
  console.log(`  ${result.reports} matching reports over ${(result.matching_ms / 1000).toFixed(1)} s`);
  console.log(
    `  first estimate   ${result.first_estimate ?? 'none'}` +
      (result.first_estimate_after_ms === null ? '' : ` (${result.first_estimate_after_ms} ms in)`),
  );
  console.log(`  last estimate    ${result.last_estimate ?? 'none'}`);
  console.log(`  revisions shown  ${result.revisions}`);
  console.log(`  upward revisions ${result.upward_revisions}`);
  console.log(`  stalls cleared   ${result.stalls}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}
