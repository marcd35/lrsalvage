// The app's harness: every vault through the page's own entry, held to native.
//
// What this drives, in order — and each step is a thing #149 promised:
//
// 1. Every vault through `start`, comparing the report with the native one: the
//    counts, the cache facts, the index's condition, the recorded roots, and the
//    two digests (`--verify-source`'s promise, in a browser).
// 2. A stop, pressed at the first catalog image: the scan ends at that boundary
//    and reports `stopped`, which is what returns a person to source selection.
// 3. The recheck: the recorded originals folder mounted where the catalog spells
//    it, and the same scan run again — `original-present` must move by exactly
//    the number the native run reports.
// 4. A cancelled picker, for real: headless Chromium answers `AbortError` with no
//    dialog, so the branch the product page shows is exercised, not imitated.
// 5. The capability gate, with the API absent.
//
// The result is `window.__result` and `window.__done`, which the CDP driver reads.

import { capability, folderSource, pickFolder, sourceFrom, start, withMount } from './app.js';
import { json, mountVault } from './mount.js';

const logNode = document.getElementById('log');
function log(...parts) {
  const line = parts
    .map((part) => (typeof part === 'string' ? part : JSON.stringify(part)))
    .join(' ');
  logNode.textContent += line + '\n';
  console.log(...parts);
}

window.__result = { ok: false, differences: ['the harness did not finish'], numbers: null };
window.__done = false;

main().catch((error) => {
  const detail = error && error.stack ? error.stack : String(error);
  window.__result = { ok: false, error: detail, differences: [] };
  window.__done = true;
  log('failed:', detail);
});

async function main() {
  if (!crossOriginIsolated) {
    // The bridge parks the worker on a `SharedArrayBuffer`, and a page that is
    // not cross-origin isolated cannot make one.
    throw new Error(
      'not cross-origin isolated: the page needs COOP same-origin and COEP require-corp',
    );
  }

  const native = await json('/native.json');
  const base = await navigator.storage.getDirectory();
  const differences = [];
  const numbers = { vaults: [], timings: {} };

  // -- 1. every vault through the page's entry -----------------------------

  for (const vault of native.vaults) {
    const dir = await mountVault(base, `app-${vault.name}`, `/${vault.name}`, vault.files, log);
    const source = {
      mounts: [{ path: '/source', handle: dir }],
      catalog: `/source/${vault.catalog}`,
      cache: `/source/${vault.cache}`,
      label: vault.name,
    };

    const began = performance.now();
    const run = await once(source);
    numbers.timings[vault.name] = Math.round(performance.now() - began);
    numbers.vaults.push({ name: vault.name, report: run.report, progress: run.progress.length });

    if (vault.compare === 'failure') {
      if (run.report.ok !== false) {
        differences.push(`${vault.name}: expected a refused scan, got ${JSON.stringify(run.report)}`);
      } else if (run.report.problem.stage !== 'open') {
        differences.push(`${vault.name}: refused at ${run.report.problem.stage}, expected open`);
      } else if (!run.report.problem.message) {
        differences.push(`${vault.name}: refused without a message`);
      }
      continue;
    }

    if (vault.compare === 'after-mount') {
      // Before the folder is handed over, the two targets must disagree: the
      // desktop stat()s the drive, and a page has nothing to stat.
      const roots = run.report.roots ?? [];
      if (roots.length !== 1) {
        differences.push(`${vault.name}: expected one recorded root, got ${roots.length}`);
        continue;
      }
      if (roots[0].in_reach !== false) {
        differences.push(`${vault.name}: the drive was reachable before it was mounted`);
      }
      if (run.report.stats.original_present !== 0) {
        differences.push(
          `${vault.name}: ${run.report.stats.original_present} originals were present before mount`,
        );
      }

      if (!vault.files.some((file) => file.startsWith('originals/'))) {
        differences.push(`${vault.name}: the vault holds no originals/ to mount`);
        continue;
      }
      const originals = await mountVault(
        base,
        `app-${vault.name}-originals`,
        `/${vault.name}/originals`,
        vault.files
          .filter((file) => file.startsWith('originals/'))
          .map((file) => file.slice('originals/'.length)),
        log,
      );
      const again = await once(withMount(source, roots[0].path, originals));
      numbers.vaults.push({
        name: `${vault.name} (rechecked)`,
        report: again.report,
        progress: again.progress.length,
      });
      const expected = vault.report.stats.original_present;
      if (again.report.stats.original_present !== expected) {
        differences.push(
          `${vault.name}: ${again.report.stats.original_present} originals after mount, ` +
            `native says ${expected}`,
        );
      }
      if (again.report.roots[0].in_reach !== true) {
        differences.push(`${vault.name}: the mounted root still reads as out of reach`);
      }
      compareReports(vault.name, vault.report, again.report, differences);
      continue;
    }

    compareReports(vault.name, vault.report, run.report, differences);
  }

  // -- 2. a stop, at the first catalog image --------------------------------

  const working = native.vaults.find((vault) => vault.name === 'working');
  const workingDir = await mountVault(
    base,
    'app-stop',
    `/working`,
    working.files,
    log,
  );
  const stopSource = {
    mounts: [{ path: '/source', handle: workingDir }],
    catalog: `/source/${working.catalog}`,
    cache: `/source/${working.cache}`,
  };
  const stopped = await new Promise((resolve) => {
    const run = start(stopSource, {
      progress(message) {
        // The first report of the images stage: the stop button, pressed.
        if (message.phase === 2) run.stop();
      },
    });
    run.done.then(resolve);
  });
  numbers.stop = stopped;
  if (stopped.stopped !== true) {
    differences.push(`stop: the scan did not report a stop, got ${JSON.stringify(stopped)}`);
  }
  if ('stats' in stopped) {
    differences.push('stop: a stopped scan presented a plan');
  }

  // -- 3. a cancelled picker, for real --------------------------------------
  //
  // The driver calls `window.__gesture` with a user gesture and leaves the answer
  // in `window.__gestureResult`; nothing else can: a picker refuses with
  // `SecurityError` without a gesture, and with one, headless Chromium answers
  // `AbortError` and shows no dialog (measured, `#149`).
  const picked = await gestureAnswer();
  numbers.picker = picked;
  if (!picked.cancelled) {
    differences.push(
      `picker: a gesture was expected to cancel, got ${JSON.stringify(picked)} ` +
        '(this run needs --headless=new)',
    );
  }

  // A folder reading, against a value of the kind a pick returns. The fixture
  // backup keeps its catalog one level down — `catalog/2023.lrcat` — so the
  // folder holding it is the one a person would pick, and the vault's own root is
  // the folder that holds no `.lrcat`, which is its own right answer.
  const read = await folderSource(await workingDir.getDirectoryHandle('catalog'));
  numbers.folder = read;
  if (read.catalogs.length !== 1 || read.catalogs[0] !== '2023.lrcat') {
    differences.push(`folder: the catalog folder read as ${JSON.stringify(read.catalogs)}`);
  }
  const atRoot = await folderSource(workingDir);
  if (atRoot.catalogs.length !== 0) {
    differences.push(
      `folder: the vault root read ${JSON.stringify(atRoot.catalogs)} as catalogs, expected none`,
    );
  }

  // -- 4. the capability gate ----------------------------------------------

  const withoutPicker = capability({
    SharedArrayBuffer,
    crossOriginIsolated: true,
    isSecureContext: true,
  });
  const withoutIsolation = capability({
    showDirectoryPicker: () => {},
    SharedArrayBuffer,
    crossOriginIsolated: false,
    isSecureContext: true,
  });
  const here = capability(window);
  numbers.gates = { withoutPicker, withoutIsolation, here };
  if (withoutPicker.ok || withoutPicker.reason !== 'no-picker') {
    differences.push(`gate: a browser without showDirectoryPicker read as ${JSON.stringify(withoutPicker)}`);
  }
  if (withoutIsolation.ok || withoutIsolation.reason !== 'no-isolation') {
    differences.push(`gate: a page without isolation read as ${JSON.stringify(withoutIsolation)}`);
  }
  if (!here.ok) {
    differences.push(`gate: this page reads as unable to run (${here.reason})`);
  }

  numbers.separator = native.separator;
  window.__result = { ok: differences.length === 0, differences, numbers };
  window.__done = true;
  log(differences.length === 0 ? 'app run: ok' : 'app run: differences');
  for (const difference of differences) log('  -', difference);
}

/** The pick's answer, once the driver's gesture has produced one. */
async function gestureAnswer() {
  for (let waited = 0; waited < 20_000; waited += 50) {
    if (window.__gestureResult !== undefined) return window.__gestureResult;
    await new Promise((done) => setTimeout(done, 50));
  }
  return { timedOut: true };
}

/**
 * The driver's one gesture: this is what a person's click on "Choose folder…"
 * starts, so the picker's refusal is the page's own rather than a mock's.
 */
window.__gesture = () => pickFolder({ id: 'lrsalvage-catalog', mode: 'read' });

/** One scan, resolving with the report and the progress reports it emitted. */
function once(source) {
  const progress = [];
  const run = start(source, {
    stage(name) {
      progress.push({ stage: name });
    },
    progress(message) {
      progress.push(message);
    },
  });
  return run.done.then((report) => ({ report, progress }));
}

/**
 * One browser report against one native report.
 *
 * Everything both targets can agree on. Two things are deliberately absent:
 * `original-present` (a drive a page has not been given) and `roots[].in_reach`
 * (the same fact from the other side) — `originals` asserts those itself, after
 * the folder is handed over.
 */
function compareReports(name, native, browser, differences) {
  if (!browser.ok) {
    differences.push(`${name}: the browser refused the scan: ${JSON.stringify(browser.problem)}`);
    return;
  }
  for (const key of ['source_sha256', 'snapshot_sha256']) {
    if (browser[key] !== native[key]) {
      differences.push(`${name}: ${key} ${browser[key]} vs native ${native[key]}`);
    }
  }
  for (const [key, expected] of Object.entries(native.stats ?? {})) {
    const actual = browser.stats?.[key];
    if (actual !== expected) {
      differences.push(`${name}: stats.${key} ${actual} vs native ${expected}`);
    }
  }
  for (const [key, expected] of Object.entries(native.cache ?? {})) {
    if (key === 'shapes') {
      for (const [shape, count] of Object.entries(expected)) {
        if (browser.cache?.shapes?.[shape] !== count) {
          differences.push(
            `${name}: cache.shapes.${shape} ${browser.cache?.shapes?.[shape]} vs native ${count}`,
          );
        }
      }
      continue;
    }
    if (browser.cache?.[key] !== expected) {
      differences.push(`${name}: cache.${key} ${browser.cache?.[key]} vs native ${expected}`);
    }
  }
  const nativeIndex = native.index?.problem ?? null;
  const browserIndex = browser.index?.problem ?? null;
  if (JSON.stringify(nativeIndex) !== JSON.stringify(browserIndex)) {
    differences.push(
      `${name}: index.problem ${JSON.stringify(browserIndex)} vs native ${JSON.stringify(nativeIndex)}`,
    );
  }
  if ((native.roots ?? []).length !== (browser.roots ?? []).length) {
    differences.push(
      `${name}: ${(browser.roots ?? []).length} roots vs native ${(native.roots ?? []).length}`,
    );
  } else {
    native.roots.forEach((root, index) => {
      const other = browser.roots[index];
      if (other.path !== root.path || other.images !== root.images) {
        differences.push(
          `${name}: root ${index} ${other.path} (${other.images}) vs native ${root.path} (${root.images})`,
        );
      }
    });
  }
}
