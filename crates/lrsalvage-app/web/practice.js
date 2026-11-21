import { json, mountVault } from './mount.js';

/** Return the same directory-handle type as a successful source-folder pick. */
export async function loadPractice(name) {
  const manifest = await json(new URL('./practice/samples.json', import.meta.url));
  if (!Object.hasOwn(manifest, name)) throw new Error('That practice sample does not exist.');
  const { files, revision } = manifest[name];
  const validPath = (path) => path.split('/').every(
    (part) => part && part !== '.' && part !== '..' && !/[\\]/.test(part),
  );
  if (!/^[a-f0-9]{64}$/.test(revision) || !files.every(validPath)) {
    throw new Error('The practice sample manifest is invalid.');
  }
  const root = await navigator.storage.getDirectory();
  const practice = await root.getDirectoryHandle('lrsalvage-practice', { create: true });
  const key = `${name}-${revision}`;
  const url = new URL(`./practice/${encodeURIComponent(name)}`, import.meta.url);

  // The source is read-only once installed. Keep each byte revision separate so
  // an open recovery can keep using its handle when another tab opens a sample.
  return navigator.locks.request(`lrsalvage-practice:${key}`, async () => {
    try {
      const folder = await practice.getDirectoryHandle(key);
      await folder.getFileHandle('.ready');
      return folder;
    } catch (error) {
      if (error.name !== 'NotFoundError') throw error;
    }
    const folder = await mountVault(practice, key, url, files);
    await folder.getFileHandle('.ready', { create: true });
    return folder;
  });
}
