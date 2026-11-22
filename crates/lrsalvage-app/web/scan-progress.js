// What the scanning screen shows while a scan runs, and when it says so.
//
// `plan::Progress` arrives as a phase number and a count (§185), and this module
// is the only place that turns one into the four things a person reads: the stage
// sentence, the processed count and percentage, the estimate, and the live-region
// announcement. `view.js` owns the rest of the page and calls this; the decisions
// it makes are `specs/app.md`'s, *Matching progress measures catalog images*.
//
// Times are the worker's monotonic milliseconds, sent with each report. A report
// painted late still measures the work rather than the paint, because the worker
// at most paints ten times a second and the page decodes nothing — an estimate
// built from arrival times would be an estimate of the DOM.
import { PHASES } from './app.js';

/**
 * The stage that is matching, named rather than positional: it is the one stage
 * whose end is the end of the run's work, so what follows it is preparation.
 */
const MATCHING = 'images';

/** No estimate before this much matching: one sample is not a rate. */
const WARM_UP_MS = 3000;
/** The window the recent rate is measured over. */
const RECENT_MS = 4000;
/** A gap this long means the rate history describes work that has stopped. */
const STALL_MS = 2000;

const CALCULATING = 'Calculating time remaining…';
const INSTALL = 'Preparing this browser’s storage…';
const COPY =
  'Copying the catalog into this browser’s storage. This takes a few seconds ' +
  'for a large catalog, and it cannot be stopped part-way.';

/** Two rates this far apart describe different work, so neither projects the rest. */
function comparable(first, second) {
  return first > 0 && second > 0 && Math.max(first, second) <= 2 * Math.min(first, second);
}

/**
 * The recent window's own steadiness, before the window is long enough to speak
 * for the stage: an early half and a late half that disagree mean the rate is
 * still moving, and a projection from either half is a guess shown as a number.
 */
function steady(samples) {
  const first = samples[0];
  const last = samples.at(-1);
  const middle = samples.find((sample) => sample.at >= (first.at + last.at) / 2);
  if (!middle || middle === first || middle === last) return false;
  const early = (middle.done - first.done) / (middle.at - first.at);
  const late = (last.done - middle.done) / (last.at - middle.at);
  return comparable(early, late);
}

/**
 * Seconds of matching left, or `null` while the reports cannot support one.
 *
 * The projection is the stage's **average** throughput, because that is what a
 * finished scan measured and a slowdown early in a run does not undo the work it
 * did. The recent rate is a check on the average, never the estimator: two rates
 * more than a factor of two apart mean the workload changed shape — a sustained
 * slowdown, a cache that started answering, a stall — and a number would then be
 * a guess with a unit after it. `#185` measured both halves of that trade: a
 * three-second window projected about eight minutes where 113 seconds remained,
 * and projecting upwards every time the window moved on revised the displayed
 * estimate 55 times in one run.
 */
export function estimateSeconds(samples, started, remaining) {
  if (!started || samples.length < 2) return null;
  const first = samples[0];
  const last = samples.at(-1);
  const elapsed = last.at - started.at;
  const recent = last.at - first.at;
  if (elapsed < WARM_UP_MS || recent <= 0) return null;
  const stageRate = (last.done - started.done) / elapsed;
  const recentRate = (last.done - first.done) / recent;
  if (elapsed < RECENT_MS && !steady(samples)) return null;
  if (!comparable(stageRate, recentRate)) return null;
  return remaining / stageRate / 1000;
}

/**
 * A duration as a sentence, rounded to the precision the estimate has: the rate
 * is an average over seconds of work, so single seconds and single minutes are
 * both finer than the measurement.
 */
export function approximate(seconds) {
  if (seconds < 5) return 'A few seconds remaining';
  const rounded = 5 * Math.round(seconds / 5);
  if (rounded < 60) return `About ${rounded} seconds remaining`;
  const minutes = Math.round(seconds / 60);
  return `About ${minutes} ${minutes === 1 ? 'minute' : 'minutes'} remaining`;
}

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
  /** Matching reports inside the recent window, and the first of the run. */
  let samples = [];
  let started = null;
  /** Fires when no report has arrived for a stall. */
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
    samples = [];
    started = null;
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
   */
  function preparing() {
    const armed = generation;
    completion = afterPaint().then(() => {
      if (armed !== generation) return;
      enter('preparing', 'Preparing scan results…');
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
    /** Every paragraph that is not a stage: the practice catalog's own load. */
    notice: (label) => enter('notice', label),
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
        setText(count, `${done.toLocaleString()} files checked`);
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
      const counts = `${done.toLocaleString()} of ${total.toLocaleString()}`;
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
      const sample = { done, at };
      const previous = samples.at(-1);
      if (previous && at - previous.at >= STALL_MS) {
        samples = [];
        started = null;
      }
      started ??= sample;
      samples.push(sample);
      while (samples.length > 2 && samples[1].at <= at - RECENT_MS) samples.shift();

      const seconds = estimateSeconds(samples, started, total - done);
      const time = seconds === null ? CALCULATING : approximate(seconds);
      eta.hidden = false;
      setText(eta, time);
      timer = setTimeout(() => {
        timer = null;
        samples = [];
        started = null;
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
