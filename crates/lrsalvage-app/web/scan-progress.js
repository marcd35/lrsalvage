// What the scanning screen shows while a scan runs, and when it says so.
//
// `plan::Progress` arrives as a phase number and a count (#185), and this module
// is the only place that turns one into the four things a person reads: the stage
// sentence, the processed count and percentage, the estimate, and the live-region
// announcement. `view.js` owns the rest of the page and calls this; the decisions
// it makes are `specs/app.md`'s, *Matching progress measures catalog images*. The
// estimate's arithmetic is `rate.js`'s, so a recorded run can replay it.
import { PHASES } from './app.js';
import { counted } from './count.js';
import { CALCULATING, STALL_MS, approximate, createRate } from './rate.js';

/**
 * The stage that is matching, named rather than positional: it is the one stage
 * whose end is the end of the run's work, so what follows it is preparation.
 */
const MATCHING = 'images';

const INSTALL = 'Preparing this browser’s storage…';
const COPY =
  'Copying the catalog into this browser’s storage. This takes a few seconds ' +
  'for a large catalog, and it cannot be stopped part-way.';

/**
 * The scanning section's readout, bound to the page's own nodes.
 *
 * One instance lives as long as the page does (`view.js` mounts it), and every
 * run begins with `reset()`, so nothing carries between two scans.
 */
export function createScanProgress(document) {
  const node = (id) => {
    const found = document.getElementById(id);
    if (!found) throw new Error(`the scanning section is missing #${id}`);
    return found;
  };
  const stage = node('stage');
  const count = node('count');
  const bar = node('bar');
  const eta = node('eta');
  const status = node('scan-status');
  const stop = node('stop');

  /** The stage on screen: the identity a report is compared against. */
  let phase = null;
  /** The matching reports inside the recent window, and the stall timer. */
  const rate = createRate();
  let timer = null;
  /** The last quarter announced, and whether the first estimate has been. */
  let milestone = 0;
  let announcedEstimate = false;
  /** A stop was asked for: the readout is the stop, and reports no longer land. */
  let stopping = false;
  /** The paint the completion transition waits for. */
  let completion = null;
  /** Bumped by every clear, so a transition armed before it cannot land after. */
  let generation = 0;

  function setText(target, text) {
    // Writing the same string again is a DOM mutation a screen reader has to
    // decide about, and a MutationObserver-observed test sees a change that is
    // not one.
    if (target.textContent !== text) target.textContent = text;
  }

  function clear() {
    generation += 1;
    clearTimeout(timer);
    timer = null;
    rate.reset();
    milestone = 0;
    announcedEstimate = false;
    completion = null;
    bar.hidden = false;
    bar.removeAttribute('value');
    bar.removeAttribute('max');
    count.textContent = '';
    eta.hidden = true;
    eta.textContent = '';
  }

  /**
   * Show `label` as the stage, with the readout emptied. A stage with no unit
   * inside it is named and offers no stop (`specs/app.md`).
   */
  function enter(name, label, cancellable = false) {
    if (phase === name) return;
    clear();
    phase = name;
    setText(stage, label);
    setText(status, label);
    stop.disabled = !cancellable;
  }

  /**
   * One browser frame on which the readout has been painted, or nothing when the
   * page is not visible — a hidden page has no frame to wait for.
   */
  function afterPaint() {
    if (document.hidden) return Promise.resolve();
    return new Promise((resolve) => {
      function finish() {
        document.removeEventListener('visibilitychange', finish);
        resolve();
      }
      document.addEventListener('visibilitychange', finish, { once: true });
      requestAnimationFrame(() => requestAnimationFrame(finish));
    });
  }

  /**
   * Matching is over and everything left is result preparation: plan the
   * transition for after the completion has had a frame, so a person sees the
   * full count and the 100% rather than a screen that skipped them.
   *
   * The live region keeps the completion sentence it was just given. Preparation
   * is the absence of units rather than another milestone, and a second
   * announcement in the same breath is what makes a screen reader say neither.
   */
  function preparing() {
    const armed = generation;
    completion = afterPaint().then(() => {
      if (armed !== generation) return;
      clear();
      phase = 'preparing';
      setText(stage, 'Preparing scan results…');
      stop.disabled = true;
    });
    return completion;
  }

  return {
    /**
     * Begin a run, saying `label` while it waits. The install and the copy have
     * no unit inside them, so Stop is offered and disabled rather than hidden.
     */
    reset(label = INSTALL) {
      phase = null;
      stopping = false;
      enter('start', label);
    },
    /** The copy is the one stage this page names from the worker's own message. */
    stage(name) {
      if (stopping) return;
      if (name === 'copy') enter('copy', COPY);
    },
    /** The stop a person asked for, drawn at once: the count and estimate are gone. */
    stop() {
      stopping = true;
      enter('stopping', 'Stopping…');
      bar.hidden = true;
    },
    /** The paint a completed scan must have shown before its result replaces it. */
    painted: () => completion,
    clear,
    progress({ phase: number, done, total, at }) {
      // A stop clears the readout and a queued report must not repaint it. The
      // report itself is not a progress report and still opens its result.
      if (stopping) return;
      const name = PHASES[number] ?? 'cache';
      if (name === 'cache') {
        enter(name, 'Reading the preview folder…', true);
        setText(count, `${counted(done)} files checked`);
        return;
      }
      if (name === MATCHING && total === 0) {
        // Nothing to match, so there is no range and no estimate to show.
        enter(name, 'No catalog images to match');
        bar.hidden = true;
        preparing();
        return;
      }

      const matching = name === MATCHING;
      enter(name, matching ? 'Matching catalog images…' : 'Reading preview files…', true);
      const percent = done === total ? 100 : Math.min(99, Math.floor((100 * done) / total));
      const counts = `${counted(done)} of ${counted(total)}`;
      setText(count, matching ? `${counts} (${percent}%)` : counts);
      if (total > 0) {
        bar.max = total;
        bar.value = done;
      }
      if (!matching) return;

      if (done === total) {
        // The last image is decided: the range is full, there is nothing left to
        // estimate, and there is nothing left to stop.
        eta.hidden = true;
        eta.textContent = '';
        stop.disabled = true;
        setText(status, `Catalog image matching complete. ${counts} (100%).`);
        preparing();
        return;
      }

      if (timer !== null) clearTimeout(timer);
      const seconds = rate.add(done, total, at);
      const time = seconds === null ? CALCULATING : approximate(seconds);
      eta.hidden = false;
      setText(eta, time);
      timer = setTimeout(() => {
        timer = null;
        rate.stall();
        setText(eta, CALCULATING);
      }, STALL_MS);

      // Quarters, the first estimate, and the end: a person who cannot see the
      // bar hears the stage move four times rather than every report.
      const quarter = Math.floor(percent / 25);
      if (quarter > milestone || (seconds !== null && !announcedEstimate)) {
        milestone = quarter;
        announcedEstimate ||= seconds !== null;
        setText(status, `Matching catalog images. ${count.textContent}. ${time}`);
      }
    },
  };
}
