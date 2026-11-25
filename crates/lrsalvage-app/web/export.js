import { pickFolder } from './app.js';
import { readDestination, rememberDestination } from './destination.js';
import { commitFile } from './seam/commit-file.js';
import { log } from './diagnostics.js';
import { formatBytes } from './size.js';
import { mountReceipt } from './receipt.js';
import { CALCULATING, approximate, createRate } from './rate.js';

const MAX_WRITES = 8;

/** How many planned paths the dialog renders at once (`#192`). */
const PATH_CAP = 200;

/**
 * One word for why a write did not land, from the error a browser handed us.
 *
 * The categories are `specs/app.md`'s frozen list and they are aimed at a person:
 * "it ran out of room" and "the folder went away" lead to different next steps,
 * while the platform's seven filesystem kinds do not. The message itself never
 * leaves the page (`web/diagnostics.js`).
 */
function classify(error) {
  if (error instanceof OccupiedPath) return 'occupied_foreign';
  switch (error?.name) {
    case 'NotFoundError': return 'destination_gone';
    case 'NotAllowedError':
    case 'SecurityError':
    case 'NoModificationAllowedError': return 'permission';
    case 'QuotaExceededError': return 'out_of_room';
    case 'TypeMismatchError': return 'occupied_foreign';
    case 'InvalidStateError':
    case 'NotSupportedError': return 'write_failed';
    default: break;
  }
  // A preview the core could not prepare reads as text rather than as a
  // `DOMException`; that is the source going away, not the destination.
  return /unavailable|changed|could no longer be read/i.test(error?.message ?? '')
    ? 'source_unreadable'
    : 'other';
}

/** What a report file is for, in the receipt's list. */
const DESCRIPTIONS = {
  'report.csv': 'Photos and scan outcomes',
  'extract.csv': 'Save outcomes',
  'manifest.json': 'Recovery data for tools',
  'verification.json': 'Path-free scan facts',
};

async function validateDestination(destination, inputs) {
  for (const { path, handle } of inputs) {
    if (await handle.resolve(destination) !== null || await destination.resolve(handle) !== null) {
      throw new Overlap(
        `That folder is inside ${treeName(path)}. Choose a folder outside the catalog, ` +
          'the preview cache and the original folders.',
      );
    }
  }
}

class OccupiedPath extends Error {
  constructor() {
    super('A file already exists at this path. It was left unchanged.');
  }
}

/**
 * The destination landed inside the source, and the way out has its own place.
 *
 * `#191`: this refusal used to be one sentence in the status line, which left the
 * user to find the destination control themselves. It carries the tree it
 * landed in so the panel can say which one, and the panel renders it as a block
 * with the button that reopens the picker (`refuse`).
 */
class Overlap extends Error {}

class PendingConflict extends Error {
  constructor(path, snapshot) {
    super('The destination now contains a file at this path.');
    this.path = path;
    this.snapshot = snapshot;
  }
}

/** The tree a mount path belongs to, in the words the refusal uses. */
function treeName(path) {
  if (path === '/source') return "the catalog's folder";
  if (path === '/cache') return 'the preview cache folder';
  return 'a folder the catalog records as an original-photo location';
}

async function fileAt(directory, name) {
  try {
    return await directory.getFileHandle(name);
  } catch (error) {
    if (error.name === 'TypeMismatchError') throw new OccupiedPath();
    if (error.name !== 'NotFoundError') throw error;
    return null;
  }
}

function pathParts(relative) {
  const parts = relative.split('/');
  if (parts.some((part) => !part || part === '.' || part === '..' || part.includes('\\'))) {
    throw new Error('The planned path is outside the destination.');
  }
  return parts;
}

async function entryAt(parent, name) {
  try {
    return await fileAt(parent, name);
  } catch (error) {
    if (!(error instanceof OccupiedPath)) throw error;
    return parent.getDirectoryHandle(name);
  }
}

async function snapshot(entry) {
  if (entry.kind === 'directory') return { kind: entry.kind };
  const bytes = new Uint8Array(await (await entry.getFile()).arrayBuffer());
  return { kind: entry.kind, digest: await sha256(bytes), bytes };
}

async function probe(root, relative) {
  const parts = pathParts(relative);
  const name = parts.pop();
  try {
    let parent = root;
    for (const part of parts) parent = await parent.getDirectoryHandle(part);
    const entry = await entryAt(parent, name);
    if (!entry) return null;
    const { kind, digest } = await snapshot(entry);
    return { kind, digest };
  } catch (error) {
    if (error.name === 'NotFoundError' || error.name === 'TypeMismatchError') return null;
    throw error;
  }
}

function numberedPath(path, number) {
  const dot = path.lastIndexOf('.');
  return path.slice(0, dot) + ' (' + number + ')' + path.slice(dot);
}

async function writeNew(root, relative, prepare, inspectExisting) {
  const parts = pathParts(relative);
  const name = parts.pop();
  // Tabs of this app share the check and commit for the same relative name.
  return navigator.locks.request('lrsalvage-save:' + relative.toLowerCase(), async () => {
    let parent = root;
    for (const part of parts) {
      parent = await parent.getDirectoryHandle(part, { create: true });
    }
    const occupied = async (handle) => {
      const outcome = await inspectExisting?.(handle);
      if (!outcome) throw new OccupiedPath();
      return outcome;
    };
    const existing = await entryAt(parent, name);
    if (existing) return occupied(existing);
    const { bytes, outcome } = await prepare();
    const appeared = await entryAt(parent, name);
    if (appeared) return occupied(appeared);

    const handle = await parent.getFileHandle(name, { create: true });
    const empty = await handle.getFile();
    if (empty.size !== 0) return occupied(handle);
    const unchanged = async (candidate) => {
      const file = await candidate.getFile();
      return file.size === 0 && file.lastModified === empty.lastModified;
    };
    try {
      await commitFile(handle, bytes, async () => {
        if (!await unchanged(handle)) throw new OccupiedPath();
      });
    } catch (error) {
      // A failed stream may leave the empty entry created before it opened.
      // Only clean up an entry that still matches the handle and empty file.
      try {
        const current = await fileAt(parent, name);
        if (current && await current.isSameEntry(handle) && await unchanged(current)) {
          await parent.removeEntry(name);
        }
      } catch {
        // A disconnected folder may be unreachable even for cleanup.
      }
      throw error;
    }
    return outcome;
  });
}

async function writeReport(destination, name, text) {
  const bytes = new TextEncoder().encode(text);
  for (let number = 1; ; number++) {
    const path = number === 1 ? name : numberedPath(name, number);
    try {
      await writeNew(destination, path, () => ({ bytes }));
      return path;
    } catch (error) {
      if (!(error instanceof OccupiedPath)) throw error;
    }
  }
}

/**
 * Write one report and read it back, so the receipt links the file that exists.
 *
 * The link is the bytes the destination holds, opened as a blob: a page has no
 * other way to open a file it wrote, and reading it back is also the check that
 * the file is really there. The name it landed on comes back from the writer, so
 * the receipt never re-derives the CLI's numbered-name rule.
 */
async function writeAndLink(destination, name, text, description) {
  const path = await writeReport(destination, name, text);
  const file = await (await destination.getFileHandle(path)).getFile();
  return { name: path, url: URL.createObjectURL(file), description };
}

async function sha256(bytes) {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(hash, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/**
 * What the destination answers when it is listed back.
 *
 * One walk, intersected with the run's committed paths: `present` is what the
 * folder really holds, with the size it holds now, and `missing` is the plan
 * paths it no longer has. A folder that cannot be read says so instead of
 * letting the run's own record pass for an observation (`specs/app.md`,
 * *Receipt, reports and diagnostics*).
 */
async function reconcile(destination, ledger) {
  const entries = [...ledger.values()];
  const planned = new Map(entries.map((entry) => [entry.path.toLowerCase(), entry]));
  const present = new Map();
  const changed = [];
  try {
    const walk = async (dir, prefix) => {
      for await (const [name, entry] of dir.entries()) {
        const path = prefix + name;
        if (entry.kind === 'directory') {
          await walk(entry, path + '/');
          continue;
        }
        const found = planned.get(path.toLowerCase());
        if (!found) continue;
        const file = await entry.getFile();
        if (file.size !== found.bytes || await sha256(await file.arrayBuffer()) !== found.digest) {
          changed.push(found.path);
          continue;
        }
        present.set(path.toLowerCase(), {
          id: found.id, path: found.path, bytes: file.size, status: found.status, destination: destination.name,
        });
      }
    };
    await walk(destination, '');
  } catch (error) {
    return { ok: false, reason: classify(error), present: [], missing: [] };
  }
  const changedNames = new Set(changed);
  const missing = entries
    .filter((entry) => !present.has(entry.path.toLowerCase()) && !changedNames.has(entry.path))
    .map((entry) => entry.path);
  return { ok: true, reason: null, present: [...present.values()], missing, changed };
}

function savedFiles(current) {
  return current.targets.flatMap((target) => [...target.files.values()]);
}

async function reconcileReceipt(current) {
  const settled = { ok: true, present: [], missing: [], changed: [] };
  for (const target of current.targets) {
    if (!target.files.size) continue;
    const result = await reconcile(target.handle, target.files);
    if (!result.ok) return result;
    settled.present = settled.present.concat(result.present);
    settled.missing = settled.missing.concat(result.missing);
    settled.changed = settled.changed.concat(result.changed);
  }
  return settled;
}

export function mountExport(document) {
  const el = (name) => document.getElementById('export-' + name);
  const section = el('section');
  const layout = el('layout');
  const picker = el('pick');
  const suggestion = el('suggestion');
  const start = el('start');
  const stop = el('stop');
  const end = el('end');
  const save = el('one');
  const refusal = el('refusal');
  const refusalMessage = el('refusal-message');
  const refusalPick = el('refusal-pick');
  const saveBar = el('save-bar');
  const saveBarCount = el('save-bar-count');
  const saveBarDestination = el('save-bar-destination');
  const saveBarSave = el('save-bar-save');
  const pathsOpen = el('paths-open');
  const pathsSummary = el('paths-summary');
  const pathsDialog = el('paths-dialog');
  const conflictsDialog = el('conflict');
  const pathsNote = el('paths-note');
  const pathsSearch = el('paths-search');
  const pathsShown = el('paths-shown');
  const bar = el('bar');
  const progress = el('progress');
  const announceNode = el('announce');
  /** The batch's estimate, its node, and the one milestone already announced. */
  const rate = createRate();
  let milestone = 0;
  const receipt = mountReceipt(document, { saveVerification });
  let run;

  function open(scan, inputs, mode = 'catalog') {
    clear();
    // No catalog means no original folders to mirror: the two cache modes share
    // that, and differ in the mode word a report or a receipt carries.
    const cacheOnly = mode !== 'catalog';
    run = {
      scan, inputs, mode, cacheOnly, checked: new Set(), layouts: new Map(), targets: [],
      flat: false, target: null, suggestion: null, batch: null, busy: false, photo: null,
      // The catalog's own folder, so the destination picker can start there: the
      // recovery folder belongs beside the source, and the page still holds the
      // handle it was picked with (`#191`).
      catalog: inputs.find((mount) => mount.path === '/source')?.handle ?? null,
      // The run's account: one entry per catalog image this run committed, plus
      // the failures and the reports it wrote. `targets[].saved` caches per
      // destination for the writer; this is what the receipt counts.
      ledger: new Map(), failures: [], reportFailures: [], reports: [],
      categorised: new Set(), reconciled: null, batchSummary: null, omitted: new Map(),
    };
    section.hidden = false;
    el('layout-control').hidden = cacheOnly;
    el('metadata-total').hidden = cacheOnly;
    layout.value = cacheOnly ? 'flat' : 'mirrored';
    void loadLayout(run, cacheOnly);
    const current = run;
    void readDestination().then((handle) => {
      if (run === current && !current.target && !current.suggestion && handle) {
        current.suggestion = handle;
        render();
      }
    }).catch(() => {});
  }

  function clear() {
    if (run?.batch) run.batch.stopping = true;
    run = null;
    section.hidden = true;
    saveBar.hidden = true;
    clearRefusal();
    resetProgress();
    if (pathsDialog.open) pathsDialog.close();
    if (conflictsDialog.open) conflictsDialog.close('cancel');
    el('status').textContent = '';
    el('errors').replaceChildren();
    el('one-status').textContent = '';
    receipt.clear();
  }

  /** Show the run's account, and what the destination last answered for it. */
  function updateReceipt(current) {
    if (run !== current) return;
    receipt.update({
      ledger: current.ledger,
      files: savedFiles(current),
      cacheOnly: current.cacheOnly,
      failures: current.failures,
      omitted: current.omitted,
      batch: current.batchSummary,
      reportFailures: current.reportFailures,
      reports: current.reports,
      reconciled: current.reconciled,
      destinationNames: [...new Set([...current.ledger.values()].map((entry) => entry.target.handle.name))],
    });
    void receipt.renderVersions();
  }

  async function loadLayout(current, flat) {
    current.flat = flat;
    render();
    if (current.layouts.has(flat)) return;
    try {
      const rows = await current.scan.exportPaths(flat);
      current.layouts.set(flat, new Map(rows.map((row) => [row.id, row])));
    } catch (error) {
      if (run === current && current.flat === flat) el('status').textContent = error.message;
    }
    if (run === current) render();
  }

  function select(checked) {
    if (!run) return;
    run.checked = new Set(checked);
    render();
  }

  function photo(id) {
    if (!run) return;
    run.photo = id;
    el('one-status').textContent = '';
    renderOne();
  }

  function selected(current) {
    const rows = current.layouts.get(current.flat);
    return rows ? Array.from(rows.values()).filter(({ id }) => current.checked.has(id)) : [];
  }

  function renderOne() {
    const row = run.layouts.get(run.flat)?.get(run.photo);
    el('one-path').textContent = row?.path ?? '';
    save.disabled = run.busy || Boolean(run.batch) || !row;
  }

  function render() {
    if (!run) return;
    const rows = run.batch?.rows ?? selected(run);
    const bytes = rows.reduce((sum, row) => sum + row.bytes, 0);
    el('total').textContent = `${rows.length} JPEGs · ${formatBytes(bytes)} of previews`;
    el('destination').textContent = run.target ? 'Destination · ' + run.target.handle.name : '';
    layout.disabled = run.busy || Boolean(run.batch);
    picker.disabled = run.busy || Boolean(run.batch);
    suggestion.hidden = !run.suggestion || run.suggestion === run.target?.handle;
    suggestion.disabled = picker.disabled;
    if (!suggestion.hidden) suggestion.textContent = 'Use ' + run.suggestion.name;
    start.disabled = run.busy || !run.target || rows.length === 0;
    start.textContent = run.batch ? 'Continue saving' : 'Save selected JPEGs';
    stop.hidden = !run.busy || !run.batch;
    stop.disabled = !run.batch || run.batch.stopping || run.batch.reporting;
    end.hidden = !run.batch || run.busy;
    // The same state, above the gallery: one destination, one count, one start
    // and stop (`#192`). Nothing here decides anything the panel below does not.
    saveBar.hidden = false;
    saveBarCount.textContent = `${rows.length} selected`;
    saveBarDestination.textContent = run.target
      ? 'Destination · ' + run.target.handle.name
      : 'No destination chosen yet';
    saveBarSave.disabled = start.disabled;
    saveBarSave.textContent = start.textContent;
    pathsSummary.textContent = rows.length
      ? `${rows.length} paths planned · ${formatBytes(bytes)}`
      : '';
    pathsOpen.disabled = rows.length === 0;
    renderOne();
    if (pathsDialog.open) {
      pathsNote.textContent = pathsNoteFor(run, rows);
      renderPaths(rows);
    }
  }

  /**
   * A short account of what the plan decided, for the dialog's head.
   *
   * The collision count is derived from the names rather than reported by the
   * planner: a path whose name ends in ` (n)` was numbered, either because two
   * photos wanted the same name or because the photo's own name already carried
   * that shape. The sentence says "numbered to avoid a clash" rather than "renamed
   * by the app", which is the part that stays true either way.
   */
  function pathsNoteFor(current, rows) {
    const numbered = rows.filter(({ path }) => / \(\d+\)(\.[^./]+)?$/.test(path)).length;
    const where = current.target ? current.target.handle.name : 'no destination yet';
    const layout = current.flat ? 'all in one folder' : 'original folders mirrored';
    return `${rows.length} planned paths, ${layout}, into ${where}` +
      (numbered ? `, ${numbered} of them numbered to avoid a clash` : '');
  }

  function renderPaths(rows) {
    const list = el('path-list');
    list.replaceChildren();
    const needle = pathsSearch.value.trim().toLowerCase();
    const matches = needle ? rows.filter(({ path }) => path.toLowerCase().includes(needle)) : rows;
    for (const { path } of matches.slice(0, PATH_CAP)) {
      const item = document.createElement('li');
      item.textContent = path;
      list.append(item);
    }
    pathsShown.textContent = matches.length > PATH_CAP
      ? `Showing the first ${PATH_CAP} of ${matches.length} matching paths.`
      : `${matches.length} matching ${matches.length === 1 ? 'path' : 'paths'}.`;
  }

  function openPaths(current) {
    const rows = current.batch?.rows ?? selected(current);
    pathsSearch.value = '';
    pathsNote.textContent = pathsNoteFor(current, rows);
    renderPaths(rows);
    pathsDialog.showModal();
    pathsSearch.focus();
  }

  /**
   * The batch's readout, from zero: the bar, the sentence and the milestone.
   *
   * The arithmetic is `rate.js`'s, the module the scanning readout uses, so a
   * second estimate with its own warm-up cannot drift from the first (`#192`).
   */
  function resetProgress(total = 0) {
    rate.reset();
    milestone = 0;
    bar.hidden = total === 0;
    bar.removeAttribute('value');
    bar.max = total || 1;
    progress.hidden = total === 0;
    progress.textContent = '';
    announceNode.textContent = '';
  }

  /**
   * One paint of the batch's progress: a percentage from completed writes, and an
   * estimate from observed throughput.
   *
   * The one live region announces each quarter, not each write: a 2,659-file
   * batch crosses the boundary thousands of times and a screen reader has to hear
   * about four of them.
   */
  function paintProgress(total, done, stopping) {
    if (total === 0) return;
    bar.max = total;
    bar.value = done;
    const percent = done === total ? 100 : Math.min(99, Math.floor((100 * done) / total));
    const seconds = rate.add(done, total, performance.now());
    const eta = seconds === null ? CALCULATING : approximate(seconds);
    progress.hidden = false;
    const counts = `${done} of ${total} written`;
    progress.textContent = `${percent}% · ${counts} · ${stopping ? 'stopping' : eta}`;
    const quarter = Math.floor(percent / 25);
    if (quarter > milestone) {
      milestone = quarter;
      announce(`${percent}% — ${counts}${done === total ? ' · finished' : ''}`);
    }
  }

  function announce(text) {
    // A repeat is not a change: writing the same string again is a DOM mutation a
    // screen reader has to decide about.
    if (announceNode.textContent !== text) announceNode.textContent = text;
  }

  async function choose(current) {
    const picked = await pickFolder({
      id: 'lrsalvage-destination',
      mode: 'readwrite',
      startIn: current.catalog,
    });
    if (picked.cancelled) {
      // One of the two ways a pick ends: the panel is where it was, and what the
      // user was doing is still the thing in front of them.
      focusAfterPick();
      return undefined;
    }
    if (picked.refused) throw new Error(picked.refused.message);
    return accept(current, picked.handle);
  }

  async function accept(current, handle) {
    await validateDestination(handle, current.inputs);
    clearRefusal();
    void rememberDestination(handle).catch(() => {});
    for (const target of current.targets) {
      if (await target.handle.isSameEntry(handle)) {
        target.handle = handle;
        return target;
      }
    }
    const target = { handle, saved: new Map(), files: new Map() };
    current.targets.push(target);
    return target;
  }

  async function locked(current, action, status) {
    if (current.busy) return;
    current.busy = true;
    render();
    try {
      await action();
    } catch (error) {
      if (run === current) {
        if (error instanceof Overlap) refuse(error.message);
        else status.textContent = error.message;
      }
    } finally {
      current.busy = false;
      if (run === current) {
        render();
        // The receipt is outside this module's panel, and it must learn that the
        // run is no longer busy as well as what the run committed.
        updateReceipt(current);
      }
    }
  }

  /**
   * Show the overlap refusal where the destination is, with the way out beside it.
   *
   * Focus moves into the block, to the button rather than the sentence: the
   * sentence is announced by its `role="alert"`, and a focused paragraph would
   * leave the only useful action one Tab away.
   */
  function refuse(message) {
    refusalMessage.textContent = message;
    refusal.hidden = false;
    refusalPick.focus();
  }

  function clearRefusal() {
    refusal.hidden = true;
    refusalMessage.textContent = '';
  }

  /**
   * Where focus goes when a pick ends without one.
   *
   * Cancelled: the refusal block is still the thing to answer, so its button.
   * Otherwise the destination control, which is what an accepted folder leaves the
   * user wanting to use.
   */
  function focusAfterPick() {
    if (run && !refusal.hidden) refusalPick.focus();
    else picker.focus();
  }

  /**
   * Record one catalog image this run saved, once.
   *
   * A photo a one-off and a batch both reached is one photo on the receipt: an
   * entry that already exists is never demoted to `already recovered`, because
   * this run did write it.
   */
  function promote(current, row, outcome, target) {
    const previous = current.ledger.get(row.id);
    const at = current.failures.findIndex((entry) => entry.path === row.path);
    if (at >= 0) current.failures.splice(at, 1);
    current.omitted.delete(row.path);
    const path = outcome.actual ?? row.path;
    const key = path.toLowerCase();
    const status = target.files.get(key)?.status === 'written' ? 'written' : outcome.status;
    const file = { id: row.id, path, status, bytes: outcome.bytes, digest: outcome.digest, target };
    target.files.set(key, file);
    if (previous && !(previous.status === 'skipped' && outcome.status === 'written')) return;
    if (!previous) log.commit(outcome.status);
    current.ledger.set(row.id, file);
  }

  async function askConflicts(conflicts, target, choices) {
    const count = conflicts.size;
    const ending = count === 1 ? 'filename already exists' : 'filenames already exist';
    const hasDirectory = [...conflicts.values()].some((item) => item.kind === 'directory');
    el('conflict-message').textContent = count + ' ' + ending + ' in “' + target.handle.name + '”.' +
      (hasDirectory ? ' A folder occupies a planned filename. Choose Skip or Keep both.' :
        ' Overwrite replaces the existing files.');
    const list = el('conflict-paths');
    list.replaceChildren();
    for (const path of [...conflicts.keys()].slice(0, PATH_CAP)) {
      const item = document.createElement('li');
      item.textContent = path;
      list.append(item);
    }
    if (count > PATH_CAP) {
      const item = document.createElement('li');
      item.textContent = 'And ' + (count - PATH_CAP) + ' more';
      list.append(item);
    }
    conflictsDialog.querySelector('[value="overwrite"]').disabled = hasDirectory;
    const answer = await new Promise((resolve) => {
      conflictsDialog.addEventListener('close', () => resolve(conflictsDialog.returnValue), { once: true });
      conflictsDialog.returnValue = 'cancel';
      conflictsDialog.showModal();
    });
    if (!['overwrite', 'skip', 'keep'].includes(answer)) return false;
    for (const [path, observed] of conflicts) choices.set(path, { ...observed, action: answer });
    return true;
  }

  async function findConflicts(target, rows, stopping) {
    const conflicts = new Map();
    for (let index = 0; index < rows.length && !stopping(); index += MAX_WRITES) {
      await Promise.all(rows.slice(index, index + MAX_WRITES).map(async ({ path }) => {
        const found = await probe(target.handle, path);
        if (found) conflicts.set(path, found);
      }));
    }
    return conflicts;
  }

  async function write(current, target, flat, row, choices, reserved) {
    const prepare = () => current.scan.exportJpeg(row.id, flat);
    let actual = row.path;
    let observedDigest;
    let keepBoth = false;
    try {
      let outcome = await writeNew(target.handle, row.path, prepare, async (handle) => {
        const found = await snapshot(handle);
        const { kind, digest } = found;
        const approved = choices.get(row.path);
        if (approved?.kind !== kind || approved.digest !== digest) {
          throw new PendingConflict(row.path, { kind, digest });
        }
        if (approved.action === 'keep') {
          keepBoth = true;
          return null;
        }
        if (approved.action === 'overwrite' && kind === 'file') {
          const prepared = await prepare();
          await commitFile(handle, prepared.bytes, async () => {
            const latest = await probe(target.handle, row.path);
            if (latest?.kind !== kind || latest.digest !== digest) {
              if (latest) throw new PendingConflict(row.path, latest);
              throw new OccupiedPath();
            }
          });
          return prepared.outcome;
        }
        const previous = target.saved.get(row.path.toLowerCase());
        if (kind === 'file' && (previous?.id !== row.id || previous.digest === digest)) {
          const verified = await current.scan.existingExport(row.id, flat, found.bytes);
          if (verified) {
            observedDigest = digest;
            return verified;
          }
        }
        return {
          status: 'omitted', bytes: 0, sha256: null, output: row.path,
          note: 'Skipped because the destination already holds an unverified file or folder.',
        };
      }).catch((error) => {
        if (keepBoth && error instanceof OccupiedPath) return null;
        throw error;
      });
      if (keepBoth) {
        // Skip names belonging to other selected previews, including those
        // that another writer has not committed yet.
        for (let number = 2; !outcome; number++) {
          actual = numberedPath(row.path, number);
          if (reserved.has(actual.toLowerCase())) continue;
          reserved.add(actual.toLowerCase());
          try {
            outcome = await writeNew(target.handle, actual, prepare);
          } catch (error) {
            if (!(error instanceof OccupiedPath)) throw error;
          }
        }
      }
      if (outcome.status === 'omitted') return outcome;
      const digest = outcome.sha256 ?? observedDigest;
      target.saved.set(actual.toLowerCase(), { id: row.id, digest });
      return { ...outcome, output: actual === row.path ? outcome.output : actual, digest, actual };
    } catch (error) {
      if (error instanceof PendingConflict) throw error;
      if (error instanceof OccupiedPath && actual === row.path) {
        const latest = await probe(target.handle, row.path);
        if (latest) throw new PendingConflict(row.path, latest);
      }
      return {
        status: 'failed', bytes: 0, sha256: null, output: row.path,
        note: error.message, category: classify(error),
      };
    }
  }

  function addError(path, message) {
    const entry = document.createElement('li');
    entry.textContent = path + ' · ' + message;
    el('errors').append(entry);
  }

  /**
   * Record one photo that will not be in the folder, once per path.
   *
   * The receipt lists it and the diagnostics count its category — never its
   * message, which can name a path — so the count and the list cannot disagree
   * about how many photos are missing.
   */
  function failPhoto(current, path, message, category) {
    const at = current.failures.findIndex((entry) => entry.path === path);
    const entry = { path, note: message, category };
    if (at >= 0) current.failures[at] = entry;
    else current.failures.push(entry);
    addError(path, message);
    if (!current.categorised.has(path)) {
      current.categorised.add(path);
      log.failure(category);
      log.commit('failed');
    }
  }

  /**
   * Record a report that could not be written.
   *
   * A report is file work and its category is counted, but it is not one of the
   * photos the receipt is about, so it gets its own list and its own sentence
   * rather than inflating "N could not be saved".
   */
  function failReport(current, name, message, category) {
    if (!current.reportFailures.some((entry) => entry.name === name)) {
      current.reportFailures.push({ name, note: message });
      log.failure(category);
    }
    addError(name, message);
  }

  async function saveBatch(current) {
    const started = performance.now();
    const target = current.target;
    await validateDestination(target.handle, current.inputs);
    if (run !== current) return;
    const continuing = Boolean(current.batch);
    const batch = current.batch ??= { rows: selected(current), outcomes: new Map(), elapsed: 0 };
    if (continuing) log.continued();
    else log.planned(batch.rows.length);
    batch.stopping = false;
    const choices = new Map();
    const totals = { written: 0, skipped: 0, omitted: 0, failed: 0 };
    // Continue can retry a write failure from the preceding attempt.
    for (const [id, outcome] of batch.outcomes) {
      if (outcome.status === 'failed') batch.outcomes.delete(id);
      else totals[outcome.status]++;
    }
    const remaining = () => batch.rows.filter(({ id }) => !batch.outcomes.has(id));
    let since = started;
    const chooseConflicts = async (conflicts) => {
      batch.elapsed += performance.now() - since;
      const accepted = await askConflicts(conflicts, target, choices);
      since = performance.now();
      return accepted;
    };
    render();
    let conflicts;
    try {
      conflicts = await findConflicts(target, remaining(), () => batch.stopping || run !== current);
    } catch (error) {
      if (!continuing) current.batch = null;
      throw error;
    }
    if (run !== current) return;
    if (!batch.stopping && conflicts.size && !await chooseConflicts(conflicts)) {
      if (!continuing) current.batch = null;
      return;
    }
    if (run !== current) return;

    const reserved = new Set(batch.rows.map(({ path }) => path.toLowerCase()));
    for (const outcome of batch.outcomes.values()) {
      if (outcome.actual) reserved.add(outcome.actual.toLowerCase());
    }
    el('errors').replaceChildren();
    resetProgress(batch.rows.length);
    const paint = () => {
      const done = Object.values(totals).reduce((sum, count) => sum + count, 0);
      el('status').textContent = totals.written + ' saved of ' + batch.rows.length +
        (totals.skipped ? ' · ' + totals.skipped + ' already recovered' : '') +
        (totals.omitted ? ' · ' + totals.omitted + ' skipped' : '') +
        (totals.failed ? ' · ' + totals.failed + ' failed' : '');
      paintProgress(batch.rows.length, done, batch.stopping);
    };
    paint();

    const record = (row, outcome) => {
      if (outcome.status === 'written' || outcome.status === 'skipped') {
        promote(current, row, outcome, target);
      } else if (outcome.status === 'omitted') {
        current.omitted.set(row.path, outcome.note);
      } else {
        failPhoto(current, row.path, outcome.note ?? 'The file could not be written.', outcome.category);
      }
    };
    while (!batch.stopping && run === current) {
      const pending = remaining();
      if (!pending.length) break;
      const late = new Map();
      let next = 0;
      await Promise.all(Array.from({ length: Math.min(MAX_WRITES, pending.length) }, async () => {
        while (!batch.stopping && !late.size && run === current && next < pending.length) {
          const row = pending[next++];
          let outcome;
          try {
            outcome = await write(current, target, current.flat, row, choices, reserved);
          } catch (error) {
            if (!(error instanceof PendingConflict)) throw error;
            late.set(error.path, error.snapshot);
            continue;
          }
          batch.outcomes.set(row.id, outcome);
          if (run !== current) return;
          totals[outcome.status]++;
          record(row, outcome);
          paint();
        }
      }));
      if (run !== current) return;
      if (late.size && !batch.stopping && !await chooseConflicts(late)) {
        batch.stopping = true;
        log.stopped();
      }
    }
    log.settled();
    if (run !== current) return;

    const planned = new Map(batch.rows.flatMap(({ id }) => {
      const outcome = batch.outcomes.get(id);
      if (!outcome || !['written', 'skipped'].includes(outcome.status)) return [];
      const { actual: path, bytes, digest, status } = outcome;
      return [[id, { id, path, bytes, digest, status }]];
    }));
    const confirmed = planned.size ? await reconcile(target.handle, planned) : null;
    const files = savedFiles(current);
    const sameFiles = files.length === planned.size && files.every((file) => {
      const entry = planned.get(file.id);
      return entry && file.target === target && file.path === entry.path && file.digest === entry.digest;
    });
    current.reconciled = confirmed && (sameFiles || !confirmed.ok)
      ? confirmed
      : await reconcileReceipt(current);
    log.confirmed(current.reconciled.ok ? new Set(current.reconciled.present.map(({ id }) => id)).size : null);
    if (run !== current) return;
    batch.elapsed += performance.now() - since;
    const summary = {
      total: batch.rows.length, verified: confirmed ? (confirmed.ok ? confirmed.present.length : null) : 0,
      written: totals.written, already: totals.skipped, omitted: totals.omitted,
      failed: totals.failed, renamed: batch.rows.filter(({ id, path }) =>
        batch.outcomes.get(id)?.actual && batch.outcomes.get(id).actual !== path).length,
      folder: target.handle.name, elapsed: batch.elapsed,
    };
    current.batchSummary = summary;
    if (remaining().length) {
      el('status').textContent += ' · Stopped';
      updateReceipt(current);
      return;
    }

    batch.reporting = true;
    render();
    try {
      const outcomes = batch.rows.map(({ id, path }) => {
        const { actual, digest, ...outcome } = batch.outcomes.get(id);
        return actual && actual !== path ? { ...outcome, planned: path } : outcome;
      });
      const reports = await current.scan.exportReports(current.flat, outcomes);
      const written = [];
      for (const [name, text] of Object.entries(reports)) {
        try {
          written.push(await writeAndLink(target.handle, name, text, DESCRIPTIONS[name] ?? 'Report'));
        } catch (error) {
          failReport(current, name, error.message, classify(error));
        }
      }
      current.reports = [
        ...written,
        ...current.reports.filter((report) => report.name.startsWith('verification')),
      ];
    } finally {
      current.batch = null;
    }
    if (run !== current) return;
    updateReceipt(current);
    receipt.focus();
    if (summary.written && summary.verified === summary.total) receipt.showCompletion(summary);
  }

  /** The batch's own account, as the report's `writes` member publishes it. */
  function writes() {
    const entries = [...run.ledger.values()];
    return {
      saved: entries.filter((entry) => entry.status === 'written').length,
      already_recovered: entries.filter((entry) => entry.status === 'skipped').length,
      failed: run.failures.length,
      omitted: run.omitted.size,
      confirmed_on_disk: run.reconciled?.ok ? new Set(run.reconciled.present.map(({ id }) => id)).size : null,
    };
  }

  /**
   * Write the path-free verification report on request, and list it with the rest.
   *
   * It is offered rather than written unasked because it is the artifact a person
   * chooses to share (#117, user story 6), and because the folder holds the
   * photos, not our paperwork.
   */
  async function saveVerification(status) {
    const current = run;
    if (!current || !current.target || current.busy) return;
    current.busy = true;
    render();
    status.textContent = 'Writing the report…';
    try {
      // The encoder writes the object, `writes` included: the page measures the
      // batch and hands the counts over, and never re-serialises a published
      // file (`specs/modules/report.md`).
      const { text } = await current.scan.verificationReport(writes());
      const entry = await writeAndLink(
        current.target.handle, 'verification.json', text, DESCRIPTIONS['verification.json']);
      for (const gone of current.reports.filter((report) => report.name.startsWith('verification'))) {
        URL.revokeObjectURL(gone.url);
      }
      current.reports = [
        ...current.reports.filter((report) => !report.name.startsWith('verification')),
        entry,
      ];
      status.textContent = `Saved ${entry.name} into the folder you chose.`;
    } catch (error) {
      status.textContent = error.message;
      log.failure(classify(error));
    } finally {
      current.busy = false;
      if (run === current) {
        render();
        updateReceipt(current);
      }
    }
  }

  pathsOpen.addEventListener('click', () => {
    if (run) openPaths(run);
  });
  pathsSearch.addEventListener('input', () => {
    if (run) renderPaths(run.batch?.rows ?? selected(run));
  });
  layout.addEventListener('change', () => {
    if (run && !run.busy && !run.batch) void loadLayout(run, layout.value === 'flat');
  });
  picker.addEventListener('click', () => {
    const current = run;
    const status = el('status');
    if (!current || current.batch) return;
    void locked(current, async () => {
      status.textContent = '';
      const target = await choose(current);
      if (target) current.target = target;
    }, status).then(focusAfterPick);
  });
  // The refusal's own way out (`#191`): the same pick, from the block that says
  // why the last one was refused.
  refusalPick.addEventListener('click', () => {
    const current = run;
    const status = el('status');
    if (!current || current.batch || current.busy) return;
    void locked(current, async () => {
      status.textContent = '';
      const target = await choose(current);
      if (target) current.target = target;
    }, status).then(focusAfterPick);
  });
  suggestion.addEventListener('click', () => {
    const current = run;
    const handle = current?.suggestion;
    if (!handle || current.batch) return;
    const status = el('status');
    void locked(current, async () => {
      status.textContent = '';
      if (await handle.requestPermission({ mode: 'readwrite' }) !== 'granted') {
        throw new Error('Permission was not granted. Choose a destination to continue.');
      }
      current.target = await accept(current, handle);
    }, status);
  });
  save.addEventListener('click', () => {
    const current = run;
    const row = current?.layouts.get(current.flat)?.get(current.photo);
    if (!row || current.batch) return;
    const status = el('one-status');
    void locked(current, async () => {
      status.textContent = '';
      const target = await choose(current);
      if (!target) return;
      const choices = new Map();
      const reserved = new Set([row.path.toLowerCase()]);
      const conflicts = await findConflicts(target, [row], () => run !== current);
      if (conflicts.size && !await askConflicts(conflicts, target, choices)) return;
      let outcome;
      while (run === current) {
        try {
          outcome = await write(current, target, current.flat, row, choices, reserved);
          break;
        } catch (error) {
          if (!(error instanceof PendingConflict)) throw error;
          if (!await askConflicts(new Map([[error.path, error.snapshot]]), target, choices)) return;
        }
      }
      if (run !== current) return;
      current.batchSummary = null;
      if (outcome.status === 'omitted') {
        current.omitted.set(row.path, outcome.note);
        status.textContent = outcome.note;
        updateReceipt(current);
        return;
      }
      if (outcome.status === 'failed') {
        status.textContent = outcome.note;
        failPhoto(current, row.path, outcome.note, outcome.category);
        updateReceipt(current);
        return;
      }
      current.suggestion = target.handle;
      if (!current.target) current.target = target;
      status.textContent = (outcome.status === 'skipped' ? 'Already recovered · ' : 'Saved · ') + outcome.actual;
      el('status').textContent = status.textContent;
      // The receipt counts this run's photos, so a one-off save is a photo saved
      // even when no batch ever runs (`#117`, user story 27).
      promote(current, row, outcome, target);
      current.reconciled = await reconcileReceipt(current);
      log.confirmed(current.reconciled.ok ? new Set(current.reconciled.present.map(({ id }) => id)).size : null);
      updateReceipt(current);
    }, status);
  });
  start.addEventListener('click', () => {
    if (run) void locked(run, () => saveBatch(run), el('status'));
  });
  saveBarSave.addEventListener('click', () => {
    if (run) void locked(run, () => saveBatch(run), el('status'));
  });
  stop.addEventListener('click', () => {
    if (!run?.batch || run.batch.reporting) return;
    run.batch.stopping = true;
    log.stopped();
    stop.disabled = true;
    saveBarSave.disabled = true;
    el('status').textContent += ' · Stopping after current writes';
    if (!progress.hidden) progress.textContent += ' · stopping';
    announce('Stopping after the current writes.');
  });
  end.addEventListener('click', () => {
    if (!run || run.busy) return;
    run.batch = null;
    render();
  });

  return { open, clear, select, photo, busy: () => run?.busy === true };
}
