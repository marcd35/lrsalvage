// A vault from the run's own directory, into origin-private storage.
//
// Both browser pages need this and neither owns it: the parity harness copies the
// vault it is about to compare, and the app's run copies the vault a person would
// have picked. The copies are made from the files served over HTTP, so the
// browser reads the same bytes the native run read rather than a second building
// of the same composition.
//
// A page only gets here because it is the side with an event loop: the worker is
// parked on the seam's `SharedArrayBuffer`, so every `await` in this file happens
// while nothing else in this page is running.

/** Fetch `<path>` into `dir`, creating the folders its name needs. */
export async function writeFile(dir, relative, bytes) {
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

/** A directory that does not hold the last run's files. */
export async function freshDirectory(base, name) {
  try {
    await base.removeEntry(name, { recursive: true });
  } catch (error) {
    if (error.name !== 'NotFoundError') throw error;
  }
  return base.getDirectoryHandle(name, { create: true });
}

/**
 * Copy `files` from `<url>/<file>` into a fresh directory called `name`.
 *
 * Returns the directory handle, which is what a page mounts — the same kind of
 * value a folder picker returns, which is why a test can stand in for one.
 */
export async function mountVault(base, name, url, files, log = () => {}) {
  const began = performance.now();
  const dir = await freshDirectory(base, name);
  for (const relative of files) {
    const path = `${url}/${relative.split('/').map(encodeURIComponent).join('/')}`;
    const response = await fetch(path);
    if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
    await writeFile(dir, relative, new Uint8Array(await response.arrayBuffer()));
  }
  log(`mounted ${name}: ${files.length} files in ${Math.round(performance.now() - began)} ms`);
  return dir;
}

/** A JSON file the run's own directory holds. */
export async function json(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response.json();
}
