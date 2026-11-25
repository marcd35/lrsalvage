// The run's diagnostics: one log, one frozen object, one clipboard write.
//
// The field list is frozen in `specs/app.md` (*Receipt, reports and diagnostics*),
// and it is curated here rather than filtered later: no member of this object can
// hold a path, a filename, an identifier, an error message or a stack, so a copy
// has nothing to redact and nobody has to remember what to remove. That is
// ADR 0007's discipline applied to the second artifact that leaves a machine.
//
// The log starts when this module loads, not when a receipt appears — a failure
// at the catalog copy has no receipt, and it is exactly the failure somebody
// wants to hand over. Nothing is written to a destination, nothing is uploaded,
// and the only exit is a click on **Copy diagnostics**.
//
// Every timing is milliseconds *since the page loaded*, so the object describes a
// run's shape; there is deliberately no absolute timestamp, because when its
// author was at their desk is not a fact this file needs.

/** The page's own origin in time. `performance.now()` is monotonic. */
const STARTED = performance.now();

/** Milliseconds since the page loaded, rounded. */
const at = () => Math.round(performance.now() - STARTED);

/** The eighteen `stats` members `specs/modules/report.md` freezes, in its order. */
const SCAN_COUNTS = [
  'images', 'recovered', 'suspect', 'missing_payload', 'no_cache_entry', 'unmapped',
  'failed', 'original_present', 'below_floor', 'above_cap', 'bytes', 'levels',
  'selected_items', 'selected_images', 'tiers', 'superseded', 'deleted', 'unrecognised',
];

const CACHE_SCAN_COUNTS = ['indexed', 'recoverable', 'missing', 'invalid', 'unsupported'];

/** The four facts an indexless run measures, in `report.md`'s order. */
const CONTAINER_SCAN_COUNTS = ['containers', 'usable', 'invalid', 'unsupported'];

/** The seven categories a failed write is counted under, aimed at a person. */
const FAILURE_CATEGORIES = [
  'destination_gone', 'permission', 'out_of_room', 'occupied_foreign',
  'source_unreadable', 'write_failed', 'other',
];

/**
 * The browser and platform as the browser states them.
 *
 * `userAgentData` is Chromium's; the page already refuses every other engine, so
 * the user-agent fallback is for a Chromium that predates the API rather than for
 * a second family. `platformVersion` is a UniversalApiContract number — reported
 * verbatim, because a table mapping it to a marketing name is a table that rots.
 */
const environment = (async () => {
  const data = navigator.userAgentData;
  try {
    if (data && typeof data.getHighEntropyValues === 'function') {
      const values = await data.getHighEntropyValues(['fullVersionList', 'platform', 'platformVersion']);
      const list = Array.isArray(values.fullVersionList) ? values.fullVersionList : [];
      const brand = ['Microsoft Edge', 'Google Chrome', 'Chromium']
        .map((name) => list.find((entry) => entry.brand === name))
        .find(Boolean);
      return {
        browser: brand ? `${brand.brand} ${brand.version}` : null,
        platform: values.platform ?? null,
        platform_version: values.platformVersion ?? null,
      };
    }
  } catch {
    // An API that refuses is the same as one that is missing.
  }
  const agent = navigator.userAgent ?? '';
  const match = agent.match(/(Edg|Chrome|Chromium)\/(\d+(?:\.\d+)*)/);
  const name = match ? (match[1] === 'Edg' ? 'Microsoft Edge' : 'Chrome') : null;
  return {
    browser: match ? `${name} ${match[2]}` : null,
    platform: /Windows/.test(agent) ? 'Windows' : null,
    platform_version: null,
  };
})();

/** `build.json` when the static builder wrote one, so a built page names its commit. */
const build = (async () => {
  try {
    const response = await fetch(new URL('./build.json', import.meta.url), { cache: 'no-store' });
    if (!response.ok) return null;
    const identity = await response.json();
    return typeof identity?.commit === 'string' ? identity : null;
  } catch {
    return null;
  }
})();

function blank(mode = 'catalog') {
  return {
    mode,
    marks: {},
    scan: null,
    // The counts of the modes that measured no catalog, present on every copy so
    // a reader keys on the same names whichever run produced it. An indexless
    // run counts containers rather than pyramids, so its four names are the
    // ones at `null` on its copy (`specs/app.md`).
    cacheScan: Object.fromEntries(
      (mode === 'legacy' ? CONTAINER_SCAN_COUNTS : CACHE_SCAN_COUNTS)
        .map((name) => [name, null]),
    ),
    writes: { planned: 0, written: 0, already_recovered: 0, failed: 0, confirmed_on_disk: null },
    events: { stopped: 0, settled: 0, continued: 0 },
    failures: Object.fromEntries(FAILURE_CATEGORIES.map((name) => [name, 0])),
    failure: null,
    commits: [],
    versions: { app: null, core: null },
  };
}

let state = blank();

/**
 * The current run's log.
 *
 * One `state` object, replaced by `run()`; every other method writes into it. The
 * call sites are the flow's own (`view.js`, `export.js`, `review.js`), so nothing
 * here has to guess what happened.
 */
export const log = {
  /** A new run begins: the receipt's counters start again, the versions do not. */
  run(mode = 'catalog') {
    const versions = state.versions;
    state = blank(mode);
    state.versions = versions;
    state.cacheOnly = mode !== 'catalog';
  },

  /**
   * One stage of the entry, as `app.js` reports it: `install`, `copy`, then
   * `scan` on the first unit and `report` when the plan arrives.
   *
   * The copy, the open and the index copy are one synchronous wasm frame with no
   * clock inside it, so `catalog_copy_ms` spans all three and stops at the first
   * unit the plan reports.
   */
  stage(name) {
    const now = at();
    state.marks[name] = now;
    if (name === 'copy' && state.marks.install !== undefined) {
      state.marks.storage_setup = now - state.marks.install;
    }
    if (name === 'scan' && state.marks.copy !== undefined) {
      state.marks.catalog_copy = now - state.marks.copy;
    }
    if (name === 'report' && state.marks.copy !== undefined) {
      const begin = state.marks.scan ?? state.marks.copy;
      state.marks.scan_ms = now - begin;
    }
  },

  /** The first thumbnail this page decoded, which is when review became usable. */
  thumbnail() {
    state.marks.first_thumbnail ??= at();
  },

  /** The two compiled release numbers, once the worker has loaded them. */
  versions(versions) {
    state.versions = { app: versions?.app ?? null, core: versions?.core ?? null };
  },

  /** The scan's frozen counts, copied member by member and by name.
   *
   * Each mode fills its own object and leaves the other's `null`: a cache-only
   * scan measures five facts about pyramids, an indexless one four facts about
   * containers, and neither measures any of the catalog's eighteen. Giving one
   * set the other's names would be the second vocabulary `specs/app.md` forbids.
   */
  scanCounts(stats) {
    if (!stats) return;
    const fields = state.mode === 'legacy'
      ? CONTAINER_SCAN_COUNTS
      : state.cacheOnly ? CACHE_SCAN_COUNTS : SCAN_COUNTS;
    const counts = Object.fromEntries(fields.map((name) => [name, stats[name] ?? null]));
    if (state.cacheOnly) state.cacheScan = counts;
    else state.scan = counts;
  },

  /** How many files the batch fixed when it started. */
  planned(count) {
    state.writes.planned = count;
  },

  /** One file's write finished, whatever it decided. */
  commit(status) {
    if (status === 'written') {
      state.writes.written++;
      state.commits.push(at());
      state.marks.first_saved_jpeg ??= at();
    } else if (status === 'skipped') {
      state.writes.already_recovered++;
    } else {
      state.writes.failed++;
    }
  },

  /** One failed write, by category — never by message. */
  failure(category) {
    const name = FAILURE_CATEGORIES.includes(category) ? category : 'other';
    state.failures[name]++;
  },

  /** A failure that stopped the run before any receipt: a stage and a kind. */
  viewFailure(stage, kind) {
    // The counts in `failures` are file work; a scan that never reached a file is
    // one fact about the run, not a category of write that failed.
    state.failure = { stage, kind };
  },

  /** How many of the committed files the destination answered for. */
  confirmed(count) {
    state.writes.confirmed_on_disk = count;
  },

  stopped() {
    state.events.stopped++;
  },

  settled() {
    state.events.settled++;
  },

  continued() {
    state.events.continued++;
  },

  /** The app and core versions, from the module when it has loaded, else the build. */
  async identity() {
    const written = await build;
    return {
      app: state.versions.app ?? written?.app ?? null,
      core: state.versions.core ?? written?.core ?? null,
      build: written?.commit ?? null,
    };
  },

  /** The frozen object, as the text a person copies. */
  async text() {
    const [env, identity] = await Promise.all([environment, log.identity()]);
    const gap = () => {
      let longest = 0;
      for (let index = 1; index < state.commits.length; index++) {
        longest = Math.max(longest, state.commits[index] - state.commits[index - 1]);
      }
      return longest;
    };
    const marks = state.marks;
    const object = {
      schema: 'lrsalvage.diagnostics',
      mode: state.mode,
      versions: {
        app: identity.app,
        core: identity.core,
        build: identity.build,
        browser: env.browser,
        platform: env.platform,
        platform_version: env.platform_version,
      },
      timings: {
        storage_setup_ms: marks.storage_setup ?? null,
        catalog_copy_ms: state.cacheOnly ? null : marks.catalog_copy ?? null,
        scan_ms: marks.scan_ms ?? null,
        first_thumbnail_ms: marks.first_thumbnail ?? null,
        first_saved_jpeg_ms: marks.first_saved_jpeg ?? null,
        longest_write_gap_ms: state.commits.length > 1 ? gap() : null,
        elapsed_ms: at(),
      },
      scan: state.cacheOnly ? null : state.scan,
      cache_scan: state.cacheScan,
      writes: { ...state.writes },
      events: { ...state.events },
      failures: { ...state.failures },
      failure: state.failure,
    };
    return JSON.stringify(object, null, 2);
  },

  /** Put the object on the clipboard, and say so where the page can hear it. */
  async copy(status) {
    await log.copyText(
      await log.text(),
      status,
      'Diagnostics copied. They name no path, no file and no error text.',
    );
  },

  /** Copy one short string — the folder's name — for the same reason. */
  async copyText(text, status, done) {
    try {
      await navigator.clipboard.writeText(text);
      if (status) status.textContent = done;
    } catch (error) {
      if (status) status.textContent = `The clipboard was refused: ${error.message}`;
    }
  },
};
