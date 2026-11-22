// What the page's scan readout is held to, in a real browser (`#185`).
//
// Two entries, because they answer two questions:
//
// - `checkScanProgress` drives `web/scan-progress.js` itself with made-up
//   reports: the range and its maximum, the percentage and its 99% ceiling before
//   completion, the estimate's warm-up and its refusal to guess, the empty
//   catalog, the milestones a screen reader hears, the completion frame, and a
//   stop that a queued report cannot repaint. It is a **gate** — `scripts/
//   browser-app.sh` runs it, and `scripts/check.sh browser` with it.
// - `checkScanProgressFixture` runs a whole synthetic vault through the page's own
//   scan entry, which is what a person's catalog does. It takes minutes and is
//   **release evidence**, not a gate: the protocol and where its result is
//   recorded are `docs/agents/browser-app-deployment.md`, *Verify matching
//   progress at release scale*.
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';

const CALCULATING = 'Calculating time remaining…';

/** A DevTools Protocol session as the two checks spell it. */
function driver(send) {
  async function evaluate(expression) {
    const result = await send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description);
    return result.result.value;
  }

  async function until(expression, detail = expression, seconds = 300) {
    const deadline = Date.now() + seconds * 1000;
    while (Date.now() < deadline) {
      if (await evaluate(expression)) return;
      await sleep(50);
    }
    const state = await evaluate('globalThis.scanProgressProbe?.current?.() ?? null');
    throw new Error(`${detail} timed out. ${JSON.stringify(state)}`);
  }

  return { evaluate, until };
}

/**
 * The readout against reports this check makes up: no scan, no fixture, and every
 * branch a 5,000-image run reaches in twenty seconds on purpose.
 */
export async function checkScanProgress(send) {
  const { evaluate, until } = driver(send);

  await until('document.querySelector("#source")?.hidden === false');
  await evaluate(`(async () => {
    const { createScanProgress } = await import('./scan-progress.js');
    const get = (id) => document.getElementById(id);
    const progress = createScanProgress(document);
    get('source').hidden = true;
    get('scanning').hidden = false;
    globalThis.scanProgressProbe = {
      current: () => ({
        stage: get('stage').textContent,
        count: get('count').textContent,
        eta: get('eta').hidden ? '' : get('eta').textContent,
        status: get('scan-status').textContent,
        barHidden: get('bar').hidden,
        barDisplay: getComputedStyle(get('bar')).display,
        max: get('bar').getAttribute('max'),
        value: get('bar').getAttribute('value'),
        stopDisabled: get('stop').disabled,
      }),
    };
    globalThis.controlledProgress = progress;
    progress.reset();
  })()`);

  // An empty catalog: a sentence, no bar, and nothing to estimate.
  await evaluate('controlledProgress.progress({ phase: 2, done: 0, total: 0, at: 0 })');
  const empty = await evaluate('scanProgressProbe.current()');
  assert.equal(empty.stage, 'No catalog images to match', JSON.stringify(empty));
  assert.equal(empty.barDisplay, 'none', 'an empty catalog draws no bar');
  assert.equal(empty.eta, '', 'and offers no estimate for zero images');
  assert.equal(empty.count, '', 'and no count of a range that does not exist');

  // The range and the estimate, and the accessible bar a screen reader reports.
  await evaluate(`(() => {
    controlledProgress.reset();
    for (const at of [0, 1000, 2000, 3000]) {
      controlledProgress.progress({ phase: 2, done: at, total: 7000, at });
    }
  })()`);
  const matching = await evaluate('scanProgressProbe.current()');
  assert.equal(matching.stage, 'Matching catalog images…');
  assert.equal(matching.count, '3,000 of 7,000 (42%)');
  assert.match(matching.eta, /remaining$/, 'a steady rate has a number');
  assert.equal(matching.max, '7000', 'the bar ranges over the reported total');
  assert.equal(matching.value, '3000');
  assert.equal(matching.stopDisabled, false, 'matching can be stopped');

  const tree = await send('Accessibility.getFullAXTree');
  const bar = tree.nodes.find(
    (node) => node.role?.value === 'progressbar' && node.name?.value === 'Matching catalog images…',
  );
  assert.ok(bar, 'the progress bar is exposed with the stage as its name');
  assert.equal(bar.value?.value, 3000);
  assert.equal(
    bar.properties.find((property) => property.name === 'valuemax')?.value?.value,
    7000,
  );

  // 100% is the last image's alone: up to it the percentage stops at 99.
  await evaluate('controlledProgress.progress({ phase: 2, done: 6999, total: 7000, at: 3100 })');
  const nearEnd = await evaluate('scanProgressProbe.current()');
  assert.match(nearEnd.count, /\(99%\)$/, 'the bar is not full one image early');
  assert.equal(nearEnd.eta, CALCULATING, 'a jump is not a rate to project from');

  // Warm-up: the first second of a run has no throughput to speak of.
  await evaluate(`(() => {
    controlledProgress.reset();
    controlledProgress.progress({ phase: 2, done: 0, total: 5000, at: 0 });
    controlledProgress.progress({ phase: 2, done: 40, total: 5000, at: 1000 });
  })()`);
  assert.equal(
    await evaluate('scanProgressProbe.current().eta'),
    CALCULATING,
    'nothing is estimated before there is a rate',
  );

  // A stall: the history describes work that stopped, so the estimate goes.
  await evaluate(`(() => {
    controlledProgress.reset();
    for (const at of [0, 1000, 2000, 3000]) {
      controlledProgress.progress({ phase: 2, done: at, total: 7000, at });
    }
  })()`);
  assert.match(await evaluate('scanProgressProbe.current().eta'), /remaining$/);
  await until("scanProgressProbe.current().eta === 'Calculating time remaining…'",
    'a stall clears the estimate', 20);

  // A sustained slowdown is not a rate to project from, and a recovery is.
  const rate = await evaluate(`(() => {
    controlledProgress.reset();
    const sample = (at, done) => controlledProgress.progress({ phase: 2, done, total: 5000, at });
    for (let second = 0; second <= 8; second++) sample(second * 1000, second * 50);
    const steady = scanProgressProbe.current().eta;
    for (let second = 9; second <= 14; second++) sample(second * 1000, 400 + second - 8);
    const slowed = scanProgressProbe.current().eta;
    for (let second = 15; second <= 20; second++) sample(second * 1000, 406 + (second - 14) * 50);
    return { steady, slowed, recovered: scanProgressProbe.current().eta };
  })()`);
  assert.match(rate.steady, /^About/, 'a steady rate is projected');
  assert.match(rate.slowed, /^Calculating/, 'a sustained slowdown must not inflate the estimate');
  assert.match(rate.recovered, /^About/, 'and a recovery is a rate again');

  // What a screen reader hears: quarters, the first estimate, the end — not the
  // paint rate. Collected synchronously, because a live region's text is what
  // was written to it rather than what a later observer callback would read.
  const declared = await evaluate(`(() => {
    controlledProgress.reset();
    const region = document.getElementById('scan-status');
    const heard = [];
    const record = () => {
      if (region.textContent !== heard.at(-1)) heard.push(region.textContent);
    };
    for (let second = 0; second <= 7; second++) {
      controlledProgress.progress({ phase: 2, done: second * 500, total: 4000, at: second * 1000 });
      record();
    }
    controlledProgress.progress({ phase: 2, done: 4000, total: 4000, at: 8000 });
    record();
    // Read in the same turn: the completion's own frame turns this into result
    // preparation, and this is the state a person sees for that frame.
    return { heard, end: scanProgressProbe.current() };
  })()`);
  const milestones = declared.heard.filter((text) => text.startsWith('Matching catalog images.'));
  assert.ok(milestones.length >= 2, `quarters are announced: ${JSON.stringify(declared.heard)}`);
  assert.ok(
    declared.heard.length <= 6,
    `nine reports are not nine announcements: ${JSON.stringify(declared.heard)}`,
  );
  assert.equal(
    declared.heard.at(-1),
    'Catalog image matching complete. 4,000 of 4,000 (100%).',
    `the end is announced once, last: ${JSON.stringify(declared.heard)}`,
  );

  // The completion itself: a full bar, no estimate, no stop, and result
  // preparation only after the completion has had a frame.
  const end = declared.end;
  assert.equal(end.count, '4,000 of 4,000 (100%)');
  assert.equal(end.max, '4000');
  assert.equal(end.value, '4000');
  assert.equal(end.eta, '');
  assert.equal(end.stopDisabled, true, 'there is nothing left to stop');
  await until("scanProgressProbe.current().stage === 'Preparing scan results…'",
    'result preparation follows the completion');
  const preparing = await evaluate('scanProgressProbe.current()');
  assert.equal(preparing.count, '', 'preparation clears the count');
  assert.equal(preparing.eta, '', 'and the estimate');
  assert.equal(preparing.value, null, 'and leaves the bar indeterminate');
  assert.equal(preparing.max, null);
  assert.ok(preparing.barDisplay !== 'none', 'preparation is indeterminate, not absent');

  // A stop clears the readout and a queued report cannot repaint it.
  const stopped = await evaluate(`(() => {
    controlledProgress.stop();
    const asked = { ...scanProgressProbe.current() };
    controlledProgress.progress({ phase: 2, done: 900, total: 4000, at: 9500 });
    return { asked, queued: scanProgressProbe.current() };
  })()`);
  assert.equal(stopped.asked.stage, 'Stopping…');
  for (const text of ['count', 'eta']) {
    assert.equal(stopped.asked[text], '', `a stop clears the ${text}`);
  }
  assert.equal(stopped.asked.barDisplay, 'none', 'a stop draws no bar');
  assert.equal(stopped.queued.stage, 'Stopping…', 'a queued report cannot repaint a stop');
  assert.equal(stopped.queued.count, '');

  // Leave the page as the scan entry expects to find it, so this check can run
  // before a whole scan in the same page.
  await evaluate(`(() => {
    controlledProgress.clear();
    document.getElementById('scanning').hidden = true;
    document.getElementById('source').hidden = false;
  })()`);
  return {
    empty_catalog: true,
    accessible_range: { value: 3000, max: 7000 },
    pre_completion_99: true,
    warm_up: true,
    stall: true,
    sustained_slowdown: true,
    announcements: milestones.length,
    completion_frame: true,
    stopped_and_queued: true,
  };
}

/**
 * The same readout over the recording of a whole scan: a synthetic 5,000-image
 * vault, the page's own entry, and a Stop part-way through a second run.
 */
export async function checkScanProgressFixture(send, fixturePath) {
  const { evaluate, until } = driver(send);
  const controlled = await checkScanProgress(send);

  await until('document.querySelector("#source")?.hidden === false');
  await until('document.querySelector("#scan") !== null');

  const fixture = JSON.parse(await readFile(join(fixturePath, 'fixture.json'), 'utf8'));
  const files = (await readdir(fixturePath, { recursive: true, withFileTypes: true }))
    .filter((entry) => entry.isFile())
    .map((entry) =>
      join(entry.parentPath, entry.name).slice(fixturePath.length + 1).replaceAll('\\', '/'),
    )
    .filter((path) => path.startsWith(fixture.cache + '/') || path.startsWith('catalog/'));

  await evaluate(`(async () => {
    const fixture = ${JSON.stringify(fixture)};
    const files = ${JSON.stringify(files)};
    const { freshDirectory, writeFile } = await import('./mount.js');
    const root = await navigator.storage.getDirectory();
    const vault = await freshDirectory(root, 'progress-fixture');
    let next = 0;
    await Promise.all(Array.from({ length: 12 }, async () => {
      while (next < files.length) {
        const path = files[next++];
        const response = await fetch('/' + path.split('/').map(encodeURIComponent).join('/'));
        if (!response.ok) throw new Error(path + ': ' + response.status);
        await writeFile(vault, path, await response.arrayBuffer());
      }
    }));
    const catalog = await vault.getDirectoryHandle(fixture.catalog.split('/')[0]);
    const cache = await vault.getDirectoryHandle(fixture.cache);
    globalThis.showDirectoryPicker = async ({ id }) => (id === 'lrsalvage-cache' ? cache : catalog);

    const get = (id) => document.getElementById(id);
    const log = [];
    let events = [];
    let stopOnProgress = false;
    const WorkerClass = globalThis.Worker;
    globalThis.Worker = class extends WorkerClass {
      constructor(...args) {
        super(...args);
        this.addEventListener('message', ({ data }) => {
          if (data.type === 'progress') events.push(data);
        });
      }
    };
    function current() {
      return {
        stage: get('stage').textContent,
        count: get('count').textContent,
        eta: get('eta').hidden ? '' : get('eta').textContent,
        status: get('scan-status').textContent,
        barHidden: get('bar').hidden,
        max: get('bar').getAttribute('max'),
        value: get('bar').getAttribute('value'),
        stopDisabled: get('stop').disabled,
      };
    }
    let last = '';
    function sample() {
      if (get('scanning').hidden) return;
      const state = current();
      const signature = JSON.stringify(state);
      if (signature === last) return;
      last = signature;
      log.push({ ...state, at: performance.now() });
      if (stopOnProgress && state.stage === 'Matching catalog images…' &&
          Number(state.value) > 0 && !state.stopDisabled) {
        stopOnProgress = false;
        get('stop').click();
      }
    }
    function frame() {
      if (!get('scanning').hidden && get('count').textContent.endsWith('(100%)')) {
        globalThis.scanProgressProbe.seenAtFrame = true;
      }
      requestAnimationFrame(frame);
    }
    new MutationObserver(sample).observe(get('scanning'), {
      childList: true, subtree: true, attributes: true, characterData: true,
    });
    globalThis.scanProgressProbe = {
      current,
      log,
      seenAtFrame: false,
      events: () => events,
      clear(stop) { stopOnProgress = stop; events = []; log.length = 0; },
    };
    requestAnimationFrame(frame);
  })()`);

  async function scan(stop) {
    await evaluate(`scanProgressProbe.clear(${stop}); document.querySelector('#pick').click()`);
    await until('!document.querySelector("#scan").disabled', 'the catalog is picked');
    await evaluate('document.querySelector("#pick-cache").click()');
    await until('document.querySelector("#picked").textContent.includes("(itself)")',
      'the cache is picked');
    await evaluate('document.querySelector("#scan").click()');
  }

  // One whole scan: the recording a person's catalog leaves.
  await scan(false);
  await until('!document.querySelector("#result").hidden', 'the scan finishes');
  const result = await evaluate(
    '({ log: scanProgressProbe.log, events: scanProgressProbe.events(), seenAtFrame: scanProgressProbe.seenAtFrame })',
  );
  const matching = result.log.filter((state) => state.stage === 'Matching catalog images…');
  const imageEvents = result.events.filter((event) => event.phase === 2);
  assert.ok(matching.length > 1, 'matching paints more than once');
  assert.ok(
    result.log.some((state) => state.stage === 'Reading the preview folder…' && state.value === null),
    'the cache walk is indeterminate',
  );
  assert.ok(
    result.log.some((state) => state.stage === 'Reading preview files…' && state.max === '5000'),
    'the measured pass ranges over what the walk found',
  );
  const warmup = matching.findIndex((state) => state.eta === CALCULATING);
  assert.ok(warmup >= 0, 'the estimate starts as Calculating time remaining…');
  const estimates = matching.filter((state) => state.eta && state.eta !== CALCULATING);
  assert.ok(estimates.length > 0, 'and becomes a number once the rate is known');
  const last = matching.at(-1);
  assert.equal(last.count, '5,000 of 5,000 (100%)');
  assert.equal(last.max, '5000');
  assert.equal(last.value, '5000');
  assert.equal(last.eta, '', 'a finished stage has no estimate');
  assert.equal(last.stopDisabled, true);
  assert.ok(result.seenAtFrame, 'the full count was painted inside an animation frame');
  assert.ok(
    result.log.some(
      (state) =>
        state.stage === 'Preparing scan results…' &&
        state.stopDisabled && !state.eta && !state.count && state.value === null,
    ),
    'result preparation clears the readout and cannot be stopped',
  );
  const upward = estimates.reduce((count, state, index) => {
    const previous = estimates[index - 1];
    if (!previous) return count;
    const seconds = (text) => {
      const minutes = /About (\d+) minutes?/.exec(text);
      if (minutes) return Number(minutes[1]) * 60;
      const rest = /About (\d+) seconds/.exec(text);
      return rest ? Number(rest[1]) : 0;
    };
    return seconds(state.eta) > seconds(previous.eta) ? count + 1 : count;
  }, 0);

  // A second run, stopped during matching: the readout stays the stop, and the
  // page goes back to source selection.
  await evaluate('document.querySelector("#restart").click()');
  await scan(true);
  await until(
    '!document.querySelector("#source").hidden && ' +
      'document.querySelector("#picker-error").textContent.includes("scan was stopped")',
    'a stopped scan returns to source selection',
  );
  const stopped = await evaluate('scanProgressProbe.log');
  const stopping = stopped.findIndex((state) => state.stage === 'Stopping…');
  assert.ok(stopping >= 0, 'the stop is shown at once');
  assert.ok(
    stopped.slice(stopping).every(
      (state) =>
        state.stage === 'Stopping…' && !state.eta && !state.count && state.stopDisabled,
    ),
    'and no queued report repaints it',
  );

  return {
    matching_ms: Math.round(imageEvents.at(-1).at - imageEvents[0].at),
    matching_worker_updates: imageEvents.length,
    first_estimate_after_ms: estimates.length
      ? Math.round(estimates[0].at - matching[warmup].at)
      : null,
    first_estimate: estimates[0]?.eta ?? null,
    upward_revisions: upward,
    milestones: [...new Set(matching.map((state) => state.status))],
    saw_100_in_animation_frame: result.seenAtFrame,
    finalization_reset: true,
    stopped: true,
    controlled,
    browser: await evaluate('navigator.userAgent'),
  };
}
