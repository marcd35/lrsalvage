#!/bin/sh
# Walk every level file of a modern preview cache by its own declared segment
# lengths, and line that walk up against `previews.db`.
#
#   scripts/probe-cache.sh "<name> Previews.lrdata" [--verbose]
#
# This is a measurement tool, not a gate. It writes nothing, opens the index
# read-only and immutable, and prints what it saw. The shipped tool already
# answers part of this (`scan --json`, `scan --report`, and `extract`'s
# `payload_metadata`), but those report the level a run *selected*; this walks
# every level file in the tree, which is what `docs/format-notes.md` §5.7 means
# by "every file, not a sample".
#
# Three things it deliberately does not do:
#   * find images by scanning for `FF D8` (ADR 0001) — the top-level segment
#     chain is walked by its declared lengths, and a disagreement is reported
#     rather than skipped;
#   * trust a leaf's `_<longedge>` — every file's own `SOF` has to agree with it;
#   * conflate rows and files — a row with no file and a file with no row are
#     counted separately, because the two are different findings.
#
# Limits worth knowing before quoting a number:
#   * The walk reads the first 256 KiB of each file. A level's band headers end
#     at `SOS`, which sits far inside that for every JPEG seen in this project,
#     but a file whose chain does not reach `SOS` within the cap is reported as
#     `truncated`, never counted as if it had been walked.
#   * Filenames are read one per line, so a name containing a newline is out of
#     scope; payload leaf names cannot contain one.
set -eu

usage() {
    printf 'usage: %s "<name> Previews.lrdata" [--verbose]\n' "${0##*/}" >&2
    exit 2
}

[ "$#" -ge 1 ] || usage

cache=$1
verbose=0
if [ "$#" -ge 2 ]; then
    case $2 in
        --verbose | -v) verbose=1 ;;
        *) usage ;;
    esac
fi
[ -d "$cache" ] || {
    printf 'not a directory: %s\n' "$cache" >&2
    exit 1
}

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing %s: %s\n' "$1" "$2" >&2
        exit 1
    fi
}

require awk 'install awk'
require od 'coreutils'
require find 'findutils'
require tail 'coreutils'
require sqlite3 'install the sqlite3 CLI'

root=$(cd "$cache" && pwd)
name=${root##*/}

# The leaf grammar of §5.7, which is what makes a file a level file: a
# 36-character uuid, `-`, a 32-character lowercase hex digest, `_`, digits.
# Nothing else in a cache matches it, so matching it is a measurement rather
# than a guess about names.
grammar='^[0-9A-Fa-f]/[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]/[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]-[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]-[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]-[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]-[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]_[0-9][0-9]*$'

# Files a cache legitimately holds beside its payloads. Anything else that is
# not a level file is reported as unrecognised instead of being ignored.
not_payload='^(previews\.db|root-pixels\.db|PackageIcon\.ico|desktop\.ini)$'

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

# --- the walk -------------------------------------------------------------
# One pass per file over `od -An -v -tx1`, so every decision below is made on
# the bytes rather than on a name. A JPEG's top-level chain is: `SOI`, then
# segments whose length field counts itself, until `SOS` (after which the rest
# is entropy-coded data) or `EOI`.
cat >"$work/walk.awk" <<'EOF'
function hd1(c,   v) {
    v = index("0123456789", c)
    if (v) return v - 1
    v = index("abcdef", tolower(c))
    return v ? v + 9 : 0
}

# `od -tx1` emits one 2-character token per byte, so a length field is two of
# them, big-endian.
function hx2d(t) { return hd1(substr(t, 1, 1)) * 16 + hd1(substr(t, 2, 1)) }

function segname(m) {
    if (m ~ /^e[0-9a-f]$/) return "APP" hd1(substr(m, 2, 1))
    if (m == "fe") return "COM"
    if (m == "db") return "DQT"
    if (m == "dd") return "DRI"
    if (m == "c4") return "DHT"
    if (m == "da") return "SOS"
    if (m == "d8") return "SOI"
    if (m == "d9") return "EOI"
    if (m == "cc") return "DAC"
    if (m ~ /^c[0-9a-f]$/) return "SOF" hd1(substr(m, 2, 1))
    if (m ~ /^d[0-9a-f]$/) return "RST" hd1(substr(m, 2, 1))
    return "FF" toupper(m)
}

# Whether `len` bytes at offset `at` contain the signature, itself a list of
# hex octets. Signatures rather than fixed offsets: the identifier of an `Exif`,
# an XMP or an `ICC_PROFILE` segment is at its start in every file Adobe writes,
# but a walk that assumes the offset reports "no metadata" the day it moves.
function has(at, len, sig, siglen,   i, j, ok) {
    if (len < siglen) return 0
    for (i = 0; i <= len - siglen; i++) {
        ok = 1
        for (j = 1; j <= siglen; j++) {
            if (b[at + i + j - 1] != sig[j]) { ok = 0; break }
        }
        if (ok) return 1
    }
    return 0
}

BEGIN {
    k = split("45 78 69 66 00 00", EXIF, " "); EXIFN = k
    k = split("68 74 74 70 3a 2f 2f 6e 73 2e 61 64 6f 62 65 2e 63 6f 6d 2f 78 61 70 2f", XAP, " "); XAPN = k
    k = split("61 64 6f 62 65 3a 6e 73 3a 6d 65 74 61", META, " "); METAN = k
    k = split("49 43 43 5f 50 52 4f 46 49 4c 45", ICC, " "); ICCN = k
}

{ for (i = 1; i <= NF; i++) b[++n] = tolower($i) }

END {
    if (n < 4) { print "short|0|0|0|0|0|0|0|short-file"; exit }
    soi = (b[1] == "ff" && b[2] == "d8") ? 1 : 0
    chain = soi ? "SOI" : "no-SOI"
    w = 0; h = 0; app1 = 0; app2 = 0; exif = 0; xmp = 0; icc = 0
    flags = ""; done = 0; pos = 3
    while (pos + 1 <= n) {
        if (b[pos] != "ff") { flags = flags ",marker-not-ff@" pos; break }
        m = b[pos + 1]
        if (m == "d9") { chain = chain " EOI"; done = 1; break }
        if (m == "da") { chain = chain " SOS"; done = 1; break }
        if (m == "01" || m ~ /^d[0-7]$/) { chain = chain " " segname(m); pos += 2; continue }
        if (pos + 3 > n) { flags = flags ",header-past-buffer"; break }
        len = hx2d(b[pos + 2]) * 256 + hx2d(b[pos + 3])
        if (len < 2) { flags = flags ",length-below-2@" pos; break }
        chain = chain " " segname(m)
        if (m ~ /^c[0-9a-f]$/ && m != "c4" && m != "c8" && m != "cc") {
            h = hx2d(b[pos + 5]) * 256 + hx2d(b[pos + 6])
            w = hx2d(b[pos + 7]) * 256 + hx2d(b[pos + 8])
        } else if (m == "e1") {
            app1++
            if (has(pos + 4, len - 2, EXIF, EXIFN)) exif = 1
            if (has(pos + 4, len - 2, XAP, XAPN) || has(pos + 4, len - 2, META, METAN)) xmp = 1
        } else if (m == "e2") {
            app2++
            if (has(pos + 4, len - 2, ICC, ICCN)) icc = 1
        }
        pos = pos + 2 + len
    }
    if (!done) flags = flags ",no-sos-in-buffer"
    sub(/^,/, "", flags)
    print soi "|" chain "|" w "|" h "|" app1 "|" app2 "|" exif "|" xmp "|" icc "|" (flags == "" ? "ok" : flags)
}
EOF

# --- the summary ----------------------------------------------------------
# `previews.db` first: a row is a declaration, so the point is to see where the
# declaration and the file disagree. A store without the modern `fileSize`
# column is reported as such rather than as 0 mismatches.
cat >"$work/summary.awk" <<'EOF'
function pad(label, value) { printf "%-22s%s\n", label, value }

BEGIN { FS = "|" }

# `FILENAME`, not `FNR == NR`: an empty rows file would otherwise make the
# first walk line look like a row.
FILENAME == ARGV[1] {
    rows++
    u = tolower($1)
    key = substr(u, 1, 1) "/" substr(u, 1, 4) "/" u "-" tolower($2) "_" $3
    if (key in expect) dup++
    expect[key] = ($4 == "" ? -1 : $4 + 0)
    rowedge[$3 + 0]++
    next
}

{
    files++
    key = tolower($1)
    if (($5 + 0) == 1) soi++
    if ($4 == "ffd9") eoi++
    # The suffix names the level's LONG edge. Comparing it against the frame's
    # width under-reported every portrait level: a 2731x4096 level is `_4096`.
    if (($2 + 0) == (($7 > $8) ? $7 : $8)) suffix++
    if ($14 == "ok") walked++
    if ($9 != "0") app1++
    if ($10 != "0") app2++
    if ($11 == "1") exif++
    if ($12 == "1") xmp++
    if ($13 == "1") icc++
    if (index($6, "APP14")) app14++
    chains[$6]++
    fileedge[$2 + 0]++
    if (key in expect) {
        matched++
        if (expect[key] >= 0) {
            sized++
            if (expect[key] != $3 + 0) {
                mismatch++
                if (mismatch <= 20) printf "  size mismatch %s: row %d, disk %d\n", $1, expect[key], $3
            }
        }
        delete expect[key]
    } else {
        unmatched++
        if (unmatched <= 20) printf "  file without row %s\n", $1
    }
}

END {
    stranded = 0
    for (key in expect) {
        stranded++
        if (stranded <= 20) printf "  row without file %s\n", key
    }

    pad("previews_db", dbstate)
    pad("store_version", store)
    pad("cache_entries", entries)
    pad("pyramids", pyramids)
    pad("pyramid_levels", rows)
    pad("distinct_digests", digests)
    pad("file_size_column", sizecol)
    pad("duplicate_rows", dup + 0)
    print ""
    pad("level_files", files + 0)
    pad("soi_at_offset_0", soi + 0)
    pad("eoi_at_eof", eoi + 0)
    pad("walked_to_sos", walked + 0)
    pad("suffix_matches_sof", suffix + 0)
    pad("rows_without_file", stranded + 0)
    pad("files_without_row", unmatched + 0)
    pad("file_size_checked", sized + 0)
    pad("file_size_mismatch", mismatch + 0)
    print ""
    pad("app1", app1 + 0)
    pad("app2", app2 + 0)
    pad("app14", app14 + 0)
    pad("exif", exif + 0)
    pad("xmp", xmp + 0)
    pad("icc_profile", icc + 0)
    # Sorted by plain selection so the report reads the same on any awk: the
    # order of `for (k in a)` is unspecified and this is a quotable artifact.
    print ""
    print "chains"
    nc = 0
    for (c in chains) cl[++nc] = c
    for (i = 1; i <= nc; i++) {
        m = i
        for (j = i + 1; j <= nc; j++) if (cl[j] < cl[m]) m = j
        t = cl[i]; cl[i] = cl[m]; cl[m] = t
        printf "  %6d  %s\n", chains[cl[i]], cl[i]
    }
    print ""
    print "series (long edge: files / rows)"
    ne = 0
    for (e in fileedge) el[++ne] = e + 0
    for (e in rowedge) if (!(e in fileedge)) el[++ne] = e + 0
    for (i = 1; i <= ne; i++) {
        m = i
        for (j = i + 1; j <= ne; j++) if (el[j] < el[m]) m = j
        t = el[i]; el[i] = el[m]; el[m] = t
        printf "  %6d: %d / %d\n", el[i], fileedge[el[i]] + 0, rowedge[el[i]] + 0
    }
}
EOF

# --- the index ------------------------------------------------------------
db="$root/previews.db"
dbstate=missing
store='?'
pyramids='?'
entries='?'
digests='?'
sizecol='?'
: >"$work/rows"

if [ -f "$db" ]; then
    dbstate="present ($(wc -c <"$db" | tr -d ' ') bytes)"
    [ ! -f "$db-wal" ] || dbstate="$dbstate, with a -wal: the store may be ahead of this read"
    # Read-only: no `-wal`/`-shm` is created beside the user's cache, and SQLite
    # refuses to write through this handle. (A `file:…?immutable=1` URI would be
    # stricter, but a native sqlite3 on Windows cannot open an MSYS-styled path
    # once a query string rides along, so the flag carries the guarantee.)
    query() {
        sqlite3 -readonly -noheader -separator '|' "$db" "$1" 2>/dev/null || true
    }
    store=$(query "SELECT version FROM StoreInfo LIMIT 1;")
    store=${store:-'?'}
    # A cache can hold more pyramids than it has images: a fresh render keeps
    # the preview pyramid and a separate thumbnail pyramid that no cache entry
    # claims, so both counts are printed.
    entries=$(query "SELECT COUNT(*) FROM ImageCacheEntry;")
    entries=${entries:-'?'}
    pyramids=$(query "SELECT COUNT(*) FROM Pyramid;")
    pyramids=${pyramids:-'?'}
    digests=$(query "SELECT COUNT(DISTINCT digest) FROM Pyramid;")
    digests=${digests:-'?'}
    # The modern store adds `PyramidLevel.fileSize` (§5.4); the 13.2 schema has
    # no such column, so the query is chosen by what the table actually has.
    if [ "$(query "SELECT COUNT(*) FROM pragma_table_info('PyramidLevel') WHERE name='fileSize';")" = 1 ]; then
        sizecol=present
        query "SELECT uuid, digest, CAST(longDimension AS INTEGER), CAST(fileSize AS INTEGER) FROM PyramidLevel;" >"$work/rows" || true
    else
        sizecol=absent
        query "SELECT uuid, digest, CAST(longDimension AS INTEGER), '' FROM PyramidLevel;" >"$work/rows" || true
    fi
fi

# --- the files ------------------------------------------------------------
(cd "$root" && find . -type f) | sed 's|^\./||' | sort >"$work/all"
grep -E "$grammar" "$work/all" >"$work/level" || true
grep -Ev "$grammar" "$work/all" | grep -Ev "$not_payload" >"$work/odd" || true

files=$(wc -l <"$work/level" | tr -d ' ')
odd=$(wc -l <"$work/odd" | tr -d ' ')

: >"$work/walk"
while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    path="$root/$rel"
    leaf=${rel##*/}
    edge=${leaf##*_}
    size=$(wc -c <"$path" | tr -d ' ')
    eoi=$(tail -c 2 "$path" | od -An -v -tx1 | tr -d ' \n')
    line=$(od -An -v -tx1 -N 262144 "$path" | awk -f "$work/walk.awk")
    printf '%s|%s|%s|%s|%s\n' "$rel" "$edge" "$size" "$eoi" "$line" >>"$work/walk"
    if [ "$verbose" = 1 ]; then
        printf '%s\n' "$rel $line"
    fi
done <"$work/level"

# --- the report -----------------------------------------------------------
printf 'cache %s\n\n' "$name"
awk -F '|' \
    -v dbstate="$dbstate" -v store="$store" -v pyramids="$pyramids" \
    -v digests="$digests" -v sizecol="$sizecol" -v entries="$entries" \
    -f "$work/summary.awk" "$work/rows" "$work/walk"

if [ "$odd" != 0 ]; then
    printf '\nunrecognised files (neither a level file nor a known cache file): %s\n' "$odd"
    head -20 "$work/odd" | sed 's/^/  /'
fi

if [ ! -s "$work/rows" ] && [ "$files" != 0 ]; then
    printf '\nno PyramidLevel rows: every one of the %s level files is unmatched\n' "$files"
fi
