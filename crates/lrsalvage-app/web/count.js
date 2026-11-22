// One spelling of a count.
//
// Every number the page prints for a person goes through here, so 100078 reads
// as a number in the stage readout, the result screen and the receipt alike —
// and a change to how a count is written is one edit rather than three that
// drift apart.

/** A count with its thousands separators, so 100078 reads as a number. */
export function counted(value) {
  return typeof value === 'number' ? value.toLocaleString('en') : String(value ?? 0);
}
