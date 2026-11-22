// The dedicated worker: where the seam runs, and where a scan happens.
//
// It has to be here. The origin-private store that SQLite opens through is
// specified for a dedicated worker in a secure context, and the seam's blocking
// bridge needs `Atomics.wait`, which a page's main thread may not call. So the
// page makes the handles and this worker makes the requests: it parks on a
// `SharedArrayBuffer` while the page answers (`seam/lrsalvage-fs.js`), and runs
// everything else itself.
//
// Progress is the one thing that travels the other way while the run is inside a
// synchronous wasm frame: `postMessage` does not need the event loop, so the
// worker's `scan_progress` host call forwards each unit and reads the page's stop
// flag out of its own four-byte `SharedArrayBuffer`. A message *to* the worker
// could not be read until the run ended, which is why the flag is memory and not
// a message (`#149`, DEC-S3).
//
// `globalThis.lrsalvage` is the name the generated `wasm-bindgen` glue calls
// (`js_namespace = "lrsalvage"` in `crates/lrsalvage-app/src/wasm.rs`), and it is
// installed before the first call into wasm can happen.

import { installBridge } from './seam/lrsalvage-fs.js';
import init, {
  pause_store, poll_install, read_preview, scan, scan_cache, start_install,
  export_paths, prepare_export, existing_export, export_reports,
  verification_report, versions,
} from './pkg/lrsalvage_app.js';

/**
 * Paint at most this often. The Rust half reports every unit — a 100,000-image
 * catalog crosses the boundary that many times — and a DOM update does not cost
 * what a `u64` does, so the decision to paint is made here (`specs/modules/plan.md`,
 * *Decisions*). A stage change and the last unit of a stage always paint.
 */
const PAINT_MS = 100;

const log = (...parts) => console.log('[lrsalvage-app]', ...parts);

// Yield to the event loop *without* the clamp a zero timeout earns: a settled
/// browser promise's callback runs in a macrotask, and the store's install is a
/// chain of them. A `MessageChannel` task is one of those and is not clamped.
const channel = new MessageChannel();
const macrotask = () =>
  new Promise((resolve) => {
    channel.port2.onmessage = () => resolve();
    channel.port1.postMessage(null);
  });

self.onmessage = async (event) => {
  const message = event.data;
  if (message?.type === 'preview') {
    const { request, id } = message;
    try {
      const bytes = read_preview(id);
      self.postMessage({ type: 'preview', request, bytes }, [bytes.buffer]);
    } catch (error) {
      const problem = error?.kind === 'changed-size' ? error : { kind: 'unreadable' };
      self.postMessage({ type: 'preview', request, problem });
    }
    return;
  }
  if ([ 'export-paths', 'export-jpeg', 'export-existing', 'export-reports', 'verification-report' ].includes(message?.type)) {
    const { request, id, flat, outcomes } = message;
    try {
      if (message.type === 'verification-report') {
        // The text, not a re-serialised value: the report's formatting is the
        // report, and it goes straight into the file the page writes.
        const text = verification_report(Math.floor(Date.now() / 1000));
        self.postMessage({ type: 'export', request, value: { text } });
      } else if (message.type === 'export-jpeg') {
        const jpeg = prepare_export(id, flat);
        try {
          const bytes = jpeg.take_bytes();
          const outcome = JSON.parse(jpeg.outcome());
          self.postMessage({ type: 'export', request, value: { bytes, outcome } }, [bytes.buffer]);
        } finally {
          jpeg.free();
        }
      } else {
        let value;
        switch (message.type) {
          case 'export-paths': value = export_paths(flat); break;
          case 'export-existing': value = existing_export(id, flat, message.bytes); break;
          case 'export-reports':
            value = export_reports(flat, JSON.stringify(outcomes), Math.floor(Date.now() / 1000));
            break;
        }
        self.postMessage({ type: 'export', request, value: JSON.parse(value) });
      }
    } catch (error) {
      self.postMessage({ type: 'export', request, problem: String(error?.message ?? error) });
    }
    return;
  }
  if (message?.type !== 'scan') return;
  try {
    await run(message);
  } catch (error) {
    self.postMessage({
      type: 'failed',
      error: error && error.stack ? error.stack : String(error),
    });
  }
};

async function run({ sab, stop, mounts, catalog, cache, capacity }) {
  const started = performance.now();
  const exports = await init();
  log('wasm instantiated in', Math.round(performance.now() - started), 'ms');
  // The two release numbers, from the module that will do the work. Sent before
  // the store is installed so a run that fails there still has them.
  self.postMessage({ type: 'versions', versions: JSON.parse(versions()) });

  const flag = new Int32Array(stop);
  let lastPaint = 0;
  let lastPhase = -1;
  const progress = (phase, done, total) => {
    const now = performance.now();
    const boundary = phase !== lastPhase || (total > 0 && done === total);
    if (boundary || now - lastPaint >= PAINT_MS) {
      lastPaint = now;
      lastPhase = phase;
      self.postMessage({ type: 'progress', phase, done, total, at: now });
    }
    // Read on *every* call, painted or not: a stop must not wait for the next
    // paint to be noticed.
    return Atomics.load(flag, 0) === 0 ? 0 : 1;
  };

  installBridge({ memory: exports.memory, sab, mounts: [], progress });

  const installed = performance.now();
  // Started and polled, not awaited: the store's install is a promise and the
  // core takes no async runtime for one of them (`specs/modules/fs.md`).
  start_install(JSON.stringify(mounts), capacity, true);
  for (;;) {
    const state = poll_install();
    if (state === 'done') break;
    if (state.startsWith('failed')) throw new Error(state);
    await macrotask();
  }
  log('store installed in', Math.round(performance.now() - installed), 'ms');
  // The next stage is one call with no unit inside it — a whole catalog copied
  // into the pool — so the page is told to name it and not to draw a bar for it.
  self.postMessage({ type: 'stage', stage: 'copy' });

  const began = performance.now();
  const report = JSON.parse(catalog ? scan(catalog, cache) : scan_cache(cache));
  // Preview reads address the picked cache through the bridge. Release the
  // SQLite pool before reporting so a restart or recheck can install it again.
  pause_store();
  log('scan finished in', Math.round(performance.now() - began), 'ms');
  self.postMessage({ type: 'report', report });
}
