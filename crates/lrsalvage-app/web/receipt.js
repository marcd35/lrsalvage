// The receipt: what a run got back, and the evidence for it.//
// It is a section of the result screen rather than a stage, so a run that saved
// one photo ends with the same account a batch does and a stopped batch keeps its
// continuation beside it (`specs/app.md`, *Receipt, reports and diagnostics*).
//
// Two of its statements are reads rather than recollections: the reported count
// comes from the run's own commits, and **Show file list** lists the destination
// back through the handle that wrote it. A page cannot learn a folder's path — a
// directory handle supplies the folder name, which the receipt shows and
// offers to copy.

import { log } from './diagnostics.js';
import { counted } from './count.js';

/** `IMG_0001.jpg` from `2023/IMG_0001.jpg`. */
function name(path) {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? path : path.slice(slash + 1);
}

/** `2023` from `2023/IMG_0001.jpg`, or `null` for a flat layout. */
function folder(path) {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? null : path.slice(0, slash);
}

function quantity(count) {
  return counted(count) + (count === 1 ? ' file' : ' files');
}

function recoveryCount(batch) {
  return counted(batch.verified) + ' of ' + quantity(batch.total) + ' recovered';
}

function duration(milliseconds) {
  if (milliseconds < 1000) return 'less than 1 s';
  if (milliseconds < 60000) return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(milliseconds / 1000) + ' s';
  const seconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  return (hours ? counted(hours) + ' h ' : '') + (minutes % 60) + ' min ' + (seconds % 60) + ' s';
}

export function mountReceipt(document, actions) {
  const el = (suffix) => {
    const node = document.getElementById('receipt-' + suffix);
    if (!node) throw new Error(`the page is missing #receipt-${suffix}`);
    return node;
  };

  const section = el('section');
  const title = el('title');
  const headline = el('headline');
  const detail = el('detail');
  const reconciled = el('reconciled');
  const view = el('view');
  const completion = el('complete');
  const completionTitle = el('complete-title');
  const completionBreakdown = el('complete-breakdown');
  const completionDestination = el('complete-destination');
  const copyName = el('copy-name');
  const files = el('files');
  const saved = el('saved');
  const failures = el('failures');
  const reports = el('reports');
  const reportList = el('report-list');
  const reportFailures = el('report-failures');
  const verification = el('verification');
  const reportStatus = el('report-status');
  const versions = el('versions');
  const support = el('support');

  let current = null;
  let listed = false;

  function clear() {
    // The blob URLs belong to the reports this run wrote; a new run's receipt
    // must not keep them alive.
    for (const report of current?.reports ?? []) URL.revokeObjectURL(report.url);
    current = null;
    listed = false;
    section.hidden = true;
    if (completion.open) completion.close();
    files.replaceChildren();
    failures.replaceChildren();
    reportList.replaceChildren();
    reportFailures.replaceChildren();
    reconciled.textContent = '';
    files.hidden = true;
    view.setAttribute('aria-expanded', 'false');
    saved.textContent = '';
    reportStatus.textContent = '';
    el('diagnostics-status').textContent = '';
  }

  /**
   * Update the account, and show it once the run has committed something.
   *
   * `state` is `export.js`'s: the run's ledger of committed catalog images, the
   * reports it wrote, the failures it recorded, and what the destination answered
   * when it was listed back.
   */
  function update(state) {
    current = state;
    const entries = [...state.ledger.values()];
    const written = entries.filter((entry) => entry.status === 'written');
    const skipped = entries.filter((entry) => entry.status === 'skipped');
    if (!state.batch && !entries.length && !state.failures.length && !state.omitted.size) {
      clear();
      return;
    }
    section.hidden = false;

    const folderNames = state.destinationNames.map((name) => '“' + name + '”').join(' and ');
    const batch = state.batch;
    const savedCount = written.length + skipped.length;
    const where = batch ? '“' + batch.folder + '”' : folderNames;
    const to = where ? ' to ' + where : '';
    const inFolder = where ? ' in ' + where : '';
    const writtenCount = batch?.written ?? written.length;
    const already = batch?.already ?? skipped.length;
    const omitted = batch?.omitted ?? state.omitted.size;
    const failuresCount = batch?.failed ?? state.failures.length;
    if (batch && batch.verified !== null && batch.verified !== batch.total) {
      headline.textContent = recoveryCount(batch) + inFolder;
    } else if (!writtenCount && already) {
      headline.textContent = quantity(already) + ' already recovered' + inFolder;
    } else {
      headline.textContent = quantity(writtenCount) + ' saved' + to;
    }
    const parts = [];
    if (already && writtenCount) parts.push(counted(already) + ' already recovered');
    if (omitted) parts.push(counted(omitted) + ' skipped');
    if (failuresCount) parts.push(counted(failuresCount) + ' could not be saved');
    if (batch?.renamed) parts.push(counted(batch.renamed) + ' kept both');
    detail.textContent = parts.join(' · ');

    // The file list is available when the run has a committed path.
    view.hidden = savedCount === 0;
    if (savedCount === 0 && listed) {
      listed = false;
      files.hidden = true;
      files.replaceChildren();
      saved.textContent = '';
      view.setAttribute('aria-expanded', 'false');
      view.textContent = 'Show file list';
    }

    renderReconciled(state);

    const chosen = state.destinationNames;
    copyName.hidden = chosen.length !== 1;

    el('files').setAttribute('aria-label', `Files in ${chosen.join(' and ') || 'the destination'}`);
    renderFailures([...state.failures, ...[...state.omitted].map(([path, note]) => ({ path, note }))]);
    renderReports(state.reports);
    renderReportFailures(state.reportFailures ?? []);
    support.hidden = written.length === 0;

    if (listed) {
      renderFiles(state);
      describeListing(state);
    }
  }
  /**
   * What the destination answered for, and what it did not.
   *
   * This is the one line the spec insists on: a plan path the folder no longer
   * holds is named, and a folder that cannot be read back says so rather than
   * letting the run's own record pass for an observation.
   */
  function renderReconciled(state) {
    const settled = state.reconciled;
    if (!settled) {
      reconciled.textContent = '';
      return;
    }
    if (!settled.ok) {
      reconciled.textContent =
        'The folder could not be read back just now, so the counts above are this run’s ' +
        'own record rather than what is on the disk.';
      return;
    }
    if (settled.changed?.length) {
      const missing = settled.missing.length ? ' Missing: ' + settled.missing.join(', ') + '.' : '';
      reconciled.textContent = 'Changed in the destination: ' + settled.changed.join(', ') + '.' + missing;
      return;
    }
    if (settled.missing.length === 0) {
      reconciled.textContent = '';
      return;
    }
    reconciled.textContent = `${counted(settled.missing.length)} of them ` +
      `${settled.missing.length === 1 ? 'is' : 'are'} not in the folder any more: ` +
      `${settled.missing.join(', ')}.`;
  }

  function renderFailures(state) {
    failures.replaceChildren();
    for (const { path, note } of state) {
      const item = document.createElement('li');
      item.textContent = `${path} · ${note}`;
      failures.append(item);
    }
  }

  function renderReports(state) {
    reportList.replaceChildren();
    // The section is always here, because the verification report is offered for
    // any run that reached a file — a one-off-only run has no report files yet
    // and still has something worth writing.
    reports.hidden = false;
    el('report-note').hidden = state.length > 0;
    verification.hidden = false;
    for (const report of state) {
      const item = document.createElement('li');
      const link = document.createElement('a');
      link.href = report.url;
      link.target = '_blank';
      link.rel = 'noopener';
      link.textContent = report.name;
      item.append(link, document.createTextNode(` · ${report.description}`));
      reportList.append(item);
    }
  }

  /** A report that could not be written is a failure, never a link. */
  function renderReportFailures(state) {
    reportFailures.replaceChildren();
    for (const { name, note } of state) {
      const item = document.createElement('li');
      item.textContent = `${name} · ${note}`;
      reportFailures.append(item);
    }
  }

  /**
   * The listing: the destination's own answer when it was read, otherwise the
   * run's record — with the difference said out loud in `#receipt-saved`.
   */
  function filesFor(state) {
    const settled = state.reconciled;
    if (settled?.ok) {
      return settled.present;
    }
    return state.files.map((entry) => ({ ...entry, destination: entry.target.handle.name }));
  }

  /** What the listing is: the folder's answer, or this run's record when it could not read one. */
  function describeListing(state) {
    saved.textContent = state.reconciled?.ok ? '' : 'This list comes from the run’s own record.';
  }

  function renderFiles(state) {
    files.replaceChildren();
    const groups = new Map();
    for (const entry of filesFor(state)) {
      const subfolder = folder(entry.path);
      const key = state.destinationNames.length > 1
        ? [entry.destination, subfolder].filter(Boolean).join('/') : subfolder;
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(entry);
    }
    const sorted = [...groups.entries()].sort(([a], [b]) => String(a).localeCompare(String(b)));
    for (const [key, group] of sorted) {
      const block = document.createElement('div');
      if (key !== null) {
        const heading = document.createElement('h4');
        heading.textContent = key;
        block.append(heading);
      }
      const list = document.createElement('ul');
      for (const entry of group.sort((a, b) => a.path.localeCompare(b.path))) {
        const item = document.createElement('li');
        item.textContent = `${name(entry.path)} · ${counted(entry.bytes)} bytes` +
          (entry.status === 'skipped' ? ' · already recovered' : '');
        list.append(item);
      }
      block.append(list);
      files.append(block);
    }
  }

  async function renderVersions() {
    const identity = await log.identity();
    versions.textContent = `lrsalvage app ${identity.app ?? 'version unknown'} · ` +
      `core ${identity.core ?? 'version unknown'}`;
  }

  function showCompletion(batch) {
    completionTitle.textContent = recoveryCount(batch) + ' in ' + duration(batch.elapsed);
    completionDestination.textContent = 'Saved to “' + batch.folder + '”';
    const parts = [];
    if (batch.already) parts.push(counted(batch.written) + ' saved', counted(batch.already) + ' already recovered');
    if (batch.renamed) parts.push(counted(batch.renamed) + ' kept both');
    completionBreakdown.hidden = parts.length === 0;
    completionBreakdown.textContent = parts.join(' · ');
    completion.showModal();
  }

  function showListing(expanded) {
    listed = expanded;
    files.hidden = !listed;
    view.setAttribute('aria-expanded', String(listed));
    view.textContent = listed ? 'Hide file list' : 'Show file list';
    if (listed && current) {
      renderFiles(current);
      describeListing(current);
    }
  }

  view.addEventListener('click', () => showListing(!listed));
  completion.addEventListener('close', () => {
    if (!section.hidden && !completion.open) title.focus();
  });
  copyName.addEventListener('click', () => {
    const chosen = current?.destinationNames ?? [];
    if (chosen.length !== 1) return;
    void log.copyText(chosen[0], saved, 'The folder name is on the clipboard.');
  });

  el('diagnostics').addEventListener('click', () => {
    void log.copy(el('diagnostics-status'));
  });

  verification.addEventListener('click', () => {
    // The export module refuses a click while a write is in flight, so the button
    // stays enabled: a disabled control would need a re-render to come back.
    if (!current) return;
    void actions.saveVerification(reportStatus);
  });

  return { update, clear, renderVersions, showCompletion, show: () => section, focus: () => title.focus() };
}
