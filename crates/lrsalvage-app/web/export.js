import { pickFolder } from './app.js';
import { readDestination, rememberDestination } from './destination.js';
import { commitFile } from './seam/commit-file.js';
import { log } from './diagnostics.js';
import { mountReceipt } from './receipt.js';

const MAX_WRITES = 8;

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

const describe = (name) =>
  DESCRIPTIONS[name.replace(/ \(\d+\)(?=\.[^.]+$)/, '')] ?? 'Report';

async function validateDestination(destination, inputs) {
  for (const { handle } of inputs) {
    if (await handle.resolve(destination) !== null || await destination.resolve(handle) !== null) {
      throw new Error('Choose a folder outside the catalog, preview cache and original folders.');
    }
  }
}

class OccupiedPath extends Error {
  constructor() {
    super('A file already exists at this path. It was left unchanged.');
  }
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

async function writeNew(root, relative, prepare, inspectExisting) {
  const parts = relative.split('/');
  if (parts.some((part) => !part || part === '.' || part === '..' || part.includes('\\'))) {
    throw new Error('The planned path is outside the destination.');
  }
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
    const existing = await fileAt(parent, name);
    if (existing) return occupied(existing);
    const { bytes, outcome } = await prepare();
    const appeared = await fileAt(parent, name);
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
  const dot = name.lastIndexOf('.');
  const bytes = new TextEncoder().encode(text);
  for (let number = 1; ; number++) {
    const path = number === 1 ? name : name.slice(0, dot) + ' (' + number + ')' + name.slice(dot);
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
 * the file is really there.
 */
async function writeAndLink(destination, name, text) {
  const path = await writeReport(destination, name, text);
  const file = await (await destination.getFileHandle(path)).getFile();
  return { name: path, url: URL.createObjectURL(file), description: describe(path) };
}

/**
 * What the destination answers when it is listed back.
 *
 * One walk, intersected with the run's committed paths: the count on the receipt
 * is a count of files that exist, not a count of writes that were attempted. A
 * folder that cannot be read says so instead of claiming its files.
 */
async function reconcile(destination, ledger) {
  const entries = [...ledger.values()];
  const planned = new Set(entries.map((entry) => entry.path.toLowerCase()));
  const found = new Set();
  try {
    const walk = async (dir, prefix) => {
      for await (const [name, entry] of dir.entries()) {
        const path = prefix + name;
        if (entry.kind === 'directory') await walk(entry, path + '/');
        else if (planned.has(path.toLowerCase())) found.add(path.toLowerCase());
      }
    };
    await walk(destination, '');
  } catch (error) {
    return { ok: false, reason: classify(error) };
  }
  return { ok: true, confirmed: found.size, missing: entries.length - found.size };
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
  const details = el('paths');
  const receipt = mountReceipt(document, { saveVerification });
  let run;

  function open(scan, inputs) {
    clear();
    run = {
      scan, inputs, checked: new Set(), layouts: new Map(), targets: [],
      flat: false, target: null, suggestion: null, batch: null, busy: false, photo: null,
      // The run's account: one entry per catalog image this run committed, plus
      // the failures and the reports it wrote. `targets[].saved` caches per
      // destination for the writer; this is what the receipt counts.
      ledger: new Map(), failures: [], reports: [], categorised: new Set(),
    };
    section.hidden = false;
    layout.value = 'mirrored';
    void loadLayout(run, false);
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
    details.open = false;
    el('status').textContent = '';
    el('errors').replaceChildren();
    el('one-status').textContent = '';
    receipt.clear();
  }

  /** Show the run's account, and read the destination back when it has settled. */
  function updateReceipt(current) {
    if (run !== current) return;
    receipt.update({
      ledger: current.ledger,
      failures: current.failures,
      reports: current.reports,
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
    el('total').textContent = rows.length + ' JPEGs · ' + bytes.toLocaleString() + ' preview bytes';
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
    renderOne();
    if (details.open) renderPaths(rows);
  }

  function renderPaths(rows) {
    const list = el('path-list');
    list.replaceChildren();
    for (const { path } of rows) {
      const item = document.createElement('li');
      item.textContent = path;
      list.append(item);
    }
  }

  async function choose(current) {
    const picked = await pickFolder({ id: 'lrsalvage-destination', mode: 'readwrite' });
    if (picked.cancelled) return;
    if (picked.refused) throw new Error(picked.refused.message);
    return accept(current, picked.handle);
  }

  async function accept(current, handle) {
    await validateDestination(handle, current.inputs);
    void rememberDestination(handle).catch(() => {});
    for (const target of current.targets) {
      if (await target.handle.isSameEntry(handle)) {
        target.handle = handle;
        return target;
      }
    }
    const target = { handle, saved: new Map() };
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
      if (run === current) status.textContent = error.message;
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
   * Record one catalog image this run saved, once.
   *
   * A photo a one-off and a batch both reached is one photo on the receipt: an
   * entry that already exists is never demoted to `already recovered`, because
   * this run did write it.
   */
  function promote(current, row, outcome, target) {
    const previous = current.ledger.get(row.id);
    const better = !previous || (previous.status === 'skipped' && outcome.status === 'written');
    if (better) {
      if (!previous) log.commit(outcome.status);
      current.ledger.set(row.id, {
        id: row.id, path: row.path, status: outcome.status, bytes: outcome.bytes, target,
      });
    }
    const at = current.failures.findIndex((entry) => entry.path === row.path);
    if (at >= 0) current.failures.splice(at, 1);
    return better;
  }

  async function write(current, target, flat, row) {
    const key = row.path.toLowerCase();
    const previous = target.saved.get(key);
    if (previous?.id === row.id) return previous.outcome;
    try {
      const outcome = await writeNew(target.handle, row.path,
        () => current.scan.exportJpeg(row.id, flat),
        async (handle) => {
          const file = await handle.getFile();
          const bytes = new Uint8Array(await file.arrayBuffer());
          return current.scan.existingExport(row.id, flat, bytes);
        },
      );
      target.saved.set(key, { id: row.id, outcome });
      return outcome;
    } catch (error) {
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
   * Record one file that will not be in the folder, once per path.
   *
   * The receipt lists it, and the diagnostics count its category — never its
   * message, which can name a path — so the count and the list cannot disagree
   * about how many photos are missing.
   */
  function fail(current, path, message, category) {
    const at = current.failures.findIndex((entry) => entry.path === path);
    const entry = { path, note: message };
    if (at >= 0) current.failures[at] = entry;
    else current.failures.push(entry);
    addError(path, message);
    if (!current.categorised.has(path)) {
      current.categorised.add(path);
      log.failure(category);
    }
  }

  async function saveBatch(current) {
    const target = current.target;
    await validateDestination(target.handle, current.inputs);
    if (current.batch) log.continued();
    const batch = current.batch ??= { rows: selected(current), outcomes: new Map() };
    if (batch.outcomes.size === 0) log.planned(batch.rows.length);
    batch.stopping = false;
    render();
    const totals = { written: 0, skipped: 0, failed: 0 };
    const pending = batch.rows.filter(({ id }) => {
      const status = batch.outcomes.get(id)?.status;
      if (status !== 'written' && status !== 'skipped') return true;
      totals[status]++;
      return false;
    });
    let next = 0;
    el('errors').replaceChildren();
    const paint = () => {
      el('status').textContent = totals.written + ' saved of ' + batch.rows.length +
        (totals.skipped ? ' · ' + totals.skipped + ' already recovered' : '') +
        (totals.failed ? ' · ' + totals.failed + ' failed' : '');
    };
    paint();

    // One entry per catalog image, so a photo a one-off and a batch both reached
    // is one photo on the receipt.
    const record = (row, outcome) => {
      if (outcome.status === 'written' || outcome.status === 'skipped') {
        promote(current, row, outcome, target);
      } else {
        fail(current, row.path, outcome.note ?? 'The file could not be written.', outcome.category);
      }
    };

    await Promise.all(Array.from({ length: Math.min(MAX_WRITES, pending.length) }, async () => {
      while (!batch.stopping && next < pending.length) {
        const row = pending[next++];
        const outcome = await write(current, target, current.flat, row);
        batch.outcomes.set(row.id, outcome);
        if (run !== current) continue;
        totals[outcome.status]++;
        record(row, outcome);
        paint();
      }
    }));
    log.settled();
    if (run !== current) return;

    // The destination's own answer, not the writer's recollection of it.
    const settled = await reconcile(target.handle, current.ledger);
    log.confirmed(settled.ok ? settled.confirmed : null);
    if (run !== current) return;
    if (next < pending.length) {
      el('status').textContent += ' · Stopped';
      updateReceipt(current);
      return;
    }

    batch.reporting = true;
    render();
    try {
      const reports = await current.scan.exportReports(current.flat, batch.rows.map(({ id }) => batch.outcomes.get(id)));
      const written = [];
      for (const [name, text] of Object.entries(reports)) {
        try {
          written.push(await writeAndLink(target.handle, name, text));
        } catch (error) {
          fail(current, name, error.message, classify(error));
        }
      }
      current.reports = [
        ...written,
        ...current.reports.filter((report) => report.name.startsWith('verification')),
      ];
    } finally {
      current.batch = null;
    }
    updateReceipt(current);
    receipt.focus();
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
      const { text } = await current.scan.verificationReport();
      const entry = await writeAndLink(current.target.handle, 'verification.json', text);
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

  details.addEventListener('toggle', () => {
    if (run && details.open) renderPaths(run.batch?.rows ?? selected(run));
    else el('path-list').replaceChildren();
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
    }, status);
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
      const outcome = await write(current, target, current.flat, row);
      if (outcome.status === 'failed') {
        status.textContent = outcome.note;
        fail(current, row.path, outcome.note, outcome.category);
        updateReceipt(current);
        return;
      }
      current.suggestion = target.handle;
      if (!current.target) current.target = target;
      status.textContent = (outcome.status === 'skipped' ? 'Already recovered · ' : 'Saved · ') + row.path;
      el('status').textContent = status.textContent;
      // The receipt counts this run's photos, so a one-off save is a photo saved
      // even when no batch ever runs (`#117`, user story 27).
      promote(current, row, outcome, target);
      updateReceipt(current);
    }, status);
  });
  start.addEventListener('click', () => {
    if (run) void locked(run, () => saveBatch(run), el('status'));
  });
  stop.addEventListener('click', () => {
    if (!run?.batch || run.batch.reporting) return;
    run.batch.stopping = true;
    log.stopped();
    stop.disabled = true;
    el('status').textContent += ' · Stopping after current writes';
  });
  end.addEventListener('click', () => {
    if (!run || run.busy) return;
    run.batch = null;
    render();
  });

  return { open, clear, select, photo, busy: () => run?.busy === true };
}
