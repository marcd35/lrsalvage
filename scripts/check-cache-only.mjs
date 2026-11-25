import assert from 'node:assert/strict';
import { setTimeout as wait } from 'node:timers/promises';
import { pauseCloses } from './check-export.mjs';

async function mount(caseName, freshOutput) {
  const { freshDirectory, json, mountVault } = await import('./mount.js');
  const root = await navigator.storage.getDirectory();
  const entry = (await json('/native.json')).cache_only.find((item) => item.name === caseName);
  const cache = await mountVault(root, caseName, '/' + caseName, entry.files);
  const destination = freshOutput
    ? await freshDirectory(root, caseName + '-output')
    : await root.getDirectoryHandle(caseName + '-output');
  globalThis.cacheCheck = { cache, destination, target: destination };
  globalThis.showDirectoryPicker = async ({ mode }) =>
    mode === 'readwrite' ? cacheCheck.target : cacheCheck.cache;
  return entry;
}

async function savedFiles() {
  const files = {};
  for await (const [name, handle] of cacheCheck.destination.entries()) {
    if (!name.endsWith('.jpg')) continue;
    const file = await handle.getFile();
    const hash = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
    files[name] = {
      bytes: file.size,
      sha256: Array.from(new Uint8Array(hash), (byte) => byte.toString(16).padStart(2, '0')).join(''),
    };
  }
  return files;
}

async function report(name) {
  const handle = await cacheCheck.destination.getFileHandle(name);
  return (await handle.getFile()).text();
}

export async function checkCacheOnly(send) {
  async function evaluate(expression) {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression, returnByValue: true, awaitPromise: true,
    });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  }

  const call = (fn, ...args) => evaluate('(' + fn.toString() + ')(...' + JSON.stringify(args) + ')');
  const el = (id) => 'document.getElementById(' + JSON.stringify(id) + ')';
  const click = (id) => evaluate(el(id) + '.click()');
  const text = (id) => evaluate(el(id) + '.textContent');
  async function until(expression, reason) {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      if (await evaluate(expression)) return;
      await wait(75);
    }
    throw new Error(reason);
  }

  await until(el('source') + '?.hidden === false', 'The source screen did not load');

  // The receipt's copied diagnostics, which a headless browser neither prompts
  // for nor grants. The permission names have moved between Chromium versions,
  // so the descriptor form is tried and the legacy names after it; the assertion
  // reads the real clipboard, so an ungranted run fails loudly.
  const origin = await evaluate('location.origin');
  async function grantClipboard() {
    const attempts = [
      () => send('Browser.setPermission', {
        origin,
        permission: { name: 'clipboard-read', allowWithoutSanitization: true },
        setting: 'granted',
      }),
      () => send('Browser.setPermission', {
        origin,
        permission: { name: 'clipboard-write', allowWithoutSanitization: true },
        setting: 'granted',
      }),
      () => send('Browser.grantPermissions', {
        origin,
        permissions: ['clipboardReadWrite', 'clipboardSanitizedWrite'],
      }),
    ];
    let granted = 0;
    for (const attempt of attempts) {
      try {
        await attempt();
        granted++;
      } catch {
        // The next spelling of the same request.
      }
    }
    if (granted === 0) throw new Error('Chromium refused every clipboard permission request');
  }
  await grantClipboard();

  // What the phase measured, returned as the runbook asks a check to: the object
  // is the phase's printed result, not a number nobody reads.
  const measured = {
    layouts: 0, previewsSaved: 0, alreadyRecovered: 0, conflicts: 0, indexProblems: 0,
    reusedCache: 0,
  };
  for (const layout of ['legacy', 'modern', 'mixed']) {
    const entry = await call(mount, 'cache-' + layout, true);
    await click('recover-cache');
    await until('!' + el('result') + '.hidden', 'Cache-only recovery did not reach review');
    assert.equal(await text('scanning-title'), '2. Scanning - cache-' + layout);
    assert.match(await text('headline'), /13 of 16 indexed pyramids/);
    assert.match(await text('causes'), /1 missing · 1 invalid · 1 unsupported/);
    assert.match(await text('cache-notice'), /original files, filenames, folder structure, and catalog metadata/i);
    assert.equal(await evaluate('document.querySelectorAll(".review-tile input:checked").length'), 13);

    if (layout === 'mixed') {
      await evaluate('cacheCheck.target = cacheCheck.cache; true');
      await click('export-pick');
      // `#191`: the refusal is its own block, not a status line below the gallery.
      await until(el('export-refusal') + '.hidden === false && ' +
        el('export-refusal-message') + '.textContent.includes("outside the catalog")',
      'The overlapping destination was accepted');
      assert.equal(await evaluate(el('export-start') + '.disabled'), true);
      await evaluate('cacheCheck.target = cacheCheck.destination; true');
    }
    await click('export-pick');
    await until('!' + el('export-start') + '.disabled', 'Cannot save to the fixture destination');
    assert.equal(await evaluate(el('export-refusal') + '.hidden'), true,
      'The refusal survived a destination that was accepted');
    if (layout === 'mixed') await call(pauseCloses);
    await click('export-start');
    if (layout === 'mixed') {
      await until('exportWrites.active === 8', 'The fixture writes did not reach the writer limit');
      await click('export-stop');
      await evaluate('exportWrites.resume()');
      await until(el('export-start') + '.textContent === "Continue saving" && !' + el('export-start') + '.disabled',
        'Stop did not leave a resumable batch');
      assert.equal(Object.keys(await call(savedFiles)).length, 8);
      await click('export-start');
    }
    await until(el('receipt-report-list') + '.children.length === 3', 'Recovery reports were not saved');
    assert.match(await text('receipt-headline'), /^13 files saved to /);
    assert.equal(await text('receipt-reconciled'), '');
    assert.equal(await evaluate(el('receipt-complete') + '.open'), true);
    await evaluate('document.querySelector("#receipt-complete button[type=submit]").click()');
    await until('!' + el('receipt-complete') + '.open', 'OK did not dismiss the completion dialog');
    assert.equal(await evaluate(el('receipt-files') + '.hidden'), true);
    assert.deepEqual(await call(savedFiles), Object.fromEntries(entry.manifest.map(({ output, bytes, sha256 }) =>
      [output, { bytes, sha256 }])));
    const manifest = JSON.parse(await call(report, 'manifest.json'));
    assert.equal(manifest.schema, 'lrsalvage.manifest');
    assert.equal(manifest.mode, 'cache-only');
    assert.equal(manifest.layout, entry.report.layout);
    assert.deepEqual(manifest.scan, entry.report.scan);
    assert.deepEqual(manifest.batch, { saved: 13, already_recovered: 0, failed: 0 });
    await click('receipt-verification');
    await until(el('receipt-report-status') + '.textContent.includes("Saved")', 'Verification report was not saved');
    const verification = JSON.parse(await call(report, 'verification.json'));
    assert.equal(verification.schema, 'lrsalvage.verification');
    assert.equal(verification.mode, 'cache-only');
    assert.deepEqual(verification.scan, entry.report.scan);
    assert.deepEqual(verification.writes, { saved: 13, already_recovered: 0, failed: 0, confirmed_on_disk: 13 });
    if (layout === 'mixed') {
      // The copied diagnostics: the same frozen object, saying which mode it
      // came from and carrying the five cache counts under their own names.
      await click('receipt-diagnostics');
      await until(el('receipt-diagnostics-status') + '.textContent.length > 0',
        'Copying diagnostics said nothing');
      const diagnostics = JSON.parse(await evaluate('navigator.clipboard.readText()'));
      assert.equal(diagnostics.mode, 'cache-only');
      assert.equal(diagnostics.scan, null);
      assert.deepEqual(Object.keys(diagnostics.cache_scan).sort(),
        ['indexed', 'invalid', 'missing', 'recoverable', 'unsupported']);
      assert.equal(diagnostics.cache_scan.indexed, 16);
      assert.equal(diagnostics.cache_scan.recoverable, 13);
      assert(!JSON.stringify(diagnostics).includes('preview-'),
        'the copy names no generated filename');
    }
    measured.layouts++;
    measured.previewsSaved += 13;
    assert(!JSON.stringify(verification).includes('preview-'));
    await click('restart');
  }

  await call(mount, 'cache-modern', true);
  await click('recover-cache');
  await until('!' + el('result') + '.hidden', 'Two-destination scan failed');
  await click('export-pick');
  await until('!' + el('export-start') + '.disabled', 'Could not set the first destination');
  await evaluate('document.querySelector(".review-open").click()');
  await until('!' + el('export-one') + '.disabled', 'The one-off save did not open');
  await click('export-one');
  await until(el('receipt-headline') + '.textContent.startsWith("1 file saved to")',
    'The one-off receipt did not list the saved file');
  await click('photo-close');
  await call(async () => {
    const { freshDirectory } = await import('./mount.js');
    cacheCheck.target = await freshDirectory(await navigator.storage.getDirectory(), 'cache-other-output');
  });
  await click('export-pick');
  await until(el('export-destination') + '.textContent.includes("cache-other-output")',
    'Could not set the second destination');
  await click('photo-uncheck-visible');
  await evaluate('document.querySelectorAll(".review-tile input")[0].click()');
  await evaluate('document.querySelectorAll(".review-tile input")[1].click()');
  await click('export-start');
  await until(el('receipt-report-list') + '.children.length === 3', 'Second destination reports were not saved');
  assert.match(await text('export-status'), /^2 saved of 2/);
  assert.equal(await evaluate(el('receipt-complete') + '.open'), true,
    'Completion must verify the batch destination even when the receipt keeps an earlier path');
  assert.match(await text('receipt-complete-destination'), /cache-other-output/);
  await evaluate('document.querySelector("#receipt-complete button[type=submit]").click()');
  assert.match(await text('receipt-headline'), /^2 files saved to /);
  assert.equal(await text('receipt-reconciled'), '');
  measured.previewsSaved += 2;
  await click('restart');

  const repeat = await call(mount, 'cache-mixed', false);
  await call(async (path) => {
    const { writeFile } = await import('./mount.js');
    const file = await (await cacheCheck.destination.getFileHandle(path)).getFile();
    const bytes = new Uint8Array(await file.arrayBuffer());
    bytes[bytes.length - 3] ^= 1;
    await writeFile(cacheCheck.destination, path, bytes);
  }, repeat.manifest[0].output);
  const occupied = await call(savedFiles);
  await click('recover-cache');
  await until('!' + el('result') + '.hidden', 'Second scan did not reach review');
  await click('export-pick');
  await until('!' + el('export-start') + '.disabled', 'Cannot resume using the same folder');
  await click('export-start');
  await until(el('export-conflict') + '.open', 'Existing previews were not listed');
  await evaluate('document.querySelector("#export-conflict [value=skip]").click()');
  await until(el('receipt-report-list') + '.children.length === 3', 'Second run reports were not saved');
  assert.match(await text('receipt-headline'), /^12 of 13 files recovered/);
  assert.match(await text('receipt-detail'), /1 skipped/);
  assert.deepEqual(await call(savedFiles), occupied);
  assert.deepEqual(JSON.parse(await call(report, 'manifest (2).json')).batch,
    { saved: 0, already_recovered: 12, failed: 0, omitted: 1 });
  measured.alreadyRecovered += 12;
  measured.conflicts += 1;
  await click('restart');

  for (const [name, message] of [
    ['cache-no-index', 'no previews.db'],
    ['cache-bad-index', 'could not be read'],
    ['cache-unsupported-index', 'unsupported schema'],
  ]) {
    await call(mount, name, true);
    await click('recover-cache');
    await until('!' + el('result') + '.hidden', 'Bad index did not report a result');
    assert((await text('headline')).includes(message));
    assert.equal(await evaluate(el('export-section') + '.hidden'), true);
    measured.indexProblems++;
    await click('restart');
  }
  // A supported index with no entries is its own answer, not a failure.
  await call(mount, 'cache-empty-index', true);
  await click('recover-cache');
  await until('!' + el('result') + '.hidden', 'The empty index did not report a result');
  assert.equal(await text('headline'), 'This preview index is empty.');
  assert.equal(await evaluate(el('export-section') + '.hidden'), true);
  await click('restart');
  await call(mount, 'cache-modern', true);
  await evaluate(`(() => {
    const native = FileSystemDirectoryHandle.prototype.entries;
    globalThis.restoreCacheEntries = () => { FileSystemDirectoryHandle.prototype.entries = native; };
    FileSystemDirectoryHandle.prototype.entries = function () {
      if (this.name === 'A000') throw new DOMException('Test shard refused', 'NotAllowedError');
      return native.call(this);
    };
  })()`);
  try {
    await click('recover-cache');
    await until('!' + el('result') + '.hidden', 'Unreadable shard did not report a result');
    assert.match(await text('headline'), /could not read the entire preview cache/);
    assert.equal(await evaluate(el('export-section') + '.hidden'), true);
    await click('restart');
  } finally {
    await evaluate('restoreCacheEntries()');
  }

  await call(async () => {
    const { json, mountVault } = await import('./mount.js');
    const entry = (await json('/native.json')).vaults.find(({ name }) => name === 'not-a-catalog');
    const root = await navigator.storage.getDirectory();
    const fixture = await mountVault(root, 'cache-fallback-source', '/not-a-catalog', entry.files);
    const catalog = await fixture.getDirectoryHandle(entry.catalog.split('/')[0]);
    const cache = await fixture.getDirectoryHandle(entry.cache);
    globalThis.cacheFallbackPicks = [];
    globalThis.showDirectoryPicker = async ({ id }) => {
      cacheFallbackPicks.push(id);
      return id === 'lrsalvage-cache' ? cache : catalog;
    };
  });
  await click('pick');
  await until('!' + el('pick-cache') + '.hidden', 'Catalog picker did not accept the fixture');
  await click('pick-cache');
  await until(el('picked') + '.textContent.includes("(itself)")', 'Cache picker did not accept the fixture');
  await click('scan');
  await until('!' + el('recover-cache-after-error') + '.hidden', 'Unreadable catalog did not offer cache-only recovery');
  await click('recover-cache-after-error');
  await until(el('headline') + '.textContent.includes("indexed pyramids") || ' +
    el('headline') + '.textContent.includes("index is empty")', 'Selected cache was not reused');
  assert.deepEqual(await evaluate('cacheFallbackPicks'), ['lrsalvage-catalog', 'lrsalvage-cache']);
  measured.reusedCache++;
  console.log('cache-only: legacy, modern and mixed output parity, index errors, overlap, stop/continue and occupied files agree');
  return measured;
}
