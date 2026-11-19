// The app's flow, with no DOM in it.
//
// One function per thing the page can do, so the page's wiring is a handful of
// `addEventListener` lines and a harness page can drive the same functions in the
// same browser with the same values (`web/test.js`, and `#149`'s *Testing
// Decisions*: a File System Access dialog cannot be answered by any automation we
// have, so the picker is the one line a test supplies the *answer* to).
//
// What a run is, in the order `lrsalvage-app`'s Rust half expects it:
//
// 1. The page owns the handles, so it mounts them: a picked folder is one mount,
//    and a split backup's cache folder is a second.
// 2. The worker owns the wasm, so it answers the seam's filesystem calls through
//    the page over a `SharedArrayBuffer` (`seam/fs-host.js`), installs the
//    origin-private store, and runs the scan.
// 3. Progress arrives by message; the stop request travels the other way through
//    a second, four-byte `SharedArrayBuffer`, because the worker is inside a
//    synchronous wasm frame and cannot read a message until it ends.

import { FsHost } from './seam/fs-host.js';

/** The answer buffer the seam's bridge parks the worker on. */
const SAB_BYTES = 1024 * 1024 + 8;

/** Store slots: the catalog, its two sidecars, the index, and room to spare. */
const CAPACITY = 8;

/**
 * Where the origin-private pool is mounted.
 *
 * Not a folder a page owns: the pool answers in-process, so this mount has no
 * handle and never reaches `FsHost`. It is the namespace the seam's `scratch_dir`,
 * the index copy and SQLite's VFS all allocate under (`specs/modules/fs.md`).
 */
const STORE_MOUNT = '/store';

/** `plan::Progress`'s three stages, by the number the wasm module reports. */
export const PHASES = ['cache', 'payloads', 'images'];

/**
 * Whether this browser can run a recovery at all, and why not when it cannot.
 *
 * Asked before anything is offered, rather than failing at the picker (`#117`:
 * "the page says so before a user picks anything"). Each reason is a different
 * sentence in `view.js`; `showDirectoryPicker` is the one that is genuinely
 * missing elsewhere (Firefox, Safari), the other two mean this page is not
 * served the way it has to be.
 */
export function capability(browser = globalThis) {
  if (typeof browser.showDirectoryPicker !== 'function') {
    return { ok: false, reason: 'no-picker' };
  }
  if (typeof browser.SharedArrayBuffer !== 'function' || browser.crossOriginIsolated !== true) {
    return { ok: false, reason: 'no-isolation' };
  }
  if (browser.isSecureContext === false) {
    return { ok: false, reason: 'insecure' };
  }
  return { ok: true };
}

/**
 * One folder pick, with the three answers a page has to tell apart.
 *
 * A cancelled dialog and a refused one are one `AbortError` apart, and the
 * difference matters: cancelling is a normal thing to do and leaves the page as
 * it was, while a refusal deserves the browser's own words. Measured on Chrome
 * 157.0.8103.79 in `--headless=new`: a picker rejects `AbortError` without a
 * dialog, so this is the branch a headless run exercises for real.
 */
export async function pickFolder({ id = 'lrsalvage-catalog', mode = 'read' } = {}) {
  try {
    const handle = await globalThis.showDirectoryPicker({ id, mode });
    return { handle };
  } catch (error) {
    if (error && error.name === 'AbortError') {
      return { cancelled: true };
    }
    return {
      refused: {
        name: (error && error.name) || 'Error',
        message: (error && error.message) || String(error),
      },
    };
  }
}

/**
 * What a picked folder holds: the catalogs in it, and the preview cache beside
 * them.
 *
 * The folder is the entry unit, not the catalog file, because a picked *file*
 * cannot reach its siblings: `FileSystemHandle` exposes a name and a kind and no
 * path, so the `-wal` SQLite needs and the cache the next step wants are both
 * unreachable from it (`#149`, DEC-S1).
 *
 * The cache suggestion is the `.lrdata` whose name starts with the catalog's, or
 * the only `.lrdata` there is. Anything else is the user's to pick — a split
 * backup is exactly the case where no suggestion is right.
 */
export async function folderSource(handle) {
  const catalogs = [];
  const caches = [];
  for await (const entry of handle.values()) {
    if (entry.kind === 'file' && /\.lrcat$/i.test(entry.name)) {
      catalogs.push(entry.name);
    }
    if (entry.kind === 'directory' && /\.lrdata$/i.test(entry.name)) {
      caches.push(entry.name);
    }
  }
  catalogs.sort();
  caches.sort();

  return {
    folder: handle.name,
    catalogs,
    caches,
    catalog: catalogs.length === 1 ? catalogs[0] : null,
    cache:
      caches.find((name) => catalogs.some((catalog) => sameStem(name, catalog))) ??
      (caches.length === 1 ? caches[0] : null),
  };
}

/** `2023.lrcat` and `2023 Previews.lrdata` share a stem. */
function sameStem(cache, catalog) {
  const stem = catalog.slice(0, catalog.length - '.lrcat'.length);
  return cache.toLowerCase().startsWith(stem.toLowerCase());
}

/**
 * Everything a run needs, from the handles and the two names the user ended up
 * with.
 *
 * `mounts` is the page's half: a path and the handle that answers for it. The
 * catalog's folder is `/source`; a separately picked cache is `/cache`.
 */
export function sourceFrom({
  handle,
  catalog,
  cacheHandle = null,
  cacheName = null,
  cache = null,
  mounts = [],
}) {
  const all = [{ path: '/source', handle }, ...mounts];
  let cachePath = cache ? `/source/${cache}` : null;
  if (cacheHandle) {
    all.push({ path: '/cache', handle: cacheHandle });
    // A picked folder may be the cache itself or the folder holding it, so the
    // name is what decides: no name means the mount root *is* the cache.
    cachePath = cacheName ? `/cache/${cacheName}` : '/cache';
  }
  return {
    mounts: all,
    catalog: `/source/${catalog}`,
    cache: cachePath,
    label: catalog,
  };
}

/**
 * [`sourceFrom`]'s source with one more folder mounted at a path the catalog
 * spells.
 *
 * This is what "check again" does: the recorded root folder is mounted where the
 * catalog says it was, and the next scan asks the same question the desktop asks
 * — is the original there? — because the seam's namespace now answers for that
 * path (`#149`, DEC-S7).
 */
export function withMount(source, path, handle) {
  return { ...source, mounts: [...source.mounts, { path, handle }] };
}

/**
 * Start one scan, and hand back what it needs to be stopped.
 *
 * `ui.stage(name)` is called with `'install'`, `'copy'`, `'scan'` and `'report'`;
 * `ui.progress({phase, done, total})` with every report the worker forwards; and
 * `done` resolves with the Rust half's report — `{ok: true, …}`,
 * `{ok: true, stopped: true}`, or `{ok: false, problem: {stage, kind, message}}`.
 * It never rejects for a scan that failed: a failure is a report, so the page has
 * one code path and a person gets a sentence.
 */
export function start(source, ui = {}) {
  const answer = new SharedArrayBuffer(SAB_BYTES);
  const stop = new SharedArrayBuffer(4);
  const flag = new Int32Array(stop);
  // The page is the only side with an event loop, so it is the side that answers
  // the worker's filesystem calls while the worker is parked. Every request has to
  // be handed to `serve`: the worker is inside `Atomics.wait` until it is.
  const host = new FsHost(answer, source.mounts);

  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

  const done = new Promise((resolve, reject) => {
    worker.onmessage = (event) => {
      const message = event.data;
      if (message?.type === 'fs') {
        void host.serve(message);
        return;
      }
      if (message?.type === 'stage') {
        ui.stage?.(message.stage);
        return;
      }
      if (message?.type === 'progress') {
        ui.stage?.('scan');
        ui.progress?.(message);
        return;
      }
      if (message?.type === 'report') {
        ui.stage?.('report');
        resolve(message.report);
        return;
      }
      if (message?.type === 'failed') {
        reject(new Error(message.error));
      }
    };
    worker.onerror = (event) => {
      reject(new Error(event.message || 'the worker failed to start'));
    };
    ui.stage?.('install');
    worker.postMessage({
      type: 'scan',
      sab: answer,
      stop,
      mounts: [
        ...source.mounts.map((mount) => ({ path: mount.path, store: false })),
        { path: STORE_MOUNT, store: true },
      ],
      catalog: source.catalog,
      cache: source.cache,
      capacity: CAPACITY,
    });
  }).finally(() => worker.terminate());

  return {
    /** Ask the scan to stop. It stops at the next unit boundary, which is one file. */
    stop() {
      Atomics.store(flag, 0, 1);
    },
    done,
  };
}
