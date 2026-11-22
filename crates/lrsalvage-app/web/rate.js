// The one algorithm behind the estimate, with no DOM in it.
//
// Two callers run it: the page, to draw a sentence, and
// `scripts/scan-progress-trace.mjs`, to say what a recorded run displayed. A
// second copy of these rules would drift silently, and the drift would show up
// as an estimate rather than as a failure — so the arithmetic lives here and the
// page keeps only the nodes.
//
// Times are the worker's monotonic milliseconds, sent with each report. A report
// painted late still measures the work rather than the paint.

/** No estimate before this much matching: one sample is not a rate. */
export const WARM_UP_MS = 3000;
/** The window the recent rate is measured over. */
export const RECENT_MS = 4000;
/** A gap this long means the rate history describes work that has stopped. */
export const STALL_MS = 2000;

export const CALCULATING = 'Calculating time remaining…';

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

/** The seconds a shown sentence reports, so two sentences compare by value. */
export function secondsOf(text) {
  const minutes = /^About (\d+) minutes? remaining$/.exec(text);
  if (minutes) return Number(minutes[1]) * 60;
  const seconds = /^About (\d+) seconds remaining$/.exec(text);
  if (seconds) return Number(seconds[1]);
  if (text === 'A few seconds remaining') return 5;
  return null;
}

/**
 * The sampling state behind the estimate: what a run of matching reports says,
 * and when it stops saying anything.
 *
 * Separate from the drawing so a recorded run can be replayed without a page.
 */
export function createRate() {
  let samples = [];
  let started = null;
  const reset = () => {
    samples = [];
    started = null;
  };
  return {
    reset,
    /** One matching report; the seconds left, or `null` while it cannot know. */
    add(done, total, at) {
      const previous = samples.at(-1);
      if (previous && at - previous.at >= STALL_MS) reset();
      const sample = { done, at };
      started ??= sample;
      samples.push(sample);
      while (samples.length > 2 && samples[1].at <= at - RECENT_MS) samples.shift();
      return estimateSeconds(samples, started, total - done);
    },
    /** The stall timer fired: the history describes a pause, not a rate. */
    stall: reset,
  };
}
