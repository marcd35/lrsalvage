import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { setTimeout as wait } from 'node:timers/promises';
import { pauseCloses } from './check-export.mjs';

/**
 * The indexless recovery phase (#198).
 *
 * One cache with no `previews.db`, its `.lrprev` containers mounted into
 * origin-private storage, and the product page driven through the mode the same
 * way a person would: choose the third option on the start screen, review, pick
 * a destination, save, and read the receipt back. Every number and every byte is
 * compared with the native `lrsalvage_app::scan_legacy` over the same fixture, so
 * the page cannot drift from the module under it.
 *
 * What it deliberately does not do is answer a File System Access dialog: no
 * automation on this machine can (see `scripts/browser-app.sh`'s header), so the
 * picker's accepted answer is supplied as the value a pick returns.
 */

async function mount(caseName, freshOutput) {
  const { freshDirectory, json, mountVault } = await import('./mount.js');
  const root = await navigator.storage.getDirectory();
  const entry = (await json('/native.json')).legacy.find((item) => item.name === caseName);
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

function controls(send) {
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

  return { evaluate, call, el, click, text, until };
}

export async function checkLegacy(send) {
  const { evaluate, call, el, click, text, until } = controls(send);
  await until(el('source') + '?.hidden === false', 'The source screen did not load');
  assert.equal(await evaluate(el('recover-legacy') + '.hidden || ' + el('recover-legacy') + '.disabled'),
    false, 'The indexless mode is not offered on the start screen');

  // The receipt's copied diagnostics, which a headless browser neither prompts
  // for nor grants; the assertion reads the real clipboard.
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

  const measured = {
    cases: 0, previewsSaved: 0, alreadyRecovered: 0, conflicts: 0, stops: 0, fallback: 0,
  };

  for (const name of ['legacy-sizes', 'legacy-tie', 'legacy-damaged', 'legacy-outside']) {
    const entry = await call(mount, name, true);
    const counts = entry.report.scan;
    await click('recover-legacy');
    await until('!' + el('result') + '.hidden', 'The indexless recovery did not reach review');
    assert.match(
      await text('headline'),
      new RegExp(`${counts.usable} of ${counts.containers} preview containers`),
      `${name}: the headline does not agree with the native scan`,
    );
    assert.match(await text('cache-notice'), /original files, filenames, folder structure, and catalog metadata/i);
    assert.equal(
      await evaluate('document.querySelectorAll(".review-tile input:checked").length'),
      counts.usable,
      `${name}: every usable preview starts checked`,
    );
    const facts = await text('checked');
    for (const [label, value] of [
      ['Preview containers', counts.containers],
      ['Usable', counts.usable],
      ['Invalid', counts.invalid],
      ['Unsupported', counts.unsupported],
    ]) {
      assert.equal(
        await evaluate(`(() => {
          const terms = [...document.querySelectorAll('#checked dt')];
          const term = terms.find((node) => node.textContent === ${JSON.stringify(label)});
          return term ? term.nextElementSibling.textContent : null;
        })()`),
        String(value),
        `${name}: the ${label} count does not agree with the native scan`,
      );
    }

    // A destination inside the cache is refused: the folders this run reads are
    // not a place to write into, and the refusal is its own block.
    await evaluate('cacheCheck.target = cacheCheck.cache; true');
    await click('export-pick');
    await until(el('export-refusal') + '.hidden === false && ' +
      el('export-refusal-message') + '.textContent.includes("outside the catalog")',
    `${name}: the overlapping destination was accepted`);
    assert.equal(await evaluate(el('export-start') + '.disabled'), true);
    await evaluate('cacheCheck.target = cacheCheck.destination; true');
    await click('export-pick');
    await until('!' + el('export-start') + '.disabled', `${name}: the fixture destination was refused`);
    assert.equal(await evaluate(el('export-refusal') + '.hidden'), true);

    if (name === 'legacy-sizes') {
      // More containers than the writer schedules at once, so a stop lands in
      // the middle of a batch and the continuation keeps the same selection.
      await call(pauseCloses);
      await click('export-start');
      await until('exportWrites.active === 8', 'The fixture writes did not reach the writer limit');
      await click('export-stop');
      await evaluate('exportWrites.resume()');
      await until(el('export-start') + '.textContent === "Continue saving" && !' +
        el('export-start') + '.disabled', 'Stop did not leave a resumable batch');
      assert.equal(Object.keys(await call(savedFiles)).length, 8);
      measured.stops++;
    }
    await click('export-start');
    await until(el('receipt-report-list') + '.children.length === 3',
      `${name}: the recovery reports were not saved`);
    assert.match(await text('receipt-headline'), /^\d+ files? saved to /);
    assert.equal(await text('receipt-reconciled'), '');
    assert.deepEqual(
      await call(savedFiles),
      Object.fromEntries(entry.manifest.map(({ output, bytes, sha256 }) => [output, { bytes, sha256 }])),
      `${name}: the saved JPEGs are not the native scan's own bytes and names`,
    );

    const reportCsv = await call(report, 'report.csv');
    const rows = reportCsv.split('\r\n').filter(Boolean);
    assert.equal(rows[0], 'mode,filename,status,width,height,container');
    assert.equal(rows.length - 1, counts.containers, `${name}: one report row per container`);
    assert(rows.slice(1).every((row) => row.startsWith('legacy,')), `${name}: the rows name the mode`);
    const extractCsv = await call(report, 'extract.csv');
    assert(extractCsv.startsWith('mode,status,bytes,sha256,output,note\r\nlegacy,'));

    const manifest = JSON.parse(await call(report, 'manifest.json'));
    assert.equal(manifest.schema, 'lrsalvage.manifest');
    assert.equal(manifest.mode, 'legacy');
    assert.deepEqual(manifest.scan, counts);
    assert.equal(manifest.layout, undefined, 'an indexless run publishes no layout');
    assert.deepEqual(manifest.batch, {
      saved: counts.usable, already_recovered: 0, failed: 0,
    });

    await click('receipt-verification');
    await until(el('receipt-report-status') + '.textContent.includes("Saved")',
      `${name}: the verification report was not saved`);
    const verification = JSON.parse(await call(report, 'verification.json'));
    assert.equal(verification.schema, 'lrsalvage.verification');
    assert.equal(verification.mode, 'legacy');
    assert.deepEqual(verification.scan, counts);
    assert.deepEqual(verification.writes, {
      saved: counts.usable, already_recovered: 0, failed: 0, confirmed_on_disk: counts.usable,
    });
    assert(!JSON.stringify(verification).includes('legacy-preview-'),
      `${name}: the shared report names a generated filename`);

    // The copied diagnostics: the same frozen object, saying which mode it came
    // from, with the mode's own four counts and no path or filename at all.
    await click('receipt-diagnostics');
    await until(el('receipt-diagnostics-status') + '.textContent.length > 0',
      'Copying diagnostics said nothing');
    const diagnostics = JSON.parse(await evaluate('navigator.clipboard.readText()'));
    assert.equal(diagnostics.mode, 'legacy');
    assert.equal(diagnostics.scan, null);
    assert.deepEqual(Object.keys(diagnostics.cache_scan).sort(),
      ['containers', 'invalid', 'unsupported', 'usable']);
    assert.equal(diagnostics.cache_scan.containers, counts.containers);
    assert.equal(diagnostics.cache_scan.usable, counts.usable);
    assert(!JSON.stringify(diagnostics).includes('legacy-preview-'),
      'the copy names no generated filename');
    assert(!JSON.stringify(diagnostics).includes('loose/'),
      'the copy names no container path');
    assert(!JSON.stringify(diagnostics).includes('previews.db'),
      'the copy names no file in the cache');

    measured.cases++;
    measured.previewsSaved += counts.usable;
    await click('restart');
  }

  // An occupied path: matching bytes are already recovered, different bytes are a
  // conflict that leaves the file alone.
  const repeat = await call(mount, 'legacy-tie', false);
  await call(async (path) => {
    const { writeFile } = await import('./mount.js');
    const file = await (await cacheCheck.destination.getFileHandle(path)).getFile();
    const bytes = new Uint8Array(await file.arrayBuffer());
    bytes[bytes.length - 3] ^= 1;
    await writeFile(cacheCheck.destination, path, bytes);
  }, repeat.manifest[0].output);
  const occupied = await call(savedFiles);
  await click('recover-legacy');
  await until('!' + el('result') + '.hidden', 'The second indexless scan did not reach review');
  await click('export-pick');
  await until('!' + el('export-start') + '.disabled', 'The same folder was refused as a destination');
  await click('export-start');
  await until(el('export-conflict') + '.open', 'Existing legacy previews were not listed');
  await evaluate('document.querySelector("#export-conflict [value=skip]").click()');
  await until(el('receipt-report-list') + '.children.length === 3', 'The second run reports were not saved');
  assert.match(await text('receipt-headline'), /^1 of 2 files recovered/);
  assert.match(await text('receipt-detail'), /1 skipped/);
  assert.deepEqual(await call(savedFiles), occupied, 'a conflict must not overwrite the file');
  assert.deepEqual(JSON.parse(await call(report, 'manifest (2).json')).batch,
    { saved: 0, already_recovered: 1, failed: 0, omitted: 1 });
  measured.alreadyRecovered += 1;
  measured.conflicts += 1;
  await click('restart');

  // A folder with no container is its own answer, and it offers no save.
  const empty = await call(mount, 'legacy-empty', true);
  assert.equal(empty.report.scan.containers, 0);
  await click('recover-legacy');
  await until('!' + el('result') + '.hidden', 'The empty folder did not report a result');
  assert.equal(await text('headline'), 'This folder holds no preview containers.');
  assert.equal(await evaluate(el('export-section') + '.hidden'), true);
  assert.match(await text('causes'), /No \.lrprev file was found/);
  await click('restart');

  // The indexed mode's missing index explains the available option, and taking
  // it reuses the folder that was already chosen.
  await call(mount, 'legacy-tie', true);
  await click('recover-cache');
  await until('!' + el('result') + '.hidden', 'The indexless folder did not report a result');
  assert.match(await text('headline'), /no previews\.db/);
  assert.equal(await evaluate(el('recover-legacy-after-error') + '.closest("[hidden]") === null'), true,
    'a missing index did not offer the indexless mode');
  await click('recover-legacy-after-error');
  await until(el('headline') + '.textContent.includes("preview containers")',
    'The indexless mode did not reuse the chosen folder');
  assert.match(await text('headline'), /2 of 2 preview containers/);
  measured.fallback++;
  await click('restart');

  // A catalog run can report an index finding or stop when the index cannot be copied.
  for (const [name, refusedCopy] of [
    ['no-index', false],
    ['unreadable-index', false],
    ['unreadable-index', true],
  ]) {
    await call(async (name) => {
      const { json, mountVault } = await import('./mount.js');
      const fixtures = await json('/native.json');
      const entry = fixtures.vaults.find((item) => item.name === name);
      const legacy = fixtures.legacy.find((item) => item.name === 'legacy-tie');
      const root = await navigator.storage.getDirectory();
      const folder = await mountVault(root, name, '/' + name, entry.files);
      const catalog = await folder.getDirectoryHandle(entry.catalog.split('/')[0]);
      const cacheFiles = entry.files.filter((file) => file.startsWith(entry.cache + '/'))
        .map((file) => file.slice(entry.cache.length + 1));
      const cache = await mountVault(catalog, entry.cache,
        '/' + name + '/' + encodeURIComponent(entry.cache), cacheFiles);
      await mountVault(cache, 'legacy-containers', '/legacy-tie', legacy.files);
      globalThis.showDirectoryPicker = async () => catalog;
      globalThis.catalogSource = catalog;
    }, name);
    await click('pick');
    await until('!' + el('scan') + '.disabled', 'The catalog was not selected');
    if (refusedCopy) {
      await call(() => {
        const getFile = FileSystemFileHandle.prototype.getFile;
        globalThis.restoreIndexRead = () => { FileSystemFileHandle.prototype.getFile = getFile; };
        FileSystemFileHandle.prototype.getFile = function () {
          if (this.name === 'previews.db') throw new DOMException('Test index refused', 'NotAllowedError');
          return getFile.call(this);
        };
      });
    }
    await click('scan');
    await until('!' + el('result') + '.hidden', 'The catalog did not reach its results');
    if (refusedCopy) {
      assert.match(await text('result-title'), /scan could not finish/i);
      assert.match(await text('headline'), /index could not be copied/i);
    } else {
      assert.match(await text('result-title'), /Review previews/);
      assert.match(await text('checked'), /Previews index/);
    }
    assert.equal(await evaluate(el('recover-legacy-after-error') + '.closest("[hidden]") === null'),
      true, name + ': the catalog finding did not offer legacy recovery');
    assert.equal(await evaluate('document.activeElement.id'), 'result-title');
    await call(() => {
      globalThis.showDirectoryPicker = async ({ mode }) => {
        if (mode !== 'readwrite') throw new Error('Unexpected second source pick');
        return catalogSource;
      };
    });
    await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    assert.equal(await evaluate('document.activeElement.id'), 'recover-legacy-after-error');
    await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key: ' ', code: 'Space', windowsVirtualKeyCode: 32 });
    await until(el('headline') + '.textContent.includes("2 of 2 preview containers")',
      'Legacy recovery did not reuse the nested cache');
    await click('export-pick');
    await until(el('export-refusal') + '.hidden === false', 'The catalog folder was accepted as a destination');
    assert.equal(await evaluate(el('export-start') + '.disabled'), true);
    measured.fallback++;
    if (refusedCopy) await evaluate('restoreIndexRead()');
    await click('restart');
  }

  // A folder the walk cannot read is an inventory error, never a partial list.
  await call(mount, 'legacy-outside', true);
  await evaluate(`(() => {
    const native = FileSystemDirectoryHandle.prototype.entries;
    globalThis.restoreLegacyEntries = () => { FileSystemDirectoryHandle.prototype.entries = native; };
    FileSystemDirectoryHandle.prototype.entries = function () {
      if (this.name === 'A000') throw new DOMException('Test shard refused', 'NotAllowedError');
      return native.call(this);
    };
  })()`);
  try {
    await click('recover-legacy');
    await until('!' + el('result') + '.hidden', 'The unreadable shard did not report a result');
    assert.match(await text('headline'), /could not read the entire preview cache/);
    assert.equal(await evaluate(el('export-section') + '.hidden'), true);
    await click('restart');
  } finally {
    await evaluate('restoreLegacyEntries()');
  }

  console.log('legacy: sizes, equal-area ties, a usable prefix, damaged and unsupported containers, '
    + 'an empty folder, indexed and catalog handoffs, stop/continue, occupied files and byte parity agree');
  return measured;
}

// Drive the packaged guide and its sample through the actual installation path.
// Only the destination picker answer is supplied by the test.
export async function checkLegacyPractice(send) {
  const { evaluate, call, el, click, text, until } = controls(send);
  const sample = 'document.querySelector("#samples a[href$=\\"?sample=legacy\\"]")';
  await until(sample + ' !== null', 'The packaged guide does not link to the legacy sample');

  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: '(' + (() => {
      globalThis.practiceSourcePicks = 0;
      globalThis.showDirectoryPicker = async ({ mode }) => {
        if (mode !== 'readwrite') {
          practiceSourcePicks++;
          throw new Error('The practice cache should already be available');
        }
        const root = await navigator.storage.getDirectory();
        const destination = await root.getDirectoryHandle('practice-output', { create: true });
        globalThis.cacheCheck = { destination };
        return destination;
      };
    }).toString() + ')()',
  });

  const expected = {};
  for (const [path, asset] of [
    ['A/AAAA/practice.lrprev', 'canyon.jpg'],
    ['relocated/snow.lrprev', 'snow-small.jpg'],
  ]) {
    const name = 'legacy-preview-' + createHash('sha256').update(path).digest('hex') + '.jpg';
    const jpeg = await readFile(new URL('../crates/lrsalvage-app/practice/photos/' + asset, import.meta.url));
    expected[name] = { bytes: jpeg.length, sha256: createHash('sha256').update(jpeg).digest('hex') };
  }

  await evaluate(sample + '.click()');
  for (const repeat of [false, true]) {
    if (repeat) await send('Page.reload');
    await until('!' + el('result') + '?.hidden && ' +
      el('headline') + '?.textContent.includes("no previews.db")', 'Expected the practice index error');
    assert.equal(await evaluate(el('export-section') + '.hidden'), true);
    assert.equal(await evaluate(el('recover-legacy-after-error') + '.closest("[hidden]") === null'), true);
    await click('recover-legacy-after-error');
    await until(el('headline') + '.textContent.includes("2 of 4 preview containers")',
      'The handoff did not reach practice review');
    assert.equal(await evaluate('practiceSourcePicks'), 0);
    assert.match(await text('checked'), /Preview containers4Usable2Invalid1Unsupported1/);
    await until('document.querySelectorAll(".review-tile input:checked").length === 2',
      'The sample did not select both usable previews');
    const tiles = await evaluate('[...document.querySelectorAll(".review-tile")].map((tile) => tile.textContent)');
    for (const dims of ['960 × 640 px', '480 × 320 px']) {
      assert(tiles.some((tile) => tile.includes(dims)), 'Missing selected dimensions: ' + dims);
    }

    await click('export-pick');
    await until('!' + el('export-start') + '.disabled', 'The sample destination was refused');
    await click('export-start');
    if (repeat) {
      await until(el('export-conflict') + '.open', 'Repeat recovery did not list the existing JPEGs');
      await evaluate('document.querySelector("#export-conflict [value=skip]").click()');
    }
    await until(el('receipt-report-list') + '.children.length === 3', 'The sample receipt did not finish');
    assert.deepEqual(await call(savedFiles), expected);
    const counts = { saved: repeat ? 0 : 2, already_recovered: repeat ? 2 : 0, failed: 0 };
    const manifest = JSON.parse(await call(report, repeat ? 'manifest (2).json' : 'manifest.json'));
    assert.equal(manifest.mode, 'legacy');
    assert.deepEqual(manifest.batch, counts);
    await click('receipt-verification');
    await until(el('receipt-report-status') + '.textContent.includes("Saved")',
      'The practice verification report was not saved');
    const verification = JSON.parse(await call(report, repeat ? 'verification (2).json' : 'verification.json'));
    assert.deepEqual(verification.writes, { ...counts, confirmed_on_disk: 2 });
  }
  return { usable: 2, invalid: 1, unsupported: 1, alreadyRecovered: 2 };
}
