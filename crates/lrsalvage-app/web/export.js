import { pickFolder } from './app.js';
import { readDestination, rememberDestination } from './destination.js';
import { commitFile } from './seam/commit-file.js';

const MAX_WRITES = 8;

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
      return;
    } catch (error) {
      if (!(error instanceof OccupiedPath)) throw error;
    }
  }
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
  let run;

  function open(scan, inputs) {
    clear();
    run = {
      scan, inputs, checked: new Set(), layouts: new Map(), targets: [],
      flat: false, target: null, suggestion: null, batch: null, busy: false, photo: null,
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
      if (run === current) render();
    }
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
        note: error.message,
      };
    }
  }

  function addError(path, message) {
    const entry = document.createElement('li');
    entry.textContent = path + ' · ' + message;
    el('errors').append(entry);
  }

  async function saveBatch(current) {
    const target = current.target;
    await validateDestination(target.handle, current.inputs);
    const batch = current.batch ??= { rows: selected(current), outcomes: new Map() };
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
    await Promise.all(Array.from({ length: Math.min(MAX_WRITES, pending.length) }, async () => {
      while (!batch.stopping && next < pending.length) {
        const row = pending[next++];
        const outcome = await write(current, target, current.flat, row);
        batch.outcomes.set(row.id, outcome);
        if (run !== current) continue;
        totals[outcome.status]++;
        if (outcome.status === 'failed') addError(row.path, outcome.note);
        paint();
      }
    }));
    if (run !== current) return;
    if (next < pending.length) {
      el('status').textContent += ' · Stopped';
      return;
    }

    batch.reporting = true;
    render();
    try {
      const reports = await current.scan.exportReports(current.flat, batch.rows.map(({ id }) => batch.outcomes.get(id)));
      for (const [name, text] of Object.entries(reports)) {
        try {
          await writeReport(target.handle, name, text);
        } catch (error) {
          addError(name, error.message);
        }
      }
    } finally {
      current.batch = null;
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
        addError(row.path, outcome.note);
        return;
      }
      current.suggestion = target.handle;
      if (!current.target) current.target = target;
      status.textContent = (outcome.status === 'skipped' ? 'Already recovered · ' : 'Saved · ') + row.path;
      el('status').textContent = status.textContent;
    }, status);
  });
  start.addEventListener('click', () => {
    if (run) void locked(run, () => saveBatch(run), el('status'));
  });
  stop.addEventListener('click', () => {
    if (!run?.batch || run.batch.reporting) return;
    run.batch.stopping = true;
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
