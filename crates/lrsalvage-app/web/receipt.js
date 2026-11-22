// The receipt: what a run got back, and the evidence for it.//
// It is a section of the result screen rather than a stage, so a run that saved
// one photo ends with the same account a batch does and a stopped batch keeps its
// continuation beside it (`specs/app.md`, *Receipt, reports and diagnostics*).
//
// Two of its statements are reads rather than recollections: the reported count
// comes from the run's own commits, and **View saved files** lists the destination
// back through the handle that wrote it. A page cannot learn a folder's path — a
// directory handle carries a name and nothing else — so the receipt names the
// folder, offers that name for copying, and says how to find the folder in
// Explorer instead of offering an *Open folder* the page cannot honour.

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
  const destination = el('destination');
  const where = el('where');
  const view = el('view');
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
    if (written.length === 0 && skipped.length === 0 && state.failures.length === 0) {
      clear();
      return;
    }
    section.hidden = false;

    // The number a person came for: distinct catalog images this run saved, each
    // counted once however many times a one-off and a batch reached it.
    headline.textContent = `${counted(written.length)} ` +
      (written.length === 1 ? 'photo saved' : 'photos saved');

    const parts = [];
    if (skipped.length) parts.push(`${counted(skipped.length)} already recovered`);
    if (state.failures.length) parts.push(`${counted(state.failures.length)} could not be saved`);
    // The count belongs to a read-back, and a count of nothing says nothing the
    // failures have not already said better.
    const found = state.reconciled?.ok ? state.reconciled.present.length : 0;
    if (found > 0) {
      parts.push(`${counted(found)} ${found === 1 ? 'file' : 'files'} in the folder`);
    }
    detail.textContent = parts.join(' · ');

    // A listing of nothing is not worth opening, and a run that saved nothing
    // has no folder to name.
    const savedCount = written.length + skipped.length;
    view.hidden = savedCount === 0;
    if (savedCount === 0 && listed) {
      listed = false;
      files.hidden = true;
      files.replaceChildren();
      saved.textContent = '';
      view.setAttribute('aria-expanded', 'false');
      view.textContent = 'View saved files';
    }

    renderReconciled(state);

    const chosen = state.destinationNames;
    destination.textContent = chosen.length
      ? `Saved to ${chosen.map((name) => `\u201c${name}\u201d`).join(' and ')}`
      : '';
    // A page cannot open a folder or learn where it is; saying so once is worth
    // more than a button that would pretend.
    where.textContent = chosen.length === 1
      ? `To find it, open File Explorer and paste the folder name into the address bar — ` +
        `the button below copies it. Your browser may also list it under recent folders.`
      : '';
    copyName.hidden = chosen.length !== 1;

    el('files').setAttribute('aria-label', `Files in ${chosen.join(' and ') || 'the destination'}`);
    renderFailures(state.failures);
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
    if (settled.missing.length === 0) {
      // Nothing was saved, so there is nothing to have been found: the failure
      // list is the whole account of this run.
      reconciled.textContent = settled.present.length > 0
        ? 'Every file above was found in the folder when it was read back.'
        : '';
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
      return settled.present.map((entry) => {
        const known = state.ledger.get(entry.id);
        return { path: entry.path, bytes: entry.bytes, status: known?.status ?? 'written' };
      });
    }
    return [...state.ledger.values()];
  }

  /** What the listing is: the folder's answer, or this run's record when it could not read one. */
  function describeListing(state) {
    saved.textContent = state.reconciled?.ok
      ? 'This is the folder listed back through this page, not a second plan.'
      : 'The folder could not be read back, so this is this run’s own record.';
  }

  function renderFiles(state) {
    files.replaceChildren();
    const groups = new Map();
    for (const entry of filesFor(state)) {
      const key = folder(entry.path);
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

  view.addEventListener('click', () => {
    listed = !listed;
    files.hidden = !listed;
    view.setAttribute('aria-expanded', String(listed));
    view.textContent = listed ? 'Hide saved files' : 'View saved files';
    if (listed && current) {
      renderFiles(current);
      describeListing(current);
    }
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

  return { update, clear, renderVersions, show: () => section, focus: () => title.focus() };
}
