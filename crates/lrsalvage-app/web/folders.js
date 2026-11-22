// Which folder beside a catalog is its preview cache.
//
// One pure answer over names, because this is the rule that was wrong: the
// folder listing collects every `.lrdata` and the suggestion used to be a
// prefix match, so `2023 Helper.lrdata` — which shares the catalog's stem —
// sorted first and won (`#191`). A name list in, a name out, so
// `scripts/check-js.mjs` can hold the rule without a browser.
//
// Adobe writes up to three siblings beside `<catalog>.lrcat`:
//
//   <catalog> Previews.lrdata        the standard previews this app reads
//   <catalog> Smart Previews.lrdata  DNG-based smart previews, no `previews.db`
//   <catalog> Helper.lrdata          Lightroom's own helper caches
//
// Only the first carries the index `previews_db` reads, so the other two are
// never a candidate — not even when one is the only `.lrdata` in the folder.

/** The folders Adobe writes beside a catalog that are never a preview cache. */
const COMPANION = [/ smart previews\.lrdata$/i, / helper\.lrdata$/i];

/** Whether a `.lrdata` name is one of Lightroom's companion folders. */
export function isCompanionCache(name) {
  return COMPANION.some((pattern) => pattern.test(name));
}

/**
 * The cache folder a listing suggests for a catalog, or `null` when the listing
 * does not name one.
 *
 * The exact `<stem> Previews.lrdata` wins. Failing that, a single remaining
 * candidate is still offered — a renamed or copied backup is a real case — and
 * anything more ambiguous suggests nothing, because a guess here is a scan of a
 * folder the user did not mean.
 */
export function suggestedCache(names, catalog = null) {
  const candidates = names.filter((name) => !isCompanionCache(name));
  if (catalog) {
    const stem = catalog.replace(/\.lrcat$/i, '');
    const standard = `${stem} Previews.lrdata`.toLowerCase();
    const exact = candidates.find((name) => name.toLowerCase() === standard);
    if (exact) return exact;
    // A `Smart Previews` folder is excluded above, so a stem match left over is
    // a cache with a suffix of its own (`2023 (2) Previews.lrdata`); it is not
    // this catalog's cache and is not preferred over an unstemmed single one.
  }
  return candidates.length === 1 ? candidates[0] : null;
}
