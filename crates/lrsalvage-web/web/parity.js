// The harness page: mount one vault twice, run the seam on it, compare with the
// native run.
//
// This is the browser half of `scripts/browser-parity.sh`. What it does, in the
// order it matters:
//
// 1. Reads `native.json`, written by `cargo run --bin vault-parity` for the two
//    vaults and the host's run of the seam's own checks.
// 2. Copies each vault into its own origin-private directory, byte for byte —
//    so the browser reads what the native run read rather than a second building
//    of the same composition.
// 3. Starts the worker that holds the wasm, hands it a `SharedArrayBuffer` and
//    the mount list, and answers its filesystem requests from this thread.
// 4. Compares: same statuses, same counts, same JPEG digests, with
//    `MAIN_SEPARATOR` normalised (`docs/adr/0012-two-release-lines.md` D12).
//
// The result is `window.__result` and `window.__done`, which is what the CDP
// driver reads. Every failure is in `__result.differences` as a sentence; a run
// that cannot happen at all posts `__result.error`.

import { FsHost } from './fs-host.js';

const SAB_BYTES = 1024 * 1024 + 8;
const CAPACITY = 64;
const DIRS = ['plain', 'wal', 'dest-plain', 'dest-wal', 'probe'];

const logNode = document.getElementById('log');
function log(...parts) {
  const line = parts
    .map((part) => (typeof part === 'string' ? part : JSON.stringify(part)))
    .join(' ');
  logNode.textContent += line + '\n';
  console.log(...parts);
}

window.__result = { ok: false, differences: ['the harness did not finish'] };
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
    // not cross-origin isolated cannot make one. `scripts/browser-parity.sh`
    // serves COOP/COEP; anything else serving this page has to as well.
    throw new Error(
      'not cross-origin isolated: the page needs COOP same-origin and COEP require-corp',
    );
  }

  const native = await json('/native.json');
  const byName = new Map(native.vaults.map((vault) => [vault.name, vault]));

  const base = await navigator.storage.getDirectory();
  const root = await freshDirectory(base, 'lrsalvage-parity');
  const dirs = {};
  for (const name of DIRS) {
    dirs[name] = await root.getDirectoryHandle(name, { create: true });
  }

  const began = performance.now();
  for (const vault of native.vaults) {
    for (const relative of vault.files) {
      const path = `/${vault.name}/${relative.split('/').map(encodeURIComponent).join('/')}`;
      const response = await fetch(path);
      if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
      await writeFile(dirs[vault.name], relative, new Uint8Array(await response.arrayBuffer()));
    }
  }
  log('mounted', native.vaults.length, 'vaults in', Math.round(performance.now() - began), 'ms');

  // The plain vault's catalog is picked as a *file*, which is one of the three
  // mount kinds and the one #117's first user story uses. The `-wal` vault has to
  // be a directory, because it needs a sidecar beside the catalog.
  const plain = byName.get('plain');
  const pickedBytes = await fetch(
    `/plain/${plain.catalog.split('/').map(encodeURIComponent).join('/')}`,
  ).then((response) => response.arrayBuffer());
  const picked = new File([pickedBytes], 'catalog.lrcat');

  const pageMounts = [
    { path: '/plain', kind: 'dir', handle: dirs.plain },
    { path: '/wal', kind: 'dir', handle: dirs.wal },
    { path: '/dest-plain', kind: 'dir', handle: dirs['dest-plain'] },
    { path: '/dest-wal', kind: 'dir', handle: dirs['dest-wal'] },
    { path: '/probe', kind: 'dir', handle: dirs.probe },
  ];
  const sab = new SharedArrayBuffer(SAB_BYTES);
  const host = new FsHost(sab, pageMounts);

  const workerMounts = [
    ...pageMounts.map(({ path, kind }) => ({ path, kind, store: false })),
    { path: '/picked/catalog.lrcat', kind: 'file', store: false, file: picked },
    { path: '/store', kind: 'store', store: true },
  ];

  const wal = byName.get('wal');
  const vaults = [
    {
      name: 'plain',
      catalog: '/picked/catalog.lrcat',
      cache: `/plain/${plain.cache}`,
      output: '/dest-plain',
    },
    {
      name: 'wal',
      catalog: `/wal/${wal.catalog}`,
      cache: `/wal/${wal.cache}`,
      output: '/dest-wal',
    },
  ];

  const worker = new Worker('./worker.js', { type: 'module' });
  worker.onerror = (error) => {
    window.__result = {
      ok: false,
      error: `worker: ${error.message || 'no message'}`,
      differences: [],
    };
    window.__done = true;
  };
  worker.onmessage = async (event) => {
    const message = event.data;
    try {
      if (message.type === 'fs') {
        await host.serve(message);
        return;
      }
      if (message.type === 'log') {
        log(message.text);
        return;
      }
      if (message.type === 'failed') {
        window.__result = {
          ok: false,
          error: `worker ${message.stage}: ${message.error}`,
          differences: [],
        };
        window.__done = true;
        return;
      }
      if (message.type === 'done') {
        const result = compare(native, message);
        window.__result = result;
        window.__done = true;
        log(result.ok ? 'parity: ok' : 'parity: differences', result.differences);
      }
    } catch (error) {
      window.__result = {
        ok: false,
        error: error && error.stack ? error.stack : String(error),
        differences: [],
      };
      window.__done = true;
    }
  };

  log('starting the worker');
  worker.postMessage({
    type: 'start',
    sab,
    mounts: workerMounts,
    capacity: CAPACITY,
    clear: true,
    vaults,
    probePaths: { dir: '/probe', store: '/store' },
  });
}

/** The native document and the browser document, held to each other. */
function compare(native, browser) {
  const differences = [];
  const expected = new Map(native.probe.map((check) => [check.name, check]));

  for (const check of browser.probe) {
    const want = expected.get(check.name);
    if (want === undefined) {
      differences.push(`probe ${check.name}: the native run has no such check`);
      continue;
    }
    if (want.diverges) continue;
    if (want.outcome !== check.outcome) {
      differences.push(`probe ${check.name}: native ${want.outcome}, browser ${check.outcome}`);
    }
  }
  for (const name of expected.keys()) {
    if (!browser.probe.some((check) => check.name === name)) {
      differences.push(`probe ${name}: the browser run has no such check`);
    }
  }

  // A declared divergence has to actually diverge: a browser that "set" a
  // modification time would be claiming an operation this target does not have,
  // and one that copied onto a `-wal` name would be publishing an image the WAL
  // still describes. Both are refusals the spec promises, so they are asserted
  // rather than merely reported.
  for (const name of ['set_mtime_sets_the_time_or_refuses', 'store_copy_refuses_a_sidecar_name']) {
    const check = browser.probe.find((candidate) => candidate.name === name);
    if (check === undefined) {
      differences.push(`${name}: the browser run has no such check`);
    } else if (!check.outcome.startsWith('refused')) {
      differences.push(`${name}: the browser answered ${check.outcome}`);
    }
  }

  const numbers = { probe: {}, vaults: {} };
  numbers.probe = {
    checks: browser.probe.length,
    off: browser.probe.filter((check) => check.outcome !== 'ok').map((check) => check.name),
    divergences: browser.probe
      .filter((check) => check.diverges)
      .map((check) => `${check.name}=${check.outcome}`),
    mount_ms: browser.mountMs,
    total_ms: browser.totalMs,
  };

  // `separator` is the field ADR 0012 D12 tells us to normalise: it is `\` on
  // Windows and `/` in a page, and it is the only field allowed to differ.
  const strip = (report) => {
    const { separator, ...rest } = report;
    return rest;
  };
  const nativeVaults = new Map(native.vaults.map((vault) => [vault.name, vault.report]));
  for (const run of browser.runs) {
    const want = nativeVaults.get(run.name);
    if (want === undefined) {
      differences.push(`vault ${run.name}: the native run has no such vault`);
      continue;
    }
    const left = JSON.stringify(strip(want));
    const right = JSON.stringify(strip(run.report));
    if (left !== right) {
      differences.push(`vault ${run.name}: reports differ\n  native  ${left}\n  browser ${right}`);
    }
    numbers.vaults[run.name] = {
      images: run.report.stats.images,
      recoverable: run.report.stats.recoverable,
      plan_items: run.report.plan_items,
      written: run.report.written,
      index_problem: run.report.index_problem,
      source_sha256: run.report.source_sha256,
      snapshot_sha256: run.report.snapshot_sha256,
      outputs_sha256: run.report.outputs_sha256,
      plan_statuses_sha256: run.report.plan_statuses_sha256,
      outputs: run.report.outputs.map((entry) => entry.path),
    };
  }

  return { ok: differences.length === 0, differences, numbers };
}

async function json(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response.json();
}

async function freshDirectory(base, name) {
  try {
    await base.removeEntry(name, { recursive: true });
  } catch (error) {
    if (error.name !== 'NotFoundError') throw error;
  }
  return base.getDirectoryHandle(name, { create: true });
}

async function writeFile(dir, relative, bytes) {
  const parts = relative.split('/');
  let handle = dir;
  for (const part of parts.slice(0, -1)) {
    handle = await handle.getDirectoryHandle(part, { create: true });
  }
  const file = await handle.getFileHandle(parts[parts.length - 1], { create: true });
  const stream = await file.createWritable();
  await stream.write(bytes);
  await stream.close();
}
