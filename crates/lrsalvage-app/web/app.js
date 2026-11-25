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
import { suggestedCache } from './folders.js';

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
 *
 * `startIn` is where the dialog opens, and it is a convenience: the platform
 * may reject a handle it will not reopen in this mode (a folder picked read-only
 * inside a read-write picker is not guaranteed), so a `TypeError` retries once
 * without it rather than failing a pick the user could still make. Nothing else
 * is retried — a cancel and a refusal both keep their own answer.
 */
export async function pickFolder({ id = 'lrsalvage-catalog', mode = 'read', startIn = null } = {}) {
  const ask = (where) =>
    where ? globalThis.showDirectoryPicker({ id, mode, startIn: where })
      : globalThis.showDirectoryPicker({ id, mode });
  try {
    return { handle: await ask(startIn) };
  } catch (error) {
    if (error && error.name === 'AbortError') {
      return { cancelled: true };
    }
    if (startIn && error && error.name === 'TypeError') {
      try {
        return { handle: await ask(null) };
      } catch (again) {
        return refusal(again);
      }
    }
    return refusal(error);
  }
}

/** The browser's own words, in the shape the page's three answers expect. */
function refusal(error) {
  return {
    refused: {
      name: (error && error.name) || 'Error',
      message: (error && error.message) || String(error),
    },
  };
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
 * The cache suggestion is [`suggestedCache`]'s answer for the catalog: the exact
 * `<stem> Previews.lrdata`, never one of Adobe's companion folders (`folders.js`)
 * and never a guess between several. A null suggestion is what the page shows as
 * "no preview folder beside that catalog"; the user picks one.
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
    cache: suggestedCache(caches, catalogs.length === 1 ? catalogs[0] : null),
  };
}

/**
 * Everything a run needs, from the handles and the two names the user ended up
 * with.
 *
 * `mounts` is the page's half: a path and the handle that answers for it. The
 * catalog's folder is `/source`; a separately picked cache is `/cache`. With
 * no picked or detected cache, scan the expected sibling path so the report can
 * still list the catalog photos whose previews are unavailable.
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
  const sibling = cache ?? catalog.replace(/\.lrcat$/i, ' Previews.lrdata');
  let cachePath = `/source/${sibling}`;
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
 * One scan of one source, and hand back what it needs to be stopped.
 *
 * `ui.stage(name)` is called with `'install'`, `'copy'`, `'scan'` and `'report'`;
 * `ui.progress({phase, done, total, at})` with every report the worker forwards,
 * `at` being the worker's monotonic milliseconds at the unit; and
 * `done` resolves with the Rust half's report — `{ok: true, …}`,
 * `{ok: true, stopped: true}`, or `{ok: false, problem: {stage, kind, message}}`.
 * Scan failures are reports. Worker failures reject `done`.
 * In review mode a completed worker answers `preview(id)` until `close()`.
 *
 * `source.mode` names which entry the run takes — `catalog`, `cache-only` or
 * `legacy`, the same words the report files carry — and the worker picks the
 * module call from it. A source without one is the catalog-backed run when it
 * has a catalog and the indexed cache one otherwise, which is what every caller
 * written before the third mode said.
 */
export function start(source, ui = {}, { review = false } = {}) {
  const answer = new SharedArrayBuffer(SAB_BYTES);
  const stop = new SharedArrayBuffer(4);
  const flag = new Int32Array(stop);
  // The page is the only side with an event loop, so it is the side that answers
  // the worker's filesystem calls while the worker is parked. Every request has to
  // be handed to `serve`: the worker is inside `Atomics.wait` until it is.
  const host = new FsHost(answer, source.mounts);

  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

  const pending = new Map();
  let nextRequest = 0;
  let closed = false;
  let ready = false;

  function close(reason = new Error('This recovery has ended.')) {
    if (closed) return;
    closed = true;
    worker.terminate();
    for (const { reject } of pending.values()) reject(reason);
    pending.clear();
  }

  const done = new Promise((resolve, reject) => {
    worker.onmessage = (event) => {
      const message = event.data;
      if (message?.type === 'fs') {
        void host.serve(message);
        return;
      }
      if (message?.type === 'versions') {
        // The compiled pair, which is why the page does not have to trust a
        // `build.json` a development tree never wrote.
        ui.versions?.(message.versions);
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
        ready = review && message.report.ok && !message.report.stopped;
        if (!ready) close();
        resolve(message.report);
        return;
      }
      if (message?.type === 'preview' || message?.type === 'export') {
        const ask = pending.get(message.request);
        if (!ask) return;
        pending.delete(message.request);
        if (message.problem) ask.reject(message.type === 'export' ? new Error(message.problem) : message.problem);
        else if (message.type === 'export') ask.resolve(message.value);
        else ask.resolve(new Blob([message.bytes], { type: 'image/jpeg' }));
        return;
      }
      if (message?.type === 'failed') {
        const error = new Error(message.error);
        close(error);
        reject(error);
      }
    };
    worker.onerror = (event) => {
      const error = new Error(event.message || 'The recovery worker stopped.');
      close(error);
      reject(error);
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
      mode: source.mode ?? (source.catalog ? 'catalog' : 'cache-only'),
      capacity: CAPACITY,
    });
  });

  function ask(type, details, transfer = []) {
    if (closed || !ready) return Promise.reject(new Error('This recovery has ended.'));
    return new Promise((resolve, reject) => {
      const request = ++nextRequest;
      pending.set(request, { resolve, reject });
      worker.postMessage({ type, request, ...details }, transfer);
    });
  }

  return {
    /** Ask the scan to stop. It stops at the next unit boundary, which is one file. */
    stop() {
      Atomics.store(flag, 0, 1);
    },
    done,
    preview: (id) => ask('preview', { id }),
    exportPaths: (flat) => ask('export-paths', { flat }),
    exportJpeg: (id, flat) => ask('export-jpeg', { id, flat }),
    existingExport: (id, flat, bytes) => ask('export-existing', { id, flat, bytes }, [bytes.buffer]),
    exportReports: (flat, outcomes) => ask('export-reports', { flat, outcomes }),
    verificationReport: (writes) => ask('verification-report', { writes }),
    close,
  };
}
