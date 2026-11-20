// The seam's browser bridge, from the worker's side.
//
// `lrsalvage_core`'s `fs` seam is synchronous: `plan`, `extract` and `snapshot`
// call it in a straight line and no signature yields. On this target a picked
// folder answers every question with a promise, so this file is where one becomes
// the other. The worker hands the operation to the page, then *parks* on a
// `SharedArrayBuffer` until the page has written the answer and notified it
// (`Atomics.wait` is legal in a worker and not on the main thread, which is why
// the run happens off the page in the first place).
//
// The protocol itself — operation codes, the JSON request, the `[kind][bytes]`
// answer — is `crates/lrsalvage-core/src/fs/bridge.rs`, and `protocol.js` is its
// JavaScript half, shared with the page's `fs-host.js`. This file adds only what
// the browser can answer *without* the page: a picked `File`'s bytes, read
// synchronously in this worker.
//
// Written as a module so `worker.js` can import it; the exported `installBridge`
// puts the one function on `globalThis`, which is the name the generated
// `wasm-bindgen` glue calls (`js_namespace = "lrsalvage"`).

import {
  CTRL_WORDS,
  KIND_NOT_FOUND,
  KIND_OK,
  KIND_OTHER,
  KIND_PERMISSION,
  OP_CANONICALIZE,
  OP_READ,
  OP_STAT,
  PAYLOAD_OFFSET,
  deepestFirst,
  mountFor,
  normalise,
} from './protocol.js';

/** How long the worker will wait for the page before giving up. A wedged page
/// must be a reported failure, not a tab that never finishes. */
const WAIT_MS = 120_000;

export class Bridge {
  /**
   * @param {WebAssembly.Memory} memory the instance's linear memory
   * @param {SharedArrayBuffer} sab the answer buffer shared with the page
   * @param {Array<{path: string, kind: string, file?: File}>} mounts
   */
  constructor(memory, sab, mounts) {
    this.memory = memory;
    this.ctrl = new Int32Array(sab, 0, CTRL_WORDS);
    this.payload = new Uint8Array(sab, PAYLOAD_OFFSET);
    this.mounts = deepestFirst(mounts);
    this.nextId = 1;
    this.reader = null;
  }

  mountFor(path) {
    return mountFor(this.mounts, path);
  }

  /** The one function the seam calls. */
  call(op, requestPtr, requestLen, outPtr, outCap) {
    try {
      const request = JSON.parse(
        new TextDecoder().decode(new Uint8Array(this.memory.buffer, requestPtr, requestLen)),
      );
      // Only a picked `File` is answered here. Everything else belongs to the
      // page, which holds the handles and so owns the mount table's other half —
      // including the answer for a path nothing is mounted at.
      const mount = this.mountFor(request.path);
      if (mount !== null && mount.kind === 'file') {
        return this.answerLocally(op, mount, request, outPtr, outCap);
      }
      const { kind, bytes } = this.blocking(op, request);
      return this.write(outPtr, outCap, kind, bytes);
    } catch (error) {
      const message = error && error.message ? error.message : String(error);
      return this.answer(outPtr, outCap, KIND_OTHER, message);
    }
  }

  /** Post one operation and park until the page answers it. */
  blocking(op, request) {
    const id = this.nextId++;
    Atomics.store(this.ctrl, 0, 0);
    Atomics.store(this.ctrl, 1, 0);
    self.postMessage({ type: 'fs', id, op, request });
    const outcome = Atomics.wait(this.ctrl, 0, 0, WAIT_MS);
    if (outcome === 'timed-out') {
      throw new Error(`the page did not answer operation ${op} within ${WAIT_MS / 1000} s`);
    }
    // Control word 0 is the readiness flag the wake-up waits on; the answer's
    // own kind byte is the first byte of the payload, and the length in control
    // word 1 counts that byte.
    const length = Atomics.load(this.ctrl, 1);
    const kind = this.payload[0];
    return { kind, bytes: this.payload.slice(1, length) };
  }

  /**
   * The operations a picked `File` answers without the page: its bytes and its
   * length, synchronously, which is what `FileReaderSync` is for (File API §6.5,
   * workers only — `docs/browser-spike.md` §1).
   */
  answerLocally(op, mount, request, outPtr, outCap) {
    this.reader ??= new FileReaderSync();
    const file = mount.file;
    const rest = normalise(request.path).slice(mount.path.length);
    if (rest !== '') {
      return this.answer(outPtr, outCap, KIND_NOT_FOUND, `${request.path} is not ${mount.path}`);
    }
    switch (op) {
      case OP_READ: {
        const offset = Number(request.offset ?? 0);
        const length = Math.min(Number(request.len ?? 0), Math.max(0, file.size - offset));
        const blob = file.slice(offset, offset + length);
        const bytes = new Uint8Array(this.reader.readAsArrayBuffer(blob));
        return this.write(outPtr, outCap, KIND_OK, bytes);
      }
      case OP_STAT: {
        const stat = JSON.stringify({ exists: true, file: true, len: file.size });
        return this.answer(outPtr, outCap, KIND_OK, stat);
      }
      case OP_CANONICALIZE: {
        const answer = JSON.stringify({ path: mount.path });
        return this.answer(outPtr, outCap, KIND_OK, answer);
      }
      default:
        // A picked file is evidence: nothing writes to it, and saying so is
        // better than pretending the mount is missing.
        return this.answer(
          outPtr,
          outCap,
          KIND_PERMISSION,
          `${mount.path} is a picked file and answers read-only`,
        );
    }
  }

  answer(outPtr, outCap, kind, text) {
    return this.write(outPtr, outCap, kind, new TextEncoder().encode(text));
  }

  /** `[kind][bytes]` into the seam's buffer, and its length. */
  write(outPtr, outCap, kind, bytes) {
    const total = 1 + bytes.length;
    if (total > outCap) {
      const message = new TextEncoder().encode(
        `the answer needs ${total} bytes and the seam offered ${outCap}`,
      );
      return this.write(outPtr, outCap, KIND_OTHER, message);
    }
    const view = new Uint8Array(this.memory.buffer, outPtr, total);
    view[0] = kind;
    view.set(bytes, 1);
    return total;
  }
}

/**
 * Install `globalThis.lrsalvage`: the seam's call, and the scan's progress report.
 *
 * Both names are declared by the wasm module and read when it is instantiated, so
 * a page that paints nothing still has to install `scan_progress` — hence the
 * default that carries on and reports nowhere. `progress` is what a page that
 * does paint passes: it is called once per unit (a cache file walked, a level
 * file measured, a catalog image finished) and answers `0` to carry on and
 * anything else to stop. It runs inside the run's synchronous frame, so it may
 * not await: it posts, reads the stop flag, and returns.
 */
export function installBridge({ memory, sab, mounts, progress }) {
  const bridge = new Bridge(memory, sab, mounts);
  globalThis.lrsalvage = {
    fs_call: (op, requestPtr, requestLen, outPtr, outCap) =>
      bridge.call(op, requestPtr, requestLen, outPtr, outCap),
    scan_progress: progress ?? (() => 0),
  };
  return bridge;
}
