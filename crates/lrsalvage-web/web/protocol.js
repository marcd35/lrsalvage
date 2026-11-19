// The seam's browser protocol, as JavaScript: one home for the codes both halves
// switch on.
//
// `crates/lrsalvage-core/src/fs/bridge.rs` is the authority — the operation codes,
// the JSON request and the `[kind][bytes]` answer are its contract, and this file
// is the JS side of the same document. Both halves of the browser bridge import it
// (`lrsalvage-fs.js` in the worker, `fs-host.js` on the page), which is the point:
// a code added there and answered in one half and not the other is the kind of
// drift a second copy invites and nothing catches.
//
// The namespace arithmetic — `normalise`, `under` — lives here for the same
// reason. `crates/lrsalvage-core/src/fs/mounts.rs` is its Rust twin; the two are
// the same rule over the same paths, one per side of the boundary.

// Operation codes. `bridge.rs`'s `OP_*` are the other list, and the two are one
// contract: `bridge.rs` names the answer codes each operation may return.
export const OP_READ = 0;
export const OP_STAT = 1;
export const OP_LIST = 2;
export const OP_WALK = 3;
export const OP_MKDIR = 4;
export const OP_WRITE = 5;
export const OP_CREATE_NEW = 6;
export const OP_WRITE_ATOMIC = 7;
export const OP_REMOVE = 8;
export const OP_CANONICALIZE = 9;

// Answer codes, the first byte of every answer (`bridge.rs`'s `KIND_*`).
export const KIND_OK = 0;
export const KIND_NOT_FOUND = 1;
export const KIND_PERMISSION = 2;
export const KIND_EXISTS = 3;
export const KIND_UNSUPPORTED = 4;
export const KIND_INVALID = 5;
export const KIND_FULL = 6;
export const KIND_OTHER = 7;

/** Where the answer's control words sit in the shared buffer. */
export const CTRL_WORDS = 2;
export const PAYLOAD_OFFSET = CTRL_WORDS * 4;

/** A path's components, without the empties a leading or doubled `/` leaves. */
export function segmentsOf(path) {
  return path.split('/').filter((part) => part !== '' && part !== '.');
}

/** `/a/b/../../c` → `/c`, on whole components. */
export function normalise(path) {
  const parts = [];
  for (const part of path.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') parts.pop();
    else parts.push(part);
  }
  return '/' + parts.join('/');
}

/** Whether `path` is at or under `root`, on a component boundary. */
export function under(path, root) {
  if (root === '/') return path.startsWith('/');
  return path === root || path.startsWith(root + '/');
}

/**
 * The mount that answers for `path`: the longest registered root containing it,
 * or `null` when nothing does.
 */
export function mountFor(mounts, path) {
  const wanted = normalise(path);
  return mounts.find((mount) => under(wanted, mount.path)) ?? null;
}

/** Order a mount list so the first prefix that matches is the most specific. */
export function deepestFirst(mounts) {
  return mounts
    .map((mount) => ({ ...mount, path: normalise(mount.path) }))
    .sort((left, right) => right.path.split('/').length - left.path.split('/').length);
}
