// Sizes a person reads, in the units a disk is sold in.
//
// One module because the number is shown in two places — the planned batch's
// preview total and the receipt — and a second rounding rule would drift the way
// every duplicate does. Decimal, not binary: `1 MB` is `1 000 000` bytes, which
// is what a drive's capacity means and what File Explorer means by MB (`#192`).
//
// Two decimal places at every unit, because the interesting part of a batch total
// is the third digit of a figure in the hundreds.

/**
 * One size, as text.
 *
 * Below a kilobyte the exact count is the honest reading, since rounding 900
 * bytes to `0.90 kB` throws away the only significant digit it has.
 */
export function formatBytes(bytes) {
  const value = Number.isFinite(bytes) && bytes > 0 ? bytes : 0;
  if (value < 1_000) return `${Math.round(value)} B`;
  if (value < 1_000_000) return `${(value / 1_000).toFixed(2)} kB`;
  if (value < 1_000_000_000) return `${(value / 1_000_000).toFixed(2)} MB`;
  return `${(value / 1_000_000_000).toFixed(2)} GB`;
}
