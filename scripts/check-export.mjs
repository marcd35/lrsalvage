import assert from 'node:assert/strict';
import { setTimeout as wait } from 'node:timers/promises';

export async function checkExport(send) {
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
  await until(el('#source') + '?.hidden === false', 'The source screen did not load');
  await evaluate(`(async () => {
    const { freshDirectory, json, mountVault } = await import('./mount.js');
    const root = await navigator.storage.getDirectory();
    globalThis.prepareExport = async (name) => {
      const entry = (await json('/native.json')).vaults.find(vault => vault.name === name);
      const fixture = await mountVault(root, 'export-source', '/' + name, entry.files);
      const source = await fixture.getDirectoryHandle(entry.catalog.split('/')[0]);
      const destination = await freshDirectory(root, 'export-output');
      const cache = await fixture.getDirectoryHandle(entry.cache);
      globalThis.exportPicker = { source, destination, cache, fixture, root };
    };
    await prepareExport('working');
    // The picker is the one line a test supplies the answer to, so it is also the
    // only place startIn can be observed (#191).
    globalThis.pickerCalls = [];
    globalThis.showDirectoryPicker = async (options) => {
      const { id, mode } = options;
      pickerCalls.push({ id, mode, startIn: options.startIn?.name ?? null });
      return mode === 'readwrite' ? exportPicker.destination :
        id === 'lrsalvage-cache' ? exportPicker.cache : exportPicker.source;
    };
    globalThis.outputFiles = async (dir = exportPicker.destination) => {
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
  async function scan() {
    await click('#pick');
    await until(el('#scan') + '?.disabled === false', 'The catalog did not become ready');
    await click('#pick-cache');
    await until(el('#picked') + '.textContent.includes("(itself)")', 'The split cache was not attached');
    await click('#scan');
    await until(el('#photo-review') + '?.hidden === false', 'The review screen did not open');
    assert.equal(await evaluate('Boolean(' + el('#export-section') + ')'), true, 'The product has no export controls');
  }

  await scan();
  await click('#export-pick');
  await until(el('#export-start') + '?.disabled === false', 'The export destination was not accepted');
  // `#191`: the destination picker starts at the catalog's own folder, which is
  // the handle the page already holds.
  const startedAt = await evaluate(
    'pickerCalls.filter(call => call.mode === \'readwrite\').at(-1)?.startIn ?? null',
  );
  assert.equal(startedAt, await evaluate('exportPicker.source.name'),
    'The destination picker did not start at the catalog folder');
  await click('#export-start');
  await until(el('#export-status') + '.textContent.includes("3 saved") && ' + el('#export-start') + '.disabled === false', 'The product did not commit three JPEGs');
  // `#192`: the batch's readout — percentage from completed writes, a bar at the
  // same figure, and one live region that announced the milestones.
  assert.match(await evaluate(el('#export-progress') + '.textContent'),
    /^100% · 3 of 3 written · /, 'The export readout did not finish at 100%');
  assert.equal(await evaluate('document.getElementById("export-bar").max'), 3);
  assert.equal(await evaluate('document.getElementById("export-bar").value'), 3);
  assert.match(await evaluate(el('#export-announce') + '.textContent'),
    /100% — 3 of 3 written · finished/, 'The milestone was not announced');
  await click('#receipt-complete button[type="submit"]');
  const files = await evaluate('outputFiles()');
  const expected = await evaluate(`(async () => {
    const csv = await (await fetch('/cli-extract/extract.csv')).text();
    const manifest = await (await fetch('/cli-extract/manifest.json')).json();
    const jpegs = {};
    for (const { output, selected } of manifest.items) {
      if (!selected) continue;
      const path = output.split('/cli-extract/')[1];
      const response = await fetch('/cli-extract/' + path.split('/').map(encodeURIComponent).join('/'));
      if (!response.ok) throw new Error('Missing CLI JPEG: ' + path);
      jpegs[path] = Array.from(new Uint8Array(await response.arrayBuffer()));
    }
    return { csv, manifest, jpegs };
  })()`);
  const decoder = new TextDecoder();
  const csv = decoder.decode(Uint8Array.from(files['extract.csv']));
  const manifest = JSON.parse(decoder.decode(Uint8Array.from(files['manifest.json'])));
  const jpegs = Object.fromEntries(Object.entries(files).filter(([path]) => path.endsWith('.jpg')));
  assert(Object.keys(jpegs).some((path) => path.endsWith('/IMG_0001 (2).jpg')),
    'a below-floor candidate still reserves its ordinary name');
  assert.deepEqual(jpegs, expected.jpegs, 'app and CLI relative paths and JPEG bytes');
  const normalise = (value) => value.replaceAll(/(?:[A-Za-z]:)?[^"\r\n,]*\/cli-extract\//g, '/destination/')
    .replaceAll(/(?:[A-Za-z]:)?[^"\r\n,]*\/working\/(?:[^/"\r\n,]+\/)?[^/"\r\n,]+\.lrdata\//g, '/cache/');
  assert.equal(csv, normalise(expected.csv), 'app and CLI extraction outcomes');
  delete manifest.generated_unix;
  delete expected.manifest.generated_unix;
  assert.deepEqual(manifest, JSON.parse(normalise(JSON.stringify(expected.manifest))), 'app and CLI manifests');
  await evaluate('(' + (async (names) => {
    const { mountVault } = await import('./mount.js');
    exportPicker.firstDestination = exportPicker.destination;
    exportPicker.destination = await mountVault(exportPicker.root, 'cli-resume', '/cli-extract', names);
  }).toString() + ')(' + JSON.stringify([...Object.keys(expected.jpegs), 'extract.csv', 'manifest.json']) + ')');
  await click('#export-pick');
  await until(el('#export-destination') + '.textContent.endsWith("cli-resume")', 'The CLI output folder was not selected');
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'Existing CLI outputs were not listed');
  assert.equal(await evaluate('document.activeElement.value'), 'skip');
  await click('#export-conflict [value="skip"]');
  await until(el('#export-status') + '.textContent.includes("3 already recovered") && ' +
    el('#export-start') + '.disabled === false', 'The browser did not recognize verified CLI outputs');
  const resumed = await evaluate('outputFiles()');
  assert.match(await evaluate(el('#export-status') + '.textContent'), /^0 saved/);
  assert(resumed['extract (2).csv'] && resumed['manifest (2).json'], 'a later batch must get its own reports');
  for (const [name, bytes] of Object.entries(expected.jpegs)) assert.deepEqual(resumed[name], bytes);
  assert.equal(decoder.decode(Uint8Array.from(resumed['extract.csv'])), expected.csv);
  assert.match(decoder.decode(Uint8Array.from(resumed['extract (2).csv'])), /skipped,/);

  // A same-size replacement can retain the JPEG marker and dimensions.
  // The in-run fingerprint must still catch it without overwriting it.
  const replaced = Object.keys(jpegs)[0];
  const changedBytes = [...jpegs[replaced]];
  changedBytes[changedBytes.length - 3] ^= 1;
  await evaluate('(' + (async (path, bytes) => {
    const { writeFile } = await import('./mount.js');
    exportPicker.destination = exportPicker.firstDestination;
    await writeFile(exportPicker.destination, path, Uint8Array.from(bytes));
  }).toString() + ')(' + JSON.stringify(replaced) + ',' + JSON.stringify(changedBytes) + ')');
  await click('#export-pick');
  await until(el('#export-destination') + '.textContent.endsWith("export-output")', 'The first folder was not restored');
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'The changed file did not prompt');
  await click('#export-conflict [value="skip"]');
  await until(el('#export-status') + '.textContent.includes("1 skipped") && ' +
    el('#export-start') + '.disabled === false', 'The replaced file was not reported as a conflict');
  assert.match(await evaluate(el('#export-status') + '.textContent'), /^0 saved.*2 already recovered/);
  assert.equal(await evaluate(el('#receipt-complete') + '.open'), false);
  assert.match(await evaluate(el('#receipt-reconciled') + '.textContent'), /Changed in the destination/);
  assert.deepEqual((await evaluate('outputFiles()'))[replaced], changedBytes);

  await click('#restart');
  await evaluate("prepareExport('export')");
  await scan();
  const tally = () => evaluate(el('#photo-count') + '.textContent');
  const errors = () => evaluate(el('#export-errors') + '.textContent');
  const status = el('#export-status');
  const ready = el('#export-start') + '.disabled === false';
  // The list is only rendered while its dialog is open (`#192`), so reading the
  // plan means opening it — the same two presses a person makes.
  const listPaths = () => evaluate(
    "Array.from(document.querySelectorAll('#export-path-list li'), li => li.textContent)",
  );
  const pathsOpen = () => el('#export-paths-dialog') + '.open === true';
  const paths = async () => {
    await click('#export-paths-open');
    await until(pathsOpen(), 'The planned-path dialog did not open');
    const rows = await listPaths();
    await click('#export-paths-close');
    return rows;
  };
  const toggleFlat = (flat) => evaluate(
    el('#export-layout') + ".value = '" + (flat ? 'flat' : 'mirrored') + "'; " +
    el('#export-layout') + ".dispatchEvent(new Event('change'))",
  );
  const unchanged = await tally();
  assert.match(unchanged, /12 checked/);
  // `#192`: the save controls above the gallery carry the same state as the panel.
  assert.equal(await evaluate(el('#export-save-bar-count') + '.textContent'), '12 selected');
  assert.equal(await evaluate(el('#export-save-bar-destination') + '.textContent'),
    'No destination chosen yet');
  assert.equal(await evaluate(el('#export-save-bar-save') + '.textContent'),
    await evaluate(el('#export-start') + '.textContent'));
  assert.equal(await evaluate(el('#export-save-bar-save') + '.disabled'),
    await evaluate(el('#export-start') + '.disabled'));
  await click('#export-paths-open');
  await until(pathsOpen(), 'The planned-path dialog did not open');
  assert.equal(await evaluate(el('#export-paths-note') + '.textContent'),
    '12 planned paths, original folders mirrored, into no destination yet, ' +
      '3 of them numbered to avoid a clash');
  assert.match(await evaluate(el('#export-paths-shown') + '.textContent'), /^12 matching paths\.$/);
  assert.equal(await evaluate('document.activeElement.id'), 'export-paths-search');
  assert((await listPaths()).includes('Synthetic export/2023/img (3).jpg'));
  assert((await listPaths()).includes('Synthetic export/2024/IMG.jpg'));
  assert((await listPaths()).includes('Synthetic export/2024/A (2).jpg'));
  assert((await listPaths()).includes('Synthetic export/2024/A.jpg/H.jpg'));
  // Searching filters the plan, not the DOM that happens to be there.
  await evaluate(el('#export-paths-search') + ".value = 'img (3)'; " +
    el('#export-paths-search') + ".dispatchEvent(new Event('input'))");
  await until(el('#export-paths-shown') + '.textContent.startsWith("1 matching path")',
    'The path search did not filter');
  assert.deepEqual(await listPaths(), ['Synthetic export/2023/img (3).jpg']);
  await click('#export-paths-close');
  await until('document.getElementById("export-paths-dialog").open === false',
    'The planned-path dialog did not close');
  await toggleFlat(true);
  await until(el('#export-total') + '.textContent.startsWith("12 JPEGs")', 'Flat paths did not load');
  await click('#export-paths-open');
  await until(pathsOpen(), 'The planned-path dialog did not reopen');
  assert.deepEqual((await listPaths()).slice(0, 4), ['IMG.jpg', 'IMG (2).jpg', 'img (3).jpg', 'IMG (4).jpg']);
  await click('#export-paths-close');

  const inputsBefore = await evaluate('outputFiles(exportPicker.fixture)');
  const originalDestination = await evaluate("exportPicker.safe = exportPicker.destination; exportPicker.safe.name");
  // `#191`: the refusal is its own block with the way out inside it, and focus
  // lands on that button rather than on a status line below the fold.
  const refused = el('#export-refusal') + '.hidden === false';
  const refusalText = el('#export-refusal-message') + '.textContent';
  const refusalSays = (fragment) => refused + ' && ' + refusalText + '.includes(' + JSON.stringify(fragment) + ')';
  const focused = (id) => 'document.activeElement && document.activeElement.id === ' + JSON.stringify(id);
  for (const target of ['source', 'cache', 'fixture']) {
    await evaluate('exportPicker.destination = exportPicker.' + target);
    await click('#export-pick');
    await until(refusalSays('outside the catalog'), 'An input tree was accepted as a destination');
    assert.match(
      await evaluate(refusalText),
      /inside (the catalog's folder|the preview cache folder|a folder the catalog records)/,
      'The refusal does not say which tree the destination landed in',
    );
    assert.equal(await evaluate(focused('export-refusal-pick')), true,
      'The refusal did not take focus to its own way out');
    assert.equal(await evaluate(ready), false);
  }
  await evaluate("(async () => { exportPicker.destination = await exportPicker.cache.getDirectoryHandle('A'); })()");
  await click('#export-pick');
  await until(refusalSays('the preview cache folder'), 'A cache descendant was accepted');
  // The block's own button is the way out, and it is one press from the refusal.
  await evaluate('exportPicker.destination = exportPicker.safe; true');
  await click('#export-refusal-pick');
  await until(el('#export-refusal') + '.hidden === true', 'The refusal survived a folder that was accepted');
  await until(focused('export-pick'), 'Focus did not return to the destination control');
  // The bar above the gallery learned the same destination the panel did.
  assert.match(await evaluate(el('#export-save-bar-destination') + '.textContent'),
    /^Destination · /, 'The save bar did not take the accepted destination');
  await click('#photo-resolution summary');
  await click('#photo-resolution-all');
  await click('#photo-resolution summary');
  await click('button[aria-label="Preview Small.CR2"]');
  await until(el('#export-one') + '.disabled === false', 'One-off save was not enabled');
  assert.equal(await evaluate(el('#export-one-path') + '.textContent'), 'Small_small.jpg');
  await click('#export-one');
  await until(el('#export-one-status') + '.textContent.startsWith("Saved")', 'The one-off JPEG did not commit');
  assert.equal((await tally()).split(' · ')[1], unchanged.split(' · ')[1],
    'Saving an unchecked photo must leave selection alone');
  assert.match(await evaluate(el('#export-destination') + '.textContent'), new RegExp(originalDestination));
  assert.deepEqual(Object.keys(await evaluate('outputFiles()')), ['Small_small.jpg']);
  await click('#photo-dialog-check');
  await click('#photo-close');
  assert((await paths()).includes('Small_small.jpg'));
  assert.match(await evaluate(el('#export-total') + '.textContent'),
    /^13 JPEGs . (\d+ B|\d+\.\d\d (kB|MB|GB)) of previews$/);

  // Hold Chromium's real write streams just before close so we can stop at
  // an observable in-flight boundary and verify that only committed files count.
  await evaluate("(" + pauseCloses.toString() + ")()");
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'The one-off file did not prompt');
  await click('#export-conflict [value="skip"]');
  await until('exportWrites.active === 8', 'The bounded writer did not fill eight slots');
  await click('#export-stop');
  assert.match(await evaluate(status + '.textContent'), /^0 saved.*1 already recovered/);
  await evaluate('exportWrites.resume(); true');
  await until(status + '.textContent.includes("Stopped") && ' + ready, 'The stopped batch did not settle');
  const stopped = await evaluate('outputFiles()');
  assert.equal(Object.keys(stopped).length, 9);
  await click('#export-start');
  await until(status + '.textContent.startsWith("12 saved") && ' + ready, 'Continuation did not finish');
  const finished = await evaluate('outputFiles()');
  assert.equal(Object.keys(finished).filter((name) => name.endsWith('.jpg')).length, 13);
  for (const [name, bytes] of Object.entries(stopped)) assert.deepEqual(finished[name], bytes);
  assert.deepEqual(await evaluate('exportWrites.names.sort()'), [
    'A.jpg', 'B.jpg', 'C.jpg', 'D.jpg', 'E.jpg', 'F.jpg', 'G.jpg', 'H.jpg',
    'IMG (2).jpg', 'IMG (4).jpg', 'IMG.jpg', 'img (3).jpg',
  ].sort(), 'each remaining JPEG must open just one writer across stop and continue');

  await evaluate("(async () => { exportPicker.destination = await exportPicker.root.getDirectoryHandle('lost', {create: true}); })()");
  await click('#export-pick');
  await until(el('#export-destination') + '.textContent.endsWith("lost") && ' + ready, 'The new folder was not selected');
  await evaluate("exportPicker.root.removeEntry('lost', {recursive: true})");
  await click('#export-start');
  await until(status + '.textContent.includes("13 failed") && ' + ready, 'The lost folder did not produce per-file errors');
  assert.match(await errors(), /IMG\.jpg/);
  assert.match(await errors(), /Small_small\.jpg/);
  await evaluate('(' + (async () => {
    const { mountVault } = await import('./mount.js');
    exportPicker.destination = await mountVault(exportPicker.root, 'occupied', '/export/out', ['IMG.jpg']);
  }).toString() + ')()');
  await click('#export-pick');
  await until(el('#export-destination') + '.textContent.endsWith("occupied") && ' + ready, 'The occupied folder was not selected');
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'An occupied JPEG was not shown before saving');
  await click('#export-conflict [value="cancel"]');
  await until(ready, 'Cancel did not return to review');
  assert.deepEqual(Object.keys(await evaluate('outputFiles()')), ['IMG.jpg']);
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'The occupied path was not shown again');
  await click('#export-conflict [value="keep"]');
  await until(status + '.textContent.includes("13 saved of 13") && ' + ready, 'Keep both did not complete');
  const kept = await evaluate('outputFiles()');
  assert.deepEqual(kept['IMG.jpg'], inputsBefore['out/IMG.jpg']);
  assert.match(await evaluate(el('#receipt-complete-title') + '.textContent'), /13 of 13 files recovered in /);
  assert.match(await evaluate(el('#receipt-complete-breakdown') + '.textContent'), /1 kept both/);
  await click('#receipt-complete button[type="submit"]');
  const renamed = Object.keys(kept).find(path => !['IMG (2).jpg', 'IMG (4).jpg', 'IMG.jpg', 'img (3).jpg'].includes(path) && /^IMG \(\d+\)\.jpg$/i.test(path));
  assert(renamed, 'The extra JPEG must be numbered around reserved plan paths');
  assert(decoder.decode(Uint8Array.from(kept['extract.csv'])).includes('/' + renamed));
  const keptManifest = JSON.parse(decoder.decode(Uint8Array.from(kept['manifest.json'])));
  assert(keptManifest.items.some(item => item.selected && item.output.endsWith('/' + renamed)));

  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'Overwrite was not offered for the current batch');
  await click('#export-conflict [value="overwrite"]');
  await until(status + '.textContent.includes("13 saved of 13") && ' + ready, 'Overwrite did not complete');
  const overwritten = await evaluate('outputFiles()');
  assert.notDeepEqual(overwritten['IMG.jpg'], inputsBefore['out/IMG.jpg']);
  assert.deepEqual(overwritten[renamed], kept[renamed], 'Overwrite leaves the separate numbered copy alone');
  await click('#receipt-complete button[type="submit"]');
  await evaluate('(' + (async () => {
    const { freshDirectory } = await import('./mount.js');
    exportPicker.destination = await freshDirectory(exportPicker.root, 'late-conflict');
    const native = FileSystemDirectoryHandle.prototype.getFileHandle;
    let lookups = 0;
    FileSystemDirectoryHandle.prototype.getFileHandle = async function (name, options) {
      if (this.name === 'late-conflict' && name === 'Small_small.jpg' && ++lookups === 2) {
        const foreign = await native.call(this, name, { create: true });
        const stream = await foreign.createWritable();
        await stream.write(Uint8Array.of(1, 2, 3));
        await stream.close();
      }
      return native.call(this, name, options);
    };
  }).toString() + ')()');
  await click('#export-pick');
  await until(el('#export-destination') + '.textContent.endsWith("late-conflict")', 'The race folder was not selected');
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'A file that arrived during the save did not prompt');
  await click('#export-conflict [value="cancel"]');
  await until(ready + ' && ' + el('#export-start') + '.textContent === "Continue saving"', 'Cancel did not pause the batch');
  const pausedFiles = await evaluate('outputFiles()');
  assert(Object.keys(pausedFiles).length > 1, 'The late conflict should preserve earlier saves');
  assert.deepEqual(pausedFiles['Small_small.jpg'], [1, 2, 3]);
  await click('#export-start');
  await until(el('#export-conflict') + '.open', 'Continue did not reopen the conflict');
  await click('#export-conflict [value="skip"]');
  await until(ready + ' && ' + el('#export-status') + '.textContent.includes("1 skipped")', 'Continue did not settle');
  assert.match(await evaluate(el('#receipt-headline') + '.textContent'), /^12 of 13 files recovered in /);
  assert.equal(await evaluate(el('#receipt-complete') + '.open'), false);
  assert.deepEqual((await evaluate('outputFiles()'))['Small_small.jpg'], [1, 2, 3]);
  assert.deepEqual(await evaluate('outputFiles(exportPicker.fixture)'), inputsBefore,
    'no catalog or preview cache file may change');
  return { jpegs: Object.keys(jpegs).length, reports: 2, resumedCli: 3, stoppedAt: 8, continuedTo: 13, lostDestination: 13 };
}

export function pauseCloses() {
  const native = FileSystemFileHandle.prototype.createWritable;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const writes = { active: 0, names: [], resume: release };
  globalThis.exportWrites = writes;
  FileSystemFileHandle.prototype.createWritable = async function (...args) {
    const stream = await native.apply(this, args);
    if (this.name.endsWith('.jpg')) {
      writes.names.push(this.name);
      const close = stream.close.bind(stream);
      stream.close = async () => {
        writes.active++;
        try {
          await gate;
          await close();
        } finally {
          writes.active--;
        }
      };
    }
    return stream;
  };
}
