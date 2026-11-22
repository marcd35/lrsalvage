// The page's DOM: what it shows, and what each control does.
//
// Every string a person reads is here, and every one of them was chosen against a
// count the report actually carries — `#117`'s user stories 4 and 5 are about
// saying *which* nothing happened, and "no previews found" answers none of them.
// Untrusted text is only ever assigned to `textContent`: a catalog's folder path
// comes from a file this page did not write.
//
// The control flow is `app.js`'s; this file has no format knowledge and no
// decisions.

import {
  capability,
  folderSource,
  pickFolder,
  sourceFrom,
  start,
  withMount,
} from './app.js';
import { loadPractice } from './practice.js';
import { log } from './diagnostics.js';
import { counted } from './count.js';
import { createScanProgress } from './scan-progress.js';

import { mountReview } from './review.js';
import { mountExport } from './export.js';

const NO_PICKER =
  'This browser cannot open folders for recovery. Use Chrome or Edge on Windows, ' +
  'or use the command-line tool.';

const NO_ISOLATION =
  'This page is not served the way it has to be. It needs Cross-Origin-Opener-Policy ' +
  'same-origin and Cross-Origin-Embedder-Policy require-corp, which its host sets — ' +
  'nothing on your side can fix it.';

const INSECURE =
  'This page is not in a secure context, so the browser will not hand it a folder. ' +
  'Open it over https, or from the deployed address.';

export function mount(document) {
  const el = (id) => {
    const node = document.getElementById(id);
    if (!node) throw new Error(`the page is missing #${id}`);
    return node;
  };

  const gate = el('gate');
  const gateMessage = el('gate-message');
  const source = el('source');
  const pickCatalogue = el('pick');
  const pickCache = el('pick-cache');
  const picked = el('picked');
  const choices = el('choices');
  const scanButton = el('scan');
  const pickerError = el('picker-error');
  const scanning = el('scanning');
  const stopButton = el('stop');
  const scanProgress = createScanProgress(document);
  const result = el('result');
  const headline = el('headline');
  const causes = el('causes');
  const checked = el('checked');
  const roots = el('roots');
  const rootsSection = el('roots-section');
  const restart = el('restart');
  const resultTitle = el('result-title');
  const exports = mountExport(document);
  const review = mountReview(document, exports);

  let state = {
    handle: null,
    catalog: null,
    cache: null,
    cacheHandle: null,
    cacheName: null,
    source: null,
  };
  let running = null;
  let completed = null;

  function clearReview() {
    exports.clear();
    review.clear();
    completed?.close();
    completed = null;
  }

  const showed = capability();
  if (!showed.ok) {
    gateMessage.textContent =
      { 'no-picker': NO_PICKER, 'no-isolation': NO_ISOLATION, insecure: INSECURE }[showed.reason] ??
      'This browser cannot run a recovery.';
    showOnly(gate);
    return;
  }

  showOnly(source);
  el('recover-cache').addEventListener('click', () => void chooseRecoveryCache(pickerError));
  el('recover-cache-after-error').addEventListener('click', () => void recoverAfterError());
  scanButton.addEventListener('click', scanChosenCatalog);

  function scanChosenCatalog() {
    const chosen = chosenCatalog();
    if (!chosen) return;
    state.catalog = chosen;
    state.source = sourceFrom({
      handle: state.handle,
      catalog: chosen,
      cacheHandle: state.cacheHandle,
      cacheName: state.cacheName,
      cache: state.cache,
    });
    void runScan(state.source);
  }
  pickCatalogue.addEventListener('click', () => void chooseCatalogue());
  pickCache.addEventListener('click', () => void chooseCache());
  stopButton.addEventListener('click', () => {
    if (!running) return;
    running.stop();
    scanProgress.stop();
  });
  restart.addEventListener('click', () => {
    if (exports.busy()) return;
    clearReview();
    state = {
      handle: null,
      catalog: null,
      cache: null,
      cacheHandle: null,
      cacheName: null,
      source: null,
    };
    picked.textContent = '';
    choices.replaceChildren();
    scanButton.disabled = true;
    pickCache.hidden = true;
    pickerError.hidden = true;
    showOnly(source);
    pickCatalogue.focus();
  });

  const practice = new URL(document.URL).searchParams.get('sample');
  if (practice !== null) void openPractice(practice);

  // -- picking ------------------------------------------------------------

  async function openPractice(name) {
    showOnly(scanning);
    scanProgress.reset('Loading the practice files…');
    stopButton.hidden = true;
    try {
      const handle = await loadPractice(name);
      if (name === 'cache-only') {
        await runScan(cacheSource(handle));
        return;
      }
      await acceptCatalogue(handle);
      if (!chosenCatalog()) throw new Error('The practice folder has no catalog.');
      scanChosenCatalog();
    } catch (error) {
      showOnly(source);
      say(pickerError, `The practice files could not open. ${error.message}`);
    }
  }

  function cacheSource(handle) {
    return { mounts: [{ path: '/cache', handle }], catalog: null, cache: '/cache' };
  }

  async function chooseRecoveryCache(message) {
    const answer = await pickFolder({ id: 'lrsalvage-cache', mode: 'read' });
    if (answer.cancelled) {
      say(message, 'Nothing was chosen, so nothing changed.');
    } else if (answer.refused) {
      say(message, `This browser refused the folder: ${answer.refused.message}`);
    } else {
      await runScan(cacheSource(answer.handle));
    }
  }

  async function recoverAfterError() {
    const chosen = state.source;
    if (chosen) {
      const mount = chosen.mounts.find(({ path }) =>
        chosen.cache === path || chosen.cache.startsWith(path + '/'));
      if (mount) {
        let handle = mount.handle;
        try {
          for (const component of chosen.cache.slice(mount.path.length).split('/').filter(Boolean)) {
            handle = await handle.getDirectoryHandle(component);
          }
        } catch {
          handle = null;
        }
        if (handle) {
          await runScan(cacheSource(handle));
          return;
        }
      }
    }
    await chooseRecoveryCache(headline);
  }

  async function chooseCatalogue() {
    pickerError.hidden = true;
    const answer = await pickFolder({ id: 'lrsalvage-catalog', mode: 'read' });
    if (answer.cancelled) {
      say(pickerError, 'Nothing was chosen, so nothing changed.');
      return;
    }
    if (answer.refused) {
      say(pickerError, `This browser refused the folder: ${answer.refused.message}`);
      return;
    }
    await acceptCatalogue(answer.handle);
  }

  async function acceptCatalogue(handle) {
    const found = await folderSource(handle);
    if (found.catalogs.length === 0) {
      say(
        pickerError,
        'That folder holds no .lrcat catalog. Choose the folder your catalog is in — ' +
          'the file ending in .lrcat.',
      );
      return;
    }

    state.handle = handle;
    state.cacheHandle = null;
    state.catalog = found.catalog;
    state.cache = found.cache;

    picked.replaceChildren();
    picked.append(line('Catalog: ', found.catalogs.join(', ')));
    if (found.cache) {
      picked.append(line('Previews: ', found.cache));
    } else {
      picked.append(
        line(
          '',
          'No preview folder sits beside that catalog. If your previews live somewhere ' +
            'else, choose that folder below.',
        ),
      );
    }

    choices.replaceChildren();
    if (found.catalogs.length > 1) {
      const label = document.createElement('label');
      label.textContent = 'Which catalog? ';
      const select = document.createElement('select');
      select.id = 'catalog-choice';
      for (const name of found.catalogs) {
        const option = document.createElement('option');
        option.value = name;
        option.textContent = name;
        select.append(option);
      }
      label.append(select);
      choices.append(label);
    }

    pickCache.hidden = false;
    pickCache.textContent = found.cache
      ? 'Choose a different preview folder…'
      : 'Choose the preview folder…';
    scanButton.disabled = false;
  }

  async function chooseCache() {
    pickerError.hidden = true;
    const answer = await pickFolder({ id: 'lrsalvage-cache', mode: 'read' });
    if (answer.cancelled) {
      say(pickerError, 'Nothing was chosen, so nothing changed.');
      return;
    }
    if (answer.refused) {
      say(pickerError, `This browser refused the folder: ${answer.refused.message}`);
      return;
    }
    const found = await folderSource(answer.handle);
    // The user may have picked the `.lrdata` folder itself or the folder holding
    // it; both are ordinary answers to "where are your previews", and the name
    // is what tells them apart.
    state.cacheHandle = answer.handle;
    state.cacheName = found.caches[0] ?? null;
    state.cache = null;
    picked.append(
      line(
        'Previews: ',
        found.caches[0] ? `${found.caches[0]} (chosen)` : `${answer.handle.name} (itself)`,
      ),
    );
    scanButton.disabled = false;
  }

  function chosenCatalog() {
    const select = document.getElementById('catalog-choice');
    return select ? select.value : state.catalog;
  }

  // -- scanning -----------------------------------------------------------

  async function runScan(chosen) {
    if (running || exports.busy()) return;
    state.source = chosen;
    const cacheOnly = !chosen.catalog;
    pickerError.hidden = true;
    // A run's diagnostics start here: a failure at the copy has no receipt to
    // attach them to, and it is the one somebody wants to hand over.
    log.run(cacheOnly);
    clearReview();
    showOnly(scanning);
    scanProgress.reset();
    stopButton.hidden = false;

    const ui = {
      versions(versions) {
        log.versions(versions);
      },
      stage(name) {
        log.stage(name);
        // The cache-only run has no catalog to copy, so it names its own wait
        // instead of the catalog copy's sentence; everything else about the
        // readout is `scan-progress.js`'s (#185), including the cache walk's
        // "files checked" and the stopping state.
        if (cacheOnly && name === 'copy') {
          scanProgress.reset('Opening the preview index…');
          return;
        }
        scanProgress.stage(name);
      },
      progress: scanProgress.progress,
    };

    const scan = start(chosen, ui, { review: true });
    running = scan;
    let report;
    try {
      report = await scan.done;
      // A completed scan may have reached 100% on the frame before this one. The
      // result replaces the readout, so the readout gets its paint first.
      await scanProgress.painted();
    } catch (error) {
      running = null;
      scanProgress.clear();
      showOnly(source);
      say(pickerError, `The scan could not finish: ${error.message}`);
      return;
    }
    running = null;
    scanProgress.clear();
    stopButton.hidden = true;

    if (report.stopped) {
      // #117: a stopped scan goes back to source selection, not to a half-filled
      // result.
      showOnly(source);
      say(pickerError, 'The scan was stopped. Nothing was written.');
      return;
    }
    render(report, chosen);
    showOnly(result);
    if (report.ok) {
      log.scanCounts(cacheOnly ? report.scan : report.stats);
      completed = scan;
      if (report.photos.some((photo) => photo.preview)) {
        exports.open(scan, chosen.mounts, cacheOnly);
      }
      if (!cacheOnly || report.photos.length) review.show(report.photos, scan.preview, cacheOnly);
    }
    resultTitle.focus();
  }

  // -- results ------------------------------------------------------------

  function render(report, chosen) {
    headline.replaceChildren();
    causes.replaceChildren();
    checked.replaceChildren();
    roots.replaceChildren();

    resultTitle.textContent = report.ok ? '3. Review previews' : 'The scan could not finish';
    const cacheOnly = !chosen.catalog;
    el('cache-notice').hidden = !cacheOnly || !report.ok || !report.photos.length;
    el('recover-cache-after-error').hidden = report.ok || cacheOnly ||
      !['open', 'copy'].includes(report.problem.stage);
    el('scan-details').hidden = !report.ok;
    if (!report.ok) {
      log.viewFailure(report.problem.stage, report.problem.kind);
      headline.append(
        document.createTextNode(failureSentence(report.problem.stage, report.problem.kind)),
      );
      const detail = document.createElement('details');
      const summary = document.createElement('summary');
      summary.textContent = 'Details for troubleshooting';
      const body = document.createElement('pre');
      body.textContent = `${report.problem.stage}: ${report.problem.message}`;
      detail.append(summary, body);
      // The copy is curated and the details are local: the raw message may name a
      // path, and it stays here.
      const status = document.createElement('p');
      status.setAttribute('role', 'status');
      status.className = 'review-note';
      const copy = document.createElement('button');
      copy.type = 'button';
      copy.textContent = 'Copy diagnostics';
      copy.addEventListener('click', () => void log.copy(status));
      const item = document.createElement('li');
      item.append(detail, copy, status);
      causes.append(item);
      rootsSection.hidden = true;
      return;
    }

    if (cacheOnly) {
      const { indexed, recoverable, missing, invalid, unsupported } = report.scan;
      headline.textContent = indexed === 0 ? 'This preview index is empty.' :
        `${counted(recoverable)} of ${counted(indexed)} indexed pyramids have a recoverable preview.`;
      fact('Indexed pyramids', counted(indexed));
      fact('Recoverable', counted(recoverable));
      fact('Missing', counted(missing));
      fact('Invalid', counted(invalid));
      fact('Unsupported', counted(unsupported));
      fact('Cache layout', report.layout);
      if (missing || invalid || unsupported) {
        const item = document.createElement('li');
        item.textContent = `${counted(missing)} missing · ${counted(invalid)} invalid · ${counted(unsupported)} unsupported`;
        causes.append(item);
      }
      rootsSection.hidden = true;
      return;
    }

    const stats = report.stats;
    const previewCount = report.photos.filter((photo) => photo.preview).length;
    headline.append(
      document.createTextNode(
        previewCount > 0
          ? `${counted(previewCount)} of ${counted(stats.images)} photos have a preview to review.`
          : `No previews found for the ${counted(stats.images)} photos in this catalog.`,
      ),
    );

    // Why, in counts. Each of these is a different nothing, and a page that says
    // only "no previews found" has not answered the question.
    const reasons = [];
    const index = report.index.problem;
    if (index && index.kind === 'missing') {
      reasons.push(
        'There is no previews.db in that folder. Lightroom’s preview index is what ' +
          'says which preview belongs to which photo, so without it nothing can be matched.',
      );
    }
    if (index && index.kind === 'unreadable') {
      reasons.push(
        `previews.db is there, and it could not be read: ${index.text}. A preview folder ` +
          'from a backup that was interrupted usually looks like this.',
      );
    }
    if (report.cache.layout === 'none') {
      reasons.push(report.index.present
        ? 'The preview folder holds an index and no preview files at all.'
        : 'No preview files were found. If the preview folder lives elsewhere, choose it and scan again.');
    }
    if (report.cache.layout === 'unknown') {
      reasons.push(
        `The preview folder holds ${counted(stats.unrecognised)} files that look like ` +
        'previews and match no layout this tool knows. Check that you chose the ' +
          'matching preview folder. If it matches your catalog, you can report the layout.',
      );
    }
    if (stats.images === 0) {
      reasons.push('That catalog holds no images.');
    }
    if (stats.no_cache_entry > 0) {
      reasons.push(
        `${counted(stats.no_cache_entry)} images are in the catalog and have no row in ` +
          'the preview index — Lightroom never rendered a preview for them, or the index ' +
          'is from a different catalog.',
      );
    }
    if (stats.missing_payload > 0) {
      reasons.push(
        `${counted(stats.missing_payload)} images have a row in the index and no preview ` +
          'file on disk. Lightroom prunes previews in the background, so this is ordinary ' +
          'for an old catalog.',
      );
    }
    if (stats.failed > 0) {
      reasons.push(
        `${counted(stats.failed)} preview files are damaged — truncated or unreadable — ` +
          'and are not offered.',
      );
    }
    if (stats.unmapped > 0) {
      reasons.push(
        `${counted(stats.unmapped)} previews belong to no image in this catalog. They are ` +
          'from images the catalog no longer lists.',
      );
    }
    for (const reason of reasons) {
      const item = document.createElement('li');
      item.textContent = reason;
      causes.append(item);
    }

    // What was checked, so "nothing found" can be checked rather than believed.
    fact('Images in the catalog', counted(stats.images));
    fact(
      'Previews index',
      index ? (index.kind === 'missing' ? 'missing' : `unreadable (${index.text})`) : 'read',
    );
    fact('Cache layout', report.cache.layout);
    fact(
      'Preview files',
      `${counted(report.cache.shapes.level_file)} level files, ` +
        `${counted(report.cache.shapes.container)} .lrprev containers`,
    );
    fact(
      'Level files whose name matches their own size',
      `${counted(report.cache.shapes.suffix_matches_sof)} of ` +
        counted(report.cache.shapes.level_file),
    );
    if (stats.unrecognised > 0) fact('Files matching no shape', counted(stats.unrecognised));
    if (report.cache.thumbnail_pyramids > 0) {
      fact('Lightroom’s second, thumbnail previews', counted(report.cache.thumbnail_pyramids));
    }
    if (stats.superseded > 0) fact('Pyramids stored twice', counted(stats.superseded));
    if (report.cache.db_version) fact('Catalog format', report.cache.db_version);
    if (report.cache.crs_version) fact('Process version', report.cache.crs_version);
    fact(
      'Copy verified',
      report.source_sha256 && report.source_sha256 === report.snapshot_sha256
        ? `yes — ${report.source_sha256.slice(0, 12)}… both times`
        : 'the copy was not hashed',
    );
    if (stats.original_present > 0) {
      fact(
        'Originals still where the catalog says',
        `${counted(stats.original_present)} — those need no recovery`,
      );
    }

    // The folders the catalog recorded. A page has no way to see a drive unless it
    // is handed it, so a folder that is not connected is a thing to fix, not a
    // fault.
    rootsSection.hidden = report.roots.length === 0;
    for (const root of report.roots) {
      const row = document.createElement('li');
      const path = document.createElement('code');
      path.textContent = root.path;
      row.append(path, document.createTextNode(` — ${counted(root.images)} images, `));
      if (root.in_reach) {
        row.append(document.createTextNode('available from this page'));
      } else {
        row.append(document.createTextNode('not connected from this page'));
        const again = document.createElement('button');
        again.type = 'button';
        again.textContent = 'Check again…';
        again.addEventListener('click', () => void recheck(root.path));
        row.append(document.createTextNode(' '), again);
      }
      roots.append(row);
    }
    scanButton.disabled = false;
    void chosen;
  }

  async function recheck(rootPath) {
    if (exports.busy()) return;
    const answer = await pickFolder({ id: 'lrsalvage-originals', mode: 'read' });
    if (answer.cancelled) {
      say(pickerError, 'Nothing was chosen, so the catalog still cannot see that folder.');
      return;
    }
    if (answer.refused) {
      say(pickerError, `This browser refused the folder: ${answer.refused.message}`);
      return;
    }
    // Mount the folder where the catalog says the originals were, and scan again:
    // the seam's namespace now answers for that path, so the same question the
    // desktop asks gets the desktop's answer (`#149`, DEC-S7).
    await runScan(withMount(state.source, rootPath, answer.handle));
  }

  // -- small pieces -------------------------------------------------------

  function showOnly(section) {
    for (const node of [gate, source, scanning, result]) node.hidden = node !== section;
  }

  function fact(label, value) {
    const term = document.createElement('dt');
    term.textContent = label;
    const detail = document.createElement('dd');
    detail.textContent = value;
    checked.append(term, detail);
  }

  function say(node, text) {
    node.textContent = text;
    node.hidden = false;
  }

  function line(lead, text) {
    const paragraph = document.createElement('p');
    if (lead) {
      const strong = document.createElement('strong');
      strong.textContent = lead;
      paragraph.append(strong);
    }
    paragraph.append(document.createTextNode(text));
    return paragraph;
  }
}

/**
 * One sentence per stage-and-class the Rust half can report.
 *
 * The classes are `Failure::of`'s (`crates/lrsalvage-app/src/lib.rs`), and the
 * sentence says what a person can *do*; the raw message is kept behind the
 * details toggle for whoever is helping them.
 */
function failureSentence(stage, kind) {
  if (stage === 'cache-walk') {
    return 'The app could not read the entire preview cache. Check access to its folders and try again.';
  }
  if (stage === 'cache-index') {
    return {
      missing: 'This folder has no previews.db. Choose the preview cache folder that contains it.',
      unreadable: 'The preview index could not be read. Try another copy of the preview cache.',
      unsupported: 'This preview index has an unsupported schema.',
    }[kind];
  }
  if (kind === 'catalog-changed') {
    return (
      'The catalog changed while it was being copied, so the copy is not the catalog. ' +
      'Close Lightroom, or copy the catalog to another folder first, and try again.'
    );
  }
  if (kind === 'not-a-catalog') {
    return (
      'That file is not a Lightroom catalog. Choose the folder that holds the catalog — ' +
      'the file ending in .lrcat.'
    );
  }
  if (kind === 'out-of-room' || kind === 'unsupported') {
    return (
      'This browser’s storage cannot hold a copy of that catalog. Free some space in the ' +
      'browser, or scan a smaller catalog.'
    );
  }
  if (kind === 'not-found') {
    return 'A file or folder this run needed is gone. Check that the drive is still connected.';
  }
  if (stage === 'open' || stage === 'copy') {
    return (
      'This browser could not open that catalog. It may be damaged, or it may not be a ' +
      'Lightroom catalog at all.'
    );
  }
  if (stage === 'index') {
    return 'The preview index could not be copied into this browser’s storage.';
  }
  return 'The scan stopped with an error.';
}
