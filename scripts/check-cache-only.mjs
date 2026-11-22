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
  for (const layout of ['legacy', 'modern', 'mixed']) {
    const entry = await call(mount, 'cache-' + layout, true);
    await click('recover-cache');
    await until('!' + el('result') + '.hidden', 'Cache-only recovery did not reach review');
    assert.match(await text('headline'), /13 of 16 indexed pyramids/);
    assert.match(await text('causes'), /1 missing · 1 invalid · 1 unsupported/);
    assert.match(await text('cache-notice'), /original files, filenames, folder structure, and catalog metadata/i);
    assert.equal(await evaluate('document.querySelectorAll(".review-tile input:checked").length'), 13);

    if (layout === 'mixed') {
      await evaluate('cacheCheck.target = cacheCheck.cache; true');
      await click('export-pick');
      await until(el('export-status') + '.textContent.length > 0', 'The overlapping destination was accepted');
      assert.equal(await evaluate(el('export-start') + '.disabled'), true);
      await evaluate('cacheCheck.target = cacheCheck.destination; true');
    }
    await click('export-pick');
    await until('!' + el('export-start') + '.disabled', 'Cannot save to the fixture destination');
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
    assert.equal(await text('receipt-headline'), '13 previews saved');
    assert.match(await text('receipt-reconciled'), /Every file above was found/);
    assert.deepEqual(await call(savedFiles), Object.fromEntries(entry.manifest.map(({ output, bytes, sha256 }) =>
      [output, { bytes, sha256 }])));
    const manifest = JSON.parse(await call(report, 'manifest.json'));
    assert.equal(manifest.mode, 'cache-only');
    assert.equal(manifest.layout, entry.report.layout);
    assert.deepEqual(manifest.scan, entry.report.scan);
    assert.deepEqual(manifest.batch, { saved: 13, already_recovered: 0, failed: 0 });
    await click('receipt-verification');
    await until(el('receipt-report-status') + '.textContent.includes("Saved")', 'Verification report was not saved');
    const verification = JSON.parse(await call(report, 'verification.json'));
    assert.deepEqual(verification.scan, entry.report.scan);
    assert.deepEqual(verification.writes, { saved: 13, already_recovered: 0, failed: 0, confirmed_on_disk: 13 });
    assert(!JSON.stringify(verification).includes('preview-'));
    await click('restart');
  }

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
  await until(el('receipt-report-list') + '.children.length === 3', 'Second run reports were not saved');
  assert.match(await text('receipt-detail'), /12 already recovered · 1 could not be saved/);
  assert.deepEqual(await call(savedFiles), occupied);
  assert.deepEqual(JSON.parse(await call(report, 'manifest (2).json')).batch,
    { saved: 0, already_recovered: 12, failed: 1 });
  await click('restart');

  for (const [name, message] of [
    ['cache-no-index', 'no previews.db'],
    ['cache-bad-index', 'could not be read'],
  ]) {
    await call(mount, name, true);
    await click('recover-cache');
    await until('!' + el('result') + '.hidden', 'Bad index did not report a result');
    assert((await text('headline')).includes(message));
    assert.equal(await evaluate(el('export-section') + '.hidden'), true);
    await click('restart');
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
  console.log('cache-only: legacy, modern and mixed output parity, index errors, overlap, stop/continue and occupied files agree');
}
