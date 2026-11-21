import assert from 'node:assert/strict';
import { setTimeout as wait } from 'node:timers/promises';
import { pauseCloses } from './check-export.mjs';

/**
 * Drive the receipt, its reports and its copied diagnostics through the product.
 *
 * What this proves that no other run can: the number the receipt leads with is a
 * count of committed catalog images rather than of planned ones; the destination
 * answers for those files when it is listed back; the three report files and the
 * offered verification report are the CLI's own bytes; and the text **Copy
 * diagnostics** puts on the clipboard carries the frozen field list with no path,
 * filename or identifier anywhere in it — once from a receipt, once from a
 * failure that happened before any receipt existed.
 *
 * The clipboard is read through the browser's own API, after the driver grants
 * clipboard permission over DevTools: asserting a serializer's return value would
 * not prove that a click copies anything.
 */
export async function checkReceipt(send) {
  async function evaluate(expression) {
    const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  }

  async function until(expression, detail) {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const value = await evaluate(expression);
      if (value) return value;
      await wait(75);
    }
    throw new Error(detail);
  }

  const el = (selector) => 'document.querySelector(' + JSON.stringify(selector) + ')';
  const click = (selector) => evaluate(el(selector) + '.click()');
  const text = (selector) => evaluate(el(selector) + '.textContent');
  await until(el('#source') + '?.hidden === false', 'The source screen did not load');

  const origin = await evaluate('location.origin');
  await send('Browser.grantPermissions', {
    origin,
    permissions: ['clipboardReadWrite', 'clipboardSanitizedWrite'],
  });

  // The same picker boundary every product check supplies: an accepted folder is
  // a value, and this page reads it like one.
  await evaluate(`(async () => {
    const { freshDirectory, json, mountVault } = await import('./mount.js');
    const root = await navigator.storage.getDirectory();
    globalThis.prepareReceipt = async (name, output) => {
      const entry = (await json('/native.json')).vaults.find(vault => vault.name === name);
      const fixture = await mountVault(root, 'receipt-src-' + name, '/' + name, entry.files);
      globalThis.receiptOutline = entry.files.map((path) => path).filter(Boolean);
      const source = await fixture.getDirectoryHandle(entry.catalog.split('/')[0]);
      // A vault built to fail at the open may hold no cache at all; the source
      // folder stands in, which is a value a second pick could return.
      const cache = entry.cache ? await fixture.getDirectoryHandle(entry.cache) : source;
      globalThis.receiptPicker = {
        source, cache,
        destination: await freshDirectory(root, output),
        fixture, root,
      };
    };
    globalThis.showDirectoryPicker = async ({ id, mode }) =>
      mode === 'readwrite' ? receiptPicker.destination :
        id === 'lrsalvage-cache' ? receiptPicker.cache : receiptPicker.source;
    globalThis.receiptFiles = async (dir = receiptPicker.destination) => {
      const files = {};
      async function walk(folder, prefix) {
        for await (const [name, entry] of folder.entries()) {
          if (entry.kind === 'directory') await walk(entry, prefix + name + '/');
          else {
            const bytes = new Uint8Array(await (await entry.getFile()).arrayBuffer());
            files[prefix + name] = Array.from(bytes);
          }
        }
      }
      await walk(dir, '');
      return files;
    };
  })()`);

  const decoder = new TextDecoder();
  const copy = async () => {
    await click('#receipt-diagnostics');
    await until(
      el('#receipt-diagnostics-status') + '.textContent.length > 0',
      'Copying diagnostics said nothing',
    );
    return JSON.parse(await evaluate('navigator.clipboard.readText()'));
  };

  /** The field list, and nothing beside it, at every level. */
  function assertFields(diagnostics) {
    assert.deepEqual(Object.keys(diagnostics).sort(), [
      'events', 'failure', 'failures', 'scan', 'schema', 'timings', 'versions', 'writes',
    ]);
    assert.equal(diagnostics.schema, 'lrsalvage.diagnostics');
    assert.deepEqual(Object.keys(diagnostics.versions).sort(), [
      'app', 'browser', 'build', 'core', 'platform', 'platform_version',
    ]);
    assert.deepEqual(Object.keys(diagnostics.timings).sort(), [
      'catalog_copy_ms', 'elapsed_ms', 'first_saved_jpeg_ms', 'first_thumbnail_ms',
      'longest_write_gap_ms', 'scan_ms', 'storage_setup_ms',
    ]);
    assert.deepEqual(Object.keys(diagnostics.writes).sort(), [
      'already_recovered', 'confirmed_on_disk', 'failed', 'planned', 'written',
    ]);
    assert.deepEqual(Object.keys(diagnostics.events).sort(), ['continued', 'settled', 'stopped']);
    assert.deepEqual(Object.keys(diagnostics.failures).sort(), [
      'destination_gone', 'occupied_foreign', 'other', 'out_of_room', 'permission',
      'source_unreadable', 'write_failed',
    ]);
    assert.equal(diagnostics.versions.app, '0.1.0', 'the app version is printed, not guessed');
    assert.match(String(diagnostics.versions.core), /^0\.4\.\d+$/);
    assert.equal(diagnostics.versions.browser === null, false, 'a Chromium names itself');
    for (const value of Object.values(diagnostics.timings)) {
      if (value !== null) assert.equal(typeof value, 'number', 'a timing is milliseconds');
    }
  }

  /**
   * The whole point of the curated list: the copy cannot carry what it has no
   * field for — so the fixture's own names must be absent from the text.
   */
  function assertPathFree(diagnostics, outline) {
    const copied = JSON.stringify(diagnostics);
    const names = new Set([
      'receipt-output', 'receipt-one', 'receipt-export', 'receipt-failed',
      'IMG_0001', 'IMG_0002', 'IMG_0003', 'IMG.jpg', 'Small',
      '.lrdata', '.lrcat', '.jpg', '/destination', '/cache', '/source', 'Synthetic', 'AAAA',
    ]);
    for (const path of outline) {
      for (const part of String(path).split('/')) {
        // A file name is the leak that matters. A folder name can be a word the
        // frozen field names already contain — `catalog_copy_ms` is not a path.
        if (part.includes('.') && part.length >= 5) names.add(part);
      }
    }
    for (const name of names) {
      assert.equal(copied.includes(name), false, `the copy names ${name}:\n${copied}`);
    }
  }

  async function scan() {
    await click('#pick');
    try {
      await until(el('#scan') + '?.disabled === false', 'The catalog did not become ready');
    } catch (error) {
      const state = await evaluate(`JSON.stringify({
        screen: ['#source', '#result', '#scanning'].filter((id) => document.querySelector(id)?.hidden === false),
        picker: document.querySelector('#picker-error')?.textContent ?? '',
        picked: document.querySelector('#picked')?.textContent ?? '',
      })`);
      throw new Error(`${error.message} — ${state}`);
    }
    await click('#pick-cache');
    await until(el('#picked') + '.textContent.includes("(itself)")', 'The split cache was not attached');
    await click('#scan');
    await until(el('#result') + '?.hidden === false', 'The result screen did not open');
  }

  // ---- a finished batch: the count, the reports and the copy ---------------
  await evaluate("prepareReceipt('working', 'receipt-output')");
  await scan();
  await until(el('#photo-review') + '?.hidden === false', 'The review screen did not open');
  await click('#export-pick');
  await until(el('#export-start') + '?.disabled === false', 'The destination was not accepted');
  await click('#export-start');
  await until(el('#export-status') + '.textContent.includes("3 saved") && ' +
    el('#export-start') + '.disabled === false', 'The batch did not commit three JPEGs');
  await until(el('#receipt-section') + '?.hidden === false', 'No receipt appeared after a batch');
  assert.equal(await text('#receipt-headline'), '3 photos saved');
  assert.match(await text('#receipt-detail'), /3 files in the folder/);
  assert.match(await text('#receipt-destination'), /receipt-output/);
  assert.equal(await text('#receipt-versions'), 'lrsalvage app 0.1.0 · core 0.4.0');

  const files = await evaluate('receiptFiles()');
  for (const name of ['report.csv', 'extract.csv', 'manifest.json']) {
    assert(files[name], `the batch did not write ${name}`);
  }
  const links = () => evaluate(
    "Array.from(document.querySelectorAll('#receipt-report-list a'), a => a.textContent)",
  );
  assert.deepEqual((await links()).sort(), ['extract.csv', 'manifest.json', 'report.csv']);

  // The scan report is the CLI's own bytes, root spelling aside.
  const normalise = (value) => value
    .replaceAll(/(?:[A-Za-z]:)?[^"\r\n,]*\/cli-extract\//g, '/destination/')
    .replaceAll(/(?:[A-Za-z]:)?[^"\r\n,]*\/working\/(?:[^/"\r\n,]+\/)?[^/"\r\n,]+\.lrdata\//g, '/cache/');
  const cliReport = normalise(await evaluate("(async () => (await fetch('/cli-extract/report.csv')).text())()"));
  assert.equal(decoder.decode(Uint8Array.from(files['report.csv'])), cliReport, 'app and CLI scan reports');

  // **View saved files** is the destination listed back, not a second plan.
  await click('#receipt-view');
  await until(el('#receipt-view') + '.getAttribute("aria-expanded") === "true"', 'The listing did not open');
  const listed = () => evaluate(
    "Array.from(document.querySelectorAll('#receipt-files li'), li => li.textContent)",
  );
  const saved = await listed();
  assert.equal(saved.length, 3, 'one line per saved file:\n' + saved.join('\n'));
  assert(saved.every((line) => / · [\d,]+ bytes$/.test(line)), 'each line names its size');

  // The verification report, offered rather than written: the CLI's object.
  await click('#receipt-reports summary');
  await click('#receipt-verification');
  try {
    await until(el('#receipt-report-status') + '.textContent.includes("Saved verification.json")',
      'The verification report was not written');
  } catch (error) {
    throw new Error(`${error.message} — the receipt said: ${await text('#receipt-report-status')}`);
  }
  const after = await evaluate('receiptFiles()');
  assert(after['verification.json'], 'the verification report is in the folder');
  const ours = JSON.parse(decoder.decode(Uint8Array.from(after['verification.json'])));
  const theirs = await evaluate("(async () => (await fetch('/cli-verification.json')).json())()");
  assert.deepEqual(Object.keys(ours).sort(), Object.keys(theirs).sort(), 'the CLI field set');
  delete ours.generated_unix;
  delete theirs.generated_unix;
  assert.deepEqual(ours, theirs, 'the app writes the CLI report, digests and all');

  const first = await copy();
  assertFields(first);
  assertPathFree(first, await evaluate('receiptOutline'));
  assert.equal(first.timings.storage_setup_ms === null, false, 'the store install was timed');
  assert.equal(first.timings.catalog_copy_ms === null, false, 'the copy was timed');
  assert.equal(first.timings.first_thumbnail_ms === null, false, 'review was timed');
  assert.equal(first.timings.first_saved_jpeg_ms === null, false, 'the first save was timed');
  assert.deepEqual(first.writes, {
    planned: 3, written: 3, already_recovered: 0, failed: 0, confirmed_on_disk: 3,
  });
  assert.deepEqual(first.events, { stopped: 0, settled: 1, continued: 0 });
  assert.equal(first.failure, null);
  assert.equal(first.scan.images, 4, 'the scan counts travel as the frozen names');
  assert.equal(first.scan.recovered, 4, 'four images have a preview the plan could write');
  assert.equal(first.scan.below_floor, 1);
  assert.equal(first.scan.selected_images, 3, 'the batch selected the three above the floor');
  assert.deepEqual(Object.keys(first.scan).sort(), [
    'above_cap', 'below_floor', 'bytes', 'deleted', 'failed', 'images', 'levels',
    'missing_payload', 'no_cache_entry', 'original_present', 'recovered', 'selected_images',
    'selected_items', 'suspect', 'superseded', 'tiers', 'unmapped', 'unrecognised',
  ].sort());

  // A photo a one-off reaches after a batch does not grow the count.
  await click('button[aria-label^="Preview IMG_0002"]');
  await until(el('#export-one') + '?.disabled === false', 'The one-off save was not enabled');
  await click('#export-one');
  await until(
    el('#export-one-status') + '.textContent.startsWith("Saved") || ' +
      el('#export-one-status') + '.textContent.startsWith("Already recovered")',
    'The one-off save of a file the batch wrote said nothing',
  );
  assert.equal(await text('#receipt-headline'), '3 photos saved',
    'a photo counted once must stay counted once');
  await click('#photo-close');

  // ---- a run that only ever saved one photo -------------------------------
  await click('#restart');
  await evaluate("prepareReceipt('working', 'receipt-one')");
  await scan();
  await until(el('#photo-review') + '?.hidden === false', 'The review screen did not reopen');
  await click('button[aria-label^="Preview IMG_0002"]');
  await until(el('#export-one') + '?.disabled === false', 'The one-off save was not enabled');
  await click('#export-one');
  await until(el('#export-one-status') + '.textContent.startsWith("Saved")',
    'The one-off JPEG did not commit');
  await until(el('#receipt-section') + '?.hidden === false', 'A one-off-only run got no receipt');
  assert.equal(await text('#receipt-headline'), '1 photo saved');
  assert.equal(await evaluate(el('#receipt-report-list') + '.children.length'), 0,
    'a run with no reports must not claim any');
  assert.equal(await evaluate(el('#receipt-report-note') + '?.hidden'), false,
    'and it says so rather than staying silent');
  const single = await copy();
  assert.equal(single.writes.planned, 0, 'no batch was ever started');
  assert.equal(single.writes.written, 1);
  assert.equal(single.writes.confirmed_on_disk, null, 'a one-off does not settle a batch');
  await click('#photo-close');

  // ---- stopped, settled and continued, with reports of its own ------------
  await click('#restart');
  await evaluate("prepareReceipt('export', 'receipt-export')");
  await scan();
  await until(el('#photo-review') + '?.hidden === false', 'The review screen did not reopen');
  // The below-floor preview starts unchecked; including it makes the batch 13.
  await click('button[aria-label^="Preview Small"]');
  await click('#photo-dialog-check');
  await click('#photo-close');
  await click('#export-pick');
  await until(el('#export-start') + '?.disabled === false', 'The destination was not accepted');
  await evaluate('(' + pauseCloses.toString() + ')()');
  await click('#export-start');
  await until('exportWrites.active === 8', 'The bounded writer did not fill eight slots');
  await click('#export-stop');
  await evaluate('exportWrites.resume(); true');
  await until(el('#export-status') + '.textContent.includes("Stopped") && ' +
    el('#export-start') + '.disabled === false', 'The stopped batch did not settle');
  await until(el('#receipt-section') + '?.hidden === false', 'A stopped batch got no receipt');
  assert.match(await text('#receipt-headline'), /^[1-9][\d]* photos? saved$/, 'the settled count is on the receipt');
  await click('#export-start');
  await until(el('#export-status') + '.textContent.startsWith("13 saved") && ' +
    el('#export-start') + '.disabled === false', 'Continuation did not finish');
  await until(el('#receipt-headline') + '.textContent === "13 photos saved"',
    'The continued run did not count thirteen photos');
  const resumed = await copy();
  assertFields(resumed);
  assertPathFree(resumed, await evaluate('receiptOutline'));
  assert.equal(resumed.writes.planned, 13);
  assert.equal(resumed.writes.written, 13);
  assert.equal(resumed.writes.confirmed_on_disk, 13, 'the destination answers for all of them');
  assert.equal(resumed.events.stopped, 1);
  assert.equal(resumed.events.continued, 1);
  assert(resumed.events.settled >= 2, 'a stop settles, and so does the finish');
  assert(typeof resumed.timings.longest_write_gap_ms === 'number');
  await click('#receipt-view');
  await until(el('#receipt-view') + '.getAttribute("aria-expanded") === "true"', 'The listing did not open');
  assert.equal((await listed()).length, 13, 'the folder is listed back with every saved file');

  // ---- a failure before any receipt exists --------------------------------
  await click('#restart');
  await evaluate("prepareReceipt('not-a-catalog', 'receipt-failed')");
  await scan();
  await until(el('#result-title') + '.textContent.includes("could not finish")',
    'A file that is not a catalog did not stop the scan');
  assert.equal(await evaluate(el('#receipt-section') + '.hidden'), true,
    'a run that never saved a file must not claim a receipt');
  await evaluate("Array.from(document.querySelectorAll('#causes button'), b => b.textContent === 'Copy diagnostics' && b.click())");
  await until(el('#causes p[role="status"]') + '.textContent.length > 0', 'The failure copy said nothing');
  const failure = JSON.parse(await evaluate('navigator.clipboard.readText()'));
  assertFields(failure);
  assert.deepEqual(failure.failure, { stage: 'open', kind: 'unreadable' });
  assert.equal(failure.scan, null, 'a scan that never finished has no counts');
  assert.equal(failure.writes.written, 0);
  assert.equal(failure.versions.app, '0.1.0', 'the versions are known before a receipt exists');

  return {
    savedWithBatch: 3,
    oneOffOnly: 1,
    stoppedAndContinued: 13,
    reports: 3,
    failure: 'open/unreadable',
  };
}
