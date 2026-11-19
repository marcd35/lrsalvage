// The dedicated worker: where the seam runs.
//
// It has to be here. The origin-private store that SQLite opens through is
// specified for a dedicated worker in a secure context, and the seam's blocking
// bridge needs `Atomics.wait`, which a page's main thread may not call. So the
// page makes the handles and this worker makes the requests: it parks on a
// `SharedArrayBuffer` while the page answers (`web/lrsalvage-fs.js`), and runs
// everything else itself.
//
// `globalThis.lrsalvage` is the name the generated `wasm-bindgen` glue calls
// (`js_namespace = "lrsalvage"` in `crates/lrsalvage-web/src/wasm.rs`), and it is
// installed before the first call into wasm can happen.

import { installBridge } from './lrsalvage-fs.js';
import init, { poll_install, probe, scan, start_install } from './pkg/lrsalvage_web.js';

const log = (...parts) => console.log('[lrsalvage-worker]', ...parts);

/// Yield to the event loop *without* the clamp a zero timeout earns: a settled
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
  if (message?.type !== 'start') return;
  try {
    await start(message);
  } catch (error) {
    self.postMessage({
      type: 'failed',
      stage: 'start',
      error: error && error.stack ? error.stack : String(error),
    });
  }
};

async function start({ sab, mounts, capacity, clear, vaults, probePaths }) {
  const started = performance.now();
  const exports = await init();
  log('wasm instantiated in', Math.round(performance.now() - started), 'ms');

  // Only the page's file-backed mounts come here; a picked `File`'s bytes are read
  // in this worker, synchronously, and everything else is the page's.
  installBridge({ memory: exports.memory, sab, mounts: mounts.filter((m) => m.kind === 'file') });

  const installed = performance.now();
  // Started and polled, not awaited: the store's install is a promise and this
  // crate takes no async runtime for one of them (`deny.toml` bans one, and the
  // seam is synchronous everywhere else). `specs/modules/fs.md` records it.
  start_install(
    JSON.stringify(mounts.map(({ path, store }) => ({ path, store: store === true }))),
    capacity,
    clear,
  );
  for (;;) {
    const state = poll_install();
    if (state === 'done') break;
    if (state.startsWith('failed')) throw new Error(state);
    await macrotask();
  }
  log('store installed in', Math.round(performance.now() - installed), 'ms');

  const checks = JSON.parse(probe(probePaths.dir, probePaths.store));
  log('probe', checks.filter((check) => check.outcome !== 'ok').length, 'checks off-ok');

  const runs = [];
  for (const vault of vaults) {
    const began = performance.now();
    const report = JSON.parse(scan(vault.catalog, vault.cache, vault.output));
    log('scanned', vault.name, 'in', Math.round(performance.now() - began), 'ms');
    runs.push({ name: vault.name, report });
  }

  self.postMessage({
    type: 'done',
    probe: checks,
    runs,
    mountMs: Math.round(installed - started),
    totalMs: Math.round(performance.now() - started),
  });
}
