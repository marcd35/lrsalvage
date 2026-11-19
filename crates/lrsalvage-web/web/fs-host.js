// The seam's browser host, from the page's side.
//
// The worker is parked inside `Atomics.wait` while this runs, so every operation
// here may await freely: this is the thread with an event loop. It owns the
// `FileSystemDirectoryHandle`s — a picked folder, or a directory the harness made
// in the origin-private file system, which is the same API — and answers the
// worker's requests into the shared answer buffer.
//
// The operation codes, the answer envelope and the namespace arithmetic are
// `protocol.js`, shared with the worker's half; their authority is
// `crates/lrsalvage-core/src/fs/bridge.rs` and `src/fs/mounts.rs`.

import {
  CTRL_WORDS,
  KIND_EXISTS,
  KIND_FULL,
  KIND_INVALID,
  KIND_NOT_FOUND,
  KIND_OK,
  KIND_OTHER,
  KIND_PERMISSION,
  KIND_UNSUPPORTED,
  OP_CANONICALIZE,
  OP_CREATE_NEW,
  OP_LIST,
  OP_MKDIR,
  OP_READ,
  OP_REMOVE,
  OP_STAT,
  OP_WALK,
  OP_WRITE,
  OP_WRITE_ATOMIC,
  PAYLOAD_OFFSET,
  deepestFirst,
  mountFor,
  normalise,
  segmentsOf,
} from './protocol.js';

/** A DOMException's name, in the seam's vocabulary. */
function kindOf(error) {
  switch (error && error.name) {
    case 'NotFoundError':
      return KIND_NOT_FOUND;
    case 'NotAllowedError':
    case 'SecurityError':
    case 'NoModificationAllowedError':
      return KIND_PERMISSION;
    case 'InvalidModificationError':
    case 'InvalidStateError':
    case 'TypeMismatchError':
    case 'TypeError':
    case 'SyntaxError':
      return KIND_INVALID;
    case 'QuotaExceededError':
      return KIND_FULL;
    case 'NotSupportedError':
      return KIND_UNSUPPORTED;
    default:
      return KIND_OTHER;
  }
}

export class FsHost {
  /**
   * @param {SharedArrayBuffer} sab the answer buffer shared with the worker
   * @param {Array<{path: string, handle: FileSystemDirectoryHandle}>} mounts
   */
  constructor(sab, mounts) {
    this.ctrl = new Int32Array(sab, 0, CTRL_WORDS);
    this.payload = new Uint8Array(sab, PAYLOAD_OFFSET);
    this.mounts = deepestFirst(mounts);
    // A listing is walked once and paged out of this: a cache tree is tens of
    // thousands of files and re-walking it per page would be quadratic.
    this.listings = new Map();
  }

  /** Invalidate every cached listing: one write can change all of them. */
  invalidate() {
    this.listings.clear();
  }

  resolve(path) {
    const wanted = normalise(path);
    const mount = mountFor(this.mounts, path);
    if (mount === null) return null;
    const rest = wanted.slice(mount.path === '/' ? 0 : mount.path.length);
    return { mount, rest, segments: segmentsOf(rest), path: wanted };
  }

  async serve(message) {
    const request = message.request;
    let kind = KIND_OK;
    let body = new Uint8Array(0);
    try {
      const answer = await this.answer(message.op, request);
      kind = answer.kind ?? KIND_OK;
      body =
        answer.bytes instanceof Uint8Array
          ? answer.bytes
          : new TextEncoder().encode(answer.text ?? '');
    } catch (error) {
      kind = kindOf(error);
      const message = error && error.message ? error.message : String(error);
      body = new TextEncoder().encode(`${request?.path ?? ''}: ${message}`);
    }
    // A refused operation is the interesting one: it is what a caller reads as
    // absence, and what the seam's `exists` exists to stop it doing. Logged so a
    // failed parity run says which operation and which path, not just that one
    // check came out wrong.
    if (kind !== KIND_OK) {
      const text = new TextDecoder().decode(body.subarray(0, 200));
      console.log(`[fs-host] op=${message.op} path=${request?.path} kind=${kind} ${text}`);
    }
    const total = Math.min(1 + body.length, this.payload.length);
    this.payload[0] = kind;
    this.payload.set(body.subarray(0, total - 1), 1);
    Atomics.store(this.ctrl, 1, total);
    Atomics.store(this.ctrl, 0, 1);
    Atomics.notify(this.ctrl, 0);
  }

  async answer(op, request) {
    const resolved = this.resolve(request.path);
    if (resolved === null) {
      return { kind: KIND_NOT_FOUND, text: `no mount answers for ${request.path}` };
    }
    const { mount, segments } = resolved;

    switch (op) {
      case OP_READ: {
        const handle = await this.file(mount.handle, segments);
        const file = await handle.getFile();
        const offset = Number(request.offset ?? 0);
        const length = Math.min(Number(request.len ?? 0), Math.max(0, file.size - offset));
        const blob = file.slice(offset, offset + length);
        return { bytes: new Uint8Array(await blob.arrayBuffer()) };
      }
      case OP_STAT:
        return { text: JSON.stringify(await this.stat(mount, segments)) };
      case OP_LIST: {
        const names = await this.enumerate(mount, resolved.path, false);
        const from = Number(request.from ?? 0);
        const count = Number(request.count ?? names.length);
        const page = names.slice(from, from + count);
        return { text: JSON.stringify({ names: page, done: from + page.length >= names.length }) };
      }
      case OP_WALK: {
        const names = await this.enumerate(mount, resolved.path, true);
        const from = Number(request.from ?? 0);
        const count = Number(request.count ?? names.length);
        const page = names.slice(from, from + count);
        return { text: JSON.stringify({ names: page, done: from + page.length >= names.length }) };
      }
      case OP_MKDIR: {
        this.invalidate();
        await this.directory(mount.handle, segments, true);
        return {};
      }
      case OP_WRITE:
      case OP_WRITE_ATOMIC: {
        this.invalidate();
        const bytes = Uint8Array.from(atob(request.bytes ?? ''), (character) =>
          character.charCodeAt(0),
        );
        const handle = await this.file(mount.handle, segments, true);
        // One `createWritable` … `close` per file: the platform's own atomic
        // commit, which is why the seam's `write_atomic` needs no staging here.
        const stream = await handle.createWritable();
        await stream.write(bytes);
        await stream.close();
        return {};
      }
      case OP_CREATE_NEW: {
        this.invalidate();
        try {
          await this.file(mount.handle, segments, false);
          return { kind: KIND_EXISTS, text: `${request.path} is already there` };
        } catch (error) {
          if (kindOf(error) !== KIND_NOT_FOUND) throw error;
        }
        const handle = await this.file(mount.handle, segments, true);
        const stream = await handle.createWritable();
        await stream.close();
        return {};
      }
      case OP_REMOVE: {
        this.invalidate();
        const parent = await this.directory(mount.handle, segments.slice(0, -1), false);
        await parent.removeEntry(segments[segments.length - 1]);
        return {};
      }
      case OP_CANONICALIZE: {
        // This namespace has no symlinks, so the normalised spelling is the
        // whole of the promise; a path nothing answers for is not resolvable.
        if (!(await this.stat(mount, segments)).exists) {
          return { kind: KIND_NOT_FOUND, text: `${request.path} is not there` };
        }
        return { text: JSON.stringify({ path: normalise(request.path) }) };
      }
      default:
        return { kind: KIND_UNSUPPORTED, text: `no such operation: ${op}` };
    }
  }

  /** Whether a path is a file, a directory or nothing, and a file's length. */
  async stat(mount, segments) {
    try {
      const handle = await this.file(mount.handle, segments);
      const file = await handle.getFile();
      return { exists: true, file: true, len: file.size };
    } catch (error) {
      // A directory at this path makes `getFileHandle` throw a type mismatch,
      // which is an answer — "not a file" — and not a failure.
      if (![KIND_NOT_FOUND, KIND_INVALID].includes(kindOf(error))) throw error;
    }
    try {
      await this.directory(mount.handle, segments, false);
      return { exists: true, file: false, len: 0 };
    } catch (error) {
      if (kindOf(error) !== KIND_NOT_FOUND) throw error;
    }
    return { exists: false, file: false, len: 0 };
  }

  async directory(root, segments, create) {
    let handle = root;
    for (const segment of segments) {
      handle = await handle.getDirectoryHandle(segment, { create });
    }
    return handle;
  }

  async file(root, segments, create = false) {
    if (segments.length === 0) {
      throw Object.assign(new Error('a file path with no name'), { name: 'NotFoundError' });
    }
    const parent = await this.directory(root, segments.slice(0, -1), create);
    return parent.getFileHandle(segments[segments.length - 1], { create });
  }

  /** Every entry under a mount path, relative to it: one level or recursive. */
  async enumerate(mount, path, recursive) {
    const key = `${recursive ? 'walk' : 'list'}:${path}`;
    const cached = this.listings.get(key);
    if (cached !== undefined) return cached;

    const rest = segmentsOf(path.slice(mount.path === '/' ? 0 : mount.path.length));
    const start = await this.directory(mount.handle, rest, false);
    const names = [];
    const walk = async (directory, prefix) => {
      for await (const [name, handle] of directory.entries()) {
        if (handle.kind === 'file') {
          names.push(prefix + name);
        } else if (recursive) {
          await walk(handle, `${prefix}${name}/`);
        }
      }
    };
    await walk(start, '');
    names.sort();
    this.listings.set(key, names);
    return names;
  }
}
