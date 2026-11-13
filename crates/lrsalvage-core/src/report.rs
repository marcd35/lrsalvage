//! Human and machine readable output: summary, CSV, JSON manifest, contact sheet.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use crate::error::{IoContext, Result};
use crate::model::Orientation;
use crate::plan::{Plan, PlanItem, Tier};
use crate::{jpeg, lrprev};

/// One-line-per-status summary for the terminal, using the default 640 px floor.
pub fn summary(plan: &Plan) -> String {
    summary_with_floor(plan, 640)
}

/// The same summary, naming the floor the caller actually applied.
pub fn summary_with_floor(plan: &Plan, floor: u32) -> String {
    let stats = &plan.stats;

    // label, count, optional trailing note
    let mut rows: Vec<(String, u64, Option<String>)> = Vec::with_capacity(12);
    rows.push(("catalog images".to_string(), stats.images, None));
    rows.push((
        "recovered".to_string(),
        stats.recoverable,
        Some(mebibytes(stats.bytes)),
    ));
    for tier in [
        Tier::Full,
        Tier::Large,
        Tier::Standard,
        Tier::Small,
        Tier::Thumbnail,
    ] {
        let count = stats.tiers.get(tier.as_str()).copied().unwrap_or(0);
        if count > 0 {
            let label = match tier {
                Tier::Full => "  full",
                Tier::Large => "  large",
                Tier::Standard => "  standard",
                Tier::Small => "  small",
                Tier::Thumbnail => "  thumbnail",
            };
            rows.push((label.to_string(), count, None));
        }
    }
    if stats.below_floor > 0 {
        rows.push((
            format!("below floor ({floor}px)"),
            stats.below_floor,
            Some("not written unless --min-long-edge 0".to_string()),
        ));
    }
    for (label, count) in [
        ("missing payload", stats.missing_payload),
        ("no cache entry", stats.no_cache_entry),
        ("original present", stats.original_present),
        ("unmapped payloads", stats.unmapped),
        ("suspect", stats.suspect),
        ("failed", stats.failed),
    ] {
        if count > 0 {
            rows.push((label.to_string(), count, None));
        }
    }

    let numbers: Vec<String> = rows.iter().map(|(_, n, _)| with_thousands(*n)).collect();
    let width = numbers.iter().map(String::len).max().unwrap_or(1);

    let mut out = String::with_capacity(rows.len() * 48);
    for ((label, _, detail), number) in rows.iter().zip(&numbers) {
        out.push_str(&format!("{label:<22}{number:>width$}"));
        if let Some(detail) = detail {
            out.push_str("   ");
            out.push_str(detail);
        }
        out.push('\n');
    }
    out
}

/// `3,416` — thousand separators make four-digit counts readable at a glance.
fn with_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Tenths of a mebibyte, rounded half up, printed with one decimal and the
/// unit it actually is: `1 MiB = 1,048,576 bytes`. `stats.bytes` stays exact in
/// the JSON manifest.
fn mebibytes(bytes: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + (1 << 19)) / (1 << 20);
    format!(
        "{}.{} MiB",
        with_thousands((tenths / 10) as u64),
        tenths % 10
    )
}

/// One stable line per plan item, for the per-image verdict in `scan`:
///
/// ```text
/// status  tier  WxH  profile  relative-output
/// ```
///
/// Fields are separated by two spaces, and a fact the plan does not know is
/// printed as `-`, so a reader (or a script) can rely on the five columns.
pub fn item_lines(plan: &Plan) -> Vec<String> {
    plan.items.iter().map(item_line).collect()
}

fn item_line(item: &PlanItem) -> String {
    let tier = item.tier.map(Tier::as_str).unwrap_or("-");
    let size = match (item.width, item.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "-".to_string(),
    };
    let profile = item.color_profile.as_deref().unwrap_or("-");
    format!(
        "{}  {}  {}  {}  {}",
        item.status.as_str(),
        tier,
        size,
        profile,
        relative_output(item)
    )
}

/// The output path as it reads under the output root.
///
/// The plan stores absolute outputs but not the root they hang from, so the
/// mapped case is rebuilt from the image's own relative path plus the file name
/// the planner actually chose (which carries `- WxH` and ` (2)` suffixes).
fn relative_output(item: &PlanItem) -> String {
    match (&item.image, &item.output) {
        (Some(image), Some(output)) => {
            let base = image.relative_output_path();
            let directory = base.parent().unwrap_or(Path::new(""));
            let Some(name) = output.file_name() else {
                return slashed(&base);
            };
            if directory.as_os_str().is_empty() {
                slashed(Path::new(name))
            } else {
                slashed(&directory.join(name))
            }
        }
        (Some(image), None) => slashed(&image.relative_output_path()),
        (None, Some(output)) => match output.file_name() {
            Some(name) => slashed(Path::new(name)),
            None => "-".to_string(),
        },
        (None, None) => "-".to_string(),
    }
}

/// Fixed column order; downstream tools rely on it.
const CSV_HEADER: &str = "status,tier,width,height,long_edge,color_profile,quality,orientation,selected,output,original_path,capture_time,rating,note";

/// `report.csv`, one row per plan item.
pub fn write_csv(plan: &Plan, path: &Path) -> Result<()> {
    let file = File::create(path).at(path)?;
    let mut writer = BufWriter::new(file);
    let mut line = String::with_capacity(256);

    line.push_str(CSV_HEADER);
    line.push_str("\r\n");
    writer.write_all(line.as_bytes()).at(path)?;

    for item in &plan.items {
        line.clear();

        push_csv_field(&mut line, item.status.as_str());
        line.push(',');
        push_csv_field(&mut line, item.tier.map(Tier::as_str).unwrap_or(""));
        line.push(',');
        push_csv_field(&mut line, &optional_number(item.width));
        line.push(',');
        push_csv_field(&mut line, &optional_number(item.height));
        line.push(',');
        let long_edge = if item.width.is_none() && item.height.is_none() {
            String::new()
        } else {
            item.long_edge().to_string()
        };
        push_csv_field(&mut line, &long_edge);
        line.push(',');
        push_csv_field(&mut line, item.color_profile.as_deref().unwrap_or(""));
        line.push(',');
        push_csv_field(&mut line, item.quality.as_deref().unwrap_or(""));
        line.push(',');
        push_csv_field(&mut line, orientation_code(item.orientation));
        line.push(',');
        push_csv_field(&mut line, if item.selected { "true" } else { "false" });
        line.push(',');
        push_csv_field(
            &mut line,
            &item.output.as_deref().map(slashed).unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(
            &mut line,
            &item
                .image
                .as_ref()
                .map(|image| slashed(&image.original_path))
                .unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(&mut line, capture_time(item).as_deref().unwrap_or(""));
        line.push(',');
        push_csv_field(
            &mut line,
            &rating(item).map(|r| r.to_string()).unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(&mut line, item.note.as_deref().unwrap_or(""));
        line.push_str("\r\n");

        writer.write_all(line.as_bytes()).at(path)?;
    }

    writer.flush().at(path)?;
    Ok(())
}

fn optional_number(value: Option<u32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

/// The capture time recorded in the catalog, if any.
fn capture_time(item: &PlanItem) -> Option<String> {
    item.image
        .as_ref()
        .and_then(|i| i.xmp.as_ref())
        .and_then(|x| x.capture_time.clone())
}

/// The star rating recorded in the catalog, if any.
fn rating(item: &PlanItem) -> Option<u8> {
    item.image
        .as_ref()
        .and_then(|i| i.xmp.as_ref())
        .and_then(|x| x.rating)
}

/// RFC 4180 quoting for one field, exposed for the extraction report.
pub fn escape_csv(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    push_csv_field(&mut out, value);
    out
}

/// RFC 4180: quote when the value could otherwise break the row, and double
/// every internal quote.
fn push_csv_field(out: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        out.push('"');
        for ch in value.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

/// Paths cross into JSON and CSV, where `\` is either an escape or a foreign
/// separator. Always forward slashes.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Lightroom's two-letter orientation code.
fn orientation_code(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Normal => "AB",
        Orientation::Rotate90 => "BC",
        Orientation::Rotate180 => "CD",
        Orientation::Rotate270 => "DA",
        Orientation::Unknown => "UNKNOWN",
    }
}

/// Versioned JSON manifest, the stable contract for downstream tools.
pub fn write_json(plan: &Plan, path: &Path, version: &str) -> Result<()> {
    let mut tiers = Map::new();
    for tier in [
        Tier::Full,
        Tier::Large,
        Tier::Standard,
        Tier::Small,
        Tier::Thumbnail,
    ] {
        if let Some(count) = plan.stats.tiers.get(tier.as_str()) {
            tiers.insert(tier.as_str().to_string(), json!(count));
        }
    }

    let items: Vec<Value> = plan.items.iter().map(item_json).collect();
    let manifest = json!({
        "schema": "lrsalvage.manifest",
        "version": version,
        "generated_unix": unix_now(),
        "stats": {
            "images": plan.stats.images,
            "recovered": plan.stats.recoverable,
            "bytes": plan.stats.bytes,
            "tiers": Value::Object(tiers),
        },
        "items": items,
    });

    let mut text =
        serde_json::to_string_pretty(&manifest).map_err(|error| crate::error::Error::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(format!("manifest serialisation failed: {error}")),
        })?;
    text.push('\n');
    std::fs::write(path, text).at(path)?;
    Ok(())
}

fn item_json(item: &PlanItem) -> Value {
    let mut object = Map::new();
    object.insert("status".to_string(), json!(item.status.as_str()));
    if let Some(tier) = item.tier {
        object.insert("tier".to_string(), json!(tier.as_str()));
    }
    if let Some(width) = item.width {
        object.insert("width".to_string(), json!(width));
    }
    if let Some(height) = item.height {
        object.insert("height".to_string(), json!(height));
    }
    if let Some(profile) = &item.color_profile {
        object.insert("color_profile".to_string(), json!(profile));
    }
    if let Some(quality) = &item.quality {
        object.insert("quality".to_string(), json!(quality));
    }
    object.insert(
        "orientation".to_string(),
        json!(orientation_code(item.orientation)),
    );
    object.insert("selected".to_string(), json!(item.selected));
    if let Some(output) = &item.output {
        object.insert("output".to_string(), json!(slashed(output)));
    }
    if let Some(image) = &item.image {
        object.insert(
            "original_path".to_string(),
            json!(slashed(&image.original_path)),
        );
    }
    if let Some(capture) = capture_time(item) {
        object.insert("capture_time".to_string(), json!(capture));
    }
    if let Some(rating) = rating(item) {
        object.insert("rating".to_string(), json!(rating));
    }
    // Absent stays absent, except `note`: it is part of the schema and is
    // explicitly null when there is nothing to say.
    object.insert(
        "note".to_string(),
        match &item.note {
            Some(note) => json!(note),
            None => Value::Null,
        },
    );
    Value::Object(object)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Self-contained HTML contact sheet with inlined thumbnails.
pub fn write_contact_sheet(plan: &Plan, path: &Path, cache_root: &Path) -> Result<()> {
    // Stable group order: first appearance in the plan.
    let mut groups: Vec<(String, Vec<&PlanItem>)> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();
    for item in plan.selected() {
        let group = item
            .output
            .as_deref()
            .and_then(Path::parent)
            .map(slashed)
            .filter(|group| !group.is_empty())
            .unwrap_or_else(|| "(no output)".to_string());
        match group_index.get(&group) {
            Some(&slot) => groups[slot].1.push(item),
            None => {
                group_index.insert(group.clone(), groups.len());
                groups.push((group, vec![item]));
            }
        }
    }

    let selected = plan.selected().count();
    let mut html = String::with_capacity(2048 + selected * 9_000);

    html.push_str(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>lrsalvage contact sheet</title>\n<style>\n\
         body{font-family:system-ui,sans-serif;margin:1.5rem;background:#fafafa;color:#222}\n\
         h2{font-size:1rem;margin:1.5rem 0 .5rem}\n\
         .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:1rem}\n\
         figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:4px;padding:.5rem}\n\
         img{max-width:100%;height:auto;display:block}\n\
         figcaption{font-size:.75rem;color:#555;margin-top:.4rem;word-break:break-word}\n\
         .placeholder{aspect-ratio:4/3;display:flex;align-items:center;justify-content:center;\n\
         background:#ddd;color:#555;font-size:.8rem}\n\
         </style>\n</head>\n<body>\n<header>\n<h1>lrsalvage contact sheet</h1>\n",
    );

    html.push_str("<p class=\"stats\">");
    html.push_str(&format!(
        "catalog images {} &middot; recovered {}",
        with_thousands(plan.stats.images),
        with_thousands(plan.stats.recoverable)
    ));
    for tier in [
        Tier::Full,
        Tier::Large,
        Tier::Standard,
        Tier::Small,
        Tier::Thumbnail,
    ] {
        if let Some(count) = plan.stats.tiers.get(tier.as_str()) {
            html.push_str(&format!(
                " &middot; {} {}",
                tier.as_str(),
                with_thousands(*count)
            ));
        }
    }
    html.push_str(&format!(
        " &middot; selected {}\n</p>\n</header>\n",
        with_thousands(selected as u64)
    ));

    if groups.is_empty() {
        html.push_str("<p>No selected previews to show.</p>\n");
    }
    for (group, items) in &groups {
        html.push_str("<section>\n<h2>");
        push_escaped(&mut html, group);
        html.push_str("</h2>\n<div class=\"grid\">\n");
        for item in items {
            push_card(&mut html, item, cache_root);
        }
        html.push_str("</div>\n</section>\n");
    }

    html.push_str("</body>\n</html>\n");
    std::fs::write(path, html).at(path)?;
    Ok(())
}

fn push_card(out: &mut String, item: &PlanItem, cache_root: &Path) {
    let stem = caption_stem(item);
    out.push_str("<figure>\n");
    match thumbnail_data_url(item, cache_root) {
        Some(data_url) => {
            out.push_str("<img loading=\"lazy\" src=\"");
            out.push_str(&data_url);
            out.push_str("\" alt=\"");
            push_escaped(out, &stem);
            out.push_str("\">\n");
        }
        // A broken payload must not cost the reader the rest of the sheet.
        None => {
            out.push_str("<div class=\"placeholder\">");
            push_escaped(out, item.status.as_str());
            out.push_str("</div>\n");
        }
    }
    out.push_str("<figcaption>");
    push_escaped(out, &stem);
    if let (Some(width), Some(height)) = (item.width, item.height) {
        out.push_str(&format!(" &middot; {width}&times;{height}"));
    }
    if let Some(tier) = item.tier {
        out.push_str(" &middot; ");
        out.push_str(tier.as_str());
    }
    if let Some(profile) = &item.color_profile {
        out.push_str(" &middot; ");
        push_escaped(out, profile);
    }
    out.push_str("</figcaption>\n</figure>\n");
}

/// File stem of the output, falling back to the catalog's own name.
fn caption_stem(item: &PlanItem) -> String {
    item.output
        .as_deref()
        .and_then(Path::file_stem)
        .map(|stem| stem.to_string_lossy().into_owned())
        .or_else(|| item.image.as_ref().map(|image| image.output_stem()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// A data URL for the *smallest* level: a contact sheet wants recognition, not
/// resolution.
fn thumbnail_data_url(item: &PlanItem, cache_root: &Path) -> Option<String> {
    let payload = item.payload.as_ref()?;
    let bytes = std::fs::read(resolve_payload(payload, cache_root)).ok()?;
    let parsed = lrprev::Lrprev::parse(&bytes).ok()?;
    let smallest = smallest_level(&parsed)?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(smallest)
    ))
}

/// Resolve a payload path against the cache root exactly once.
///
/// The planner builds payload paths from whatever `--previews` it was given, so
/// with a relative `--previews` the item already carries that prefix. Joining
/// it again would look for `cache/cache/…` and silently drop every thumbnail.
fn resolve_payload(payload: &Path, cache_root: &Path) -> PathBuf {
    let already_resolved = payload.is_absolute()
        || (!cache_root.as_os_str().is_empty() && payload.starts_with(cache_root));
    if already_resolved {
        payload.to_path_buf()
    } else {
        cache_root.join(payload)
    }
}

fn smallest_level<'a>(parsed: &lrprev::Lrprev<'a>) -> Option<&'a [u8]> {
    parsed
        .levels()
        .into_iter()
        .filter_map(|chunk| {
            jpeg::dimensions(chunk.data)
                .ok()
                .map(|(width, height)| (u64::from(width) * u64::from(height), chunk.data))
        })
        .min_by_key(|(area, _)| *area)
        .map(|(_, data)| data)
}

/// HTML text is untrusted here: names come from the catalog.
fn push_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::model::CatalogImage;
    use crate::plan::{PlanItem, Stats, Status};

    fn plan_with(stats: Stats) -> Plan {
        Plan {
            items: Vec::new(),
            stats,
        }
    }

    fn plan_with_items(items: Vec<PlanItem>) -> Plan {
        Plan {
            items,
            stats: Stats::default(),
        }
    }

    /// One `AgHg` chunk, as the container writer would emit it.
    fn chunk(name: &str, payload: &[u8], padding: usize) -> Vec<u8> {
        let header_len = 24 + name.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"AgHg");
        bytes.extend_from_slice(&u16::try_from(header_len).unwrap().to_be_bytes());
        bytes.push(0); // version
        bytes.push(0); // kind
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&(padding as u64).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend(std::iter::repeat_n(0u8, padding));
        bytes
    }

    /// A structurally valid JPEG at `width` x `height`.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ]);
        bytes.extend_from_slice(&[0x00, 0xFF, 0xD9]);
        bytes
    }

    fn sample_image(base_name: &str) -> CatalogImage {
        CatalogImage {
            id_local: 7,
            copy_name: None,
            orientation: Orientation::Normal,
            file_format: "CR2".to_string(),
            original_path: PathBuf::from("F:\\photog").join(format!("{base_name}.CR2")),
            root_folder: "F:/photog/2015/3. March/3.16 107 york street/".to_string(),
            path_from_root: String::new(),
            base_name: base_name.to_string(),
            extension: "CR2".to_string(),
            xmp: None,
        }
    }

    fn recovered_item(base_name: &str, payload: PathBuf, output: PathBuf) -> PlanItem {
        PlanItem {
            status: Status::Recoverable,
            image: Some(sample_image(base_name)),
            payload: Some(payload),
            output: Some(output),
            width: Some(1600),
            height: Some(1200),
            color_profile: Some("AdobeRGB".to_string()),
            quality: Some("standard".to_string()),
            orientation: Orientation::Rotate90,
            tier: Some(Tier::Standard),
            declared: None,
            selected: true,
            note: None,
        }
    }

    #[test]
    fn summary_formats_counts_and_mebibytes_and_drops_zero_rows() {
        let mut tiers = HashMap::new();
        tiers.insert("full", 53);
        tiers.insert("standard", 2696);

        let text = summary(&plan_with(Stats {
            images: 3416,
            recoverable: 3057,
            bytes: 1_143_100_000,
            tiers,
            ..Stats::default()
        }));

        assert!(text.contains("catalog images"), "{text}");
        assert!(text.contains("recovered"), "{text}");
        assert!(text.contains("3,057"), "{text}");
        // 1,143,100,000 bytes is 1,090.1 MiB: the figure from the spec, in the
        // unit the arithmetic actually uses.
        assert!(text.contains("1,090.1 MiB"), "{text}");
        assert!(text.contains("2,696"), "{text}");
        assert!(!text.contains(" MB"), "MiB is not MB:\n{text}");

        for absent in [
            "missing payload",
            "no cache entry",
            "original present",
            "unmapped payloads",
            "suspect",
            "failed",
            "large",
            "small",
            "thumbnail",
            "below floor",
        ] {
            assert!(
                !text.contains(absent),
                "zero row `{absent}` should be omitted from:\n{text}"
            );
        }
    }

    #[test]
    fn summary_names_the_floor_it_was_given() {
        let stats = Stats {
            images: 1,
            recoverable: 1,
            below_floor: 2,
            ..Stats::default()
        };

        let text = summary_with_floor(&plan_with(stats), 1024);
        assert!(text.contains("below floor (1024px)"), "{text}");
        assert!(!text.contains("640px"), "{text}");

        let default = summary(&plan_with(Stats {
            images: 1,
            recoverable: 1,
            below_floor: 2,
            ..Stats::default()
        }));
        assert!(default.contains("below floor (640px)"), "{default}");
    }

    #[test]
    fn empty_plan_still_reports_a_recovered_row() {
        let text = summary(&plan_with(Stats::default()));
        assert!(text.contains("recovered"), "{text}");
        assert!(text.contains("0.0 MiB"), "{text}");
    }

    #[test]
    fn csv_quotes_commas_quotes_and_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");
        let mut item = recovered_item(
            "107 York St-1",
            PathBuf::from("cache/payload.lrprev"),
            PathBuf::from("out").join("107 York St-1.jpg"),
        );
        item.note = Some("a,b\"c\nd".to_string());
        item.color_profile = Some("Adobe,RGB".to_string());

        write_csv(&plan_with_items(vec![item]), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();

        let header = "status,tier,width,height,long_edge,color_profile,quality,orientation,selected,output,original_path,capture_time,rating,note";
        assert!(text.starts_with(&format!("{header}\r\n")), "{text}");
        assert!(
            text.contains("\"a,b\"\"c\nd\""),
            "comma, quote and newline must survive quoting:\n{text}"
        );
        assert!(text.contains("\"Adobe,RGB\""), "{text}");
        assert!(text.ends_with("\r\n"), "trailing newline missing:\n{text}");
    }

    #[test]
    fn csv_writes_one_row_per_item_and_leaves_absent_fields_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");

        let recovered = recovered_item(
            "107 York St-1",
            PathBuf::from("cache/payload.lrprev"),
            PathBuf::from("out")
                .join("3.16 107 york street")
                .join("107 York St-1.jpg"),
        );
        let unmapped = PlanItem {
            status: Status::Unmapped,
            image: None,
            payload: Some(PathBuf::from("cache/orphan.lrprev")),
            output: Some(PathBuf::from("out").join("_unmapped").join("orphan.jpg")),
            width: None,
            height: None,
            color_profile: None,
            quality: None,
            orientation: Orientation::Unknown,
            tier: None,
            declared: None,
            selected: false,
            note: None,
        };

        write_csv(&plan_with_items(vec![recovered, unmapped]), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(
            rows.len(),
            4,
            "header, two rows, then the trailing newline:\n{text}"
        );
        assert_eq!(
            rows[0],
            "status,tier,width,height,long_edge,color_profile,quality,orientation,selected,output,original_path,capture_time,rating,note"
        );

        let first: Vec<&str> = rows[1].split(',').collect();
        assert_eq!(first.len(), 14, "{:?}", rows[1]);
        assert_eq!(first[0], "recovered");
        assert_eq!(first[1], "standard");
        assert_eq!(first[2], "1600");
        assert_eq!(first[3], "1200");
        assert_eq!(first[4], "1600");
        assert_eq!(first[5], "AdobeRGB");
        assert_eq!(first[6], "standard");
        assert_eq!(first[7], "BC");
        assert_eq!(first[8], "true");
        assert_eq!(first[9], "out/3.16 107 york street/107 York St-1.jpg");
        assert_eq!(first[10], "F:/photog/107 York St-1.CR2");
        assert_eq!(first[11], "");
        assert_eq!(first[12], "");
        assert_eq!(first[13], "");

        let second: Vec<&str> = rows[2].split(',').collect();
        assert_eq!(second.len(), 14, "{:?}", rows[2]);
        assert_eq!(second[0], "unmapped");
        assert_eq!(second[1], "");
        assert_eq!(second[2], "");
        assert_eq!(second[3], "");
        assert_eq!(second[4], "");
        assert_eq!(second[5], "");
        assert_eq!(second[6], "");
        assert_eq!(second[7], "UNKNOWN");
        assert_eq!(second[8], "false");
        assert_eq!(second[9], "out/_unmapped/orphan.jpg");
        assert_eq!(second[10], "");
        assert_eq!(second[13], "");
    }

    #[test]
    fn json_manifest_round_trips_and_omits_fields_that_do_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let recovered = recovered_item(
            "107 York St-1",
            PathBuf::from("cache/payload.lrprev"),
            PathBuf::from("out")
                .join("3.16 107 york street")
                .join("107 York St-1.jpg"),
        );
        let orphan = PlanItem {
            status: Status::Unmapped,
            image: None,
            payload: Some(PathBuf::from("cache/orphan.lrprev")),
            output: None,
            width: None,
            height: None,
            color_profile: None,
            quality: None,
            orientation: Orientation::Unknown,
            tier: None,
            declared: None,
            selected: false,
            note: None,
        };

        let mut tiers = HashMap::new();
        tiers.insert("standard", 1);
        write_json(
            &Plan {
                items: vec![recovered, orphan],
                stats: Stats {
                    images: 2,
                    recoverable: 1,
                    bytes: 1234,
                    tiers,
                    ..Stats::default()
                },
            },
            &path,
            "1",
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let manifest: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest["schema"], "lrsalvage.manifest");
        assert_eq!(manifest["version"], "1");
        assert!(
            manifest["generated_unix"].as_u64().unwrap() > 1_600_000_000,
            "generated_unix must be a real epoch timestamp:\n{text}"
        );
        assert_eq!(manifest["stats"]["images"], 2);
        assert_eq!(manifest["stats"]["recovered"], 1);
        assert_eq!(manifest["stats"]["bytes"], 1234);
        assert_eq!(manifest["stats"]["tiers"]["standard"], 1);

        let items = manifest["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["status"], "recovered");
        assert_eq!(items[0]["tier"], "standard");
        assert_eq!(
            items[0]["output"],
            "out/3.16 107 york street/107 York St-1.jpg"
        );
        assert_eq!(items[0]["note"], Value::Null);
        assert!(items[1].get("output").is_none());
        assert!(items[1].get("width").is_none());
        assert!(items[1].get("tier").is_none());
        assert_eq!(items[1]["status"], "unmapped");
    }

    #[test]
    fn contact_sheet_inlines_the_smallest_level_and_escapes_names() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let small = jpeg(160, 120);
        let large = jpeg(1600, 1200);
        let mut payload = chunk("header", b"pyramid = { }", 0);
        payload.extend(chunk("level_1", &small, 0));
        payload.extend(chunk("level_2", &large, 0));
        let payload_path = cache.join("payload.lrprev");
        std::fs::write(&payload_path, &payload).unwrap();

        let hostile = "evil\"><script>alert('x')&";
        let output = PathBuf::from("out")
            .join("3.16 107 york street")
            .join(format!("{hostile}.jpg"));
        let plan = Plan {
            items: vec![recovered_item(hostile, payload_path, output)],
            stats: Stats {
                images: 1,
                recoverable: 1,
                bytes: u64::try_from(small.len()).unwrap(),
                ..Stats::default()
            },
        };

        let sheet = dir.path().join("sheet.html");
        write_contact_sheet(&plan, &sheet, &cache).unwrap();
        let html = std::fs::read_to_string(&sheet).unwrap();

        assert_eq!(html.matches("<img").count(), 1, "{html}");
        assert!(html.contains("data:image/jpeg;base64,"), "{html}");
        assert!(
            html.contains(&STANDARD.encode(&small)),
            "the smallest level is the one inlined"
        );
        assert!(
            !html.contains(&STANDARD.encode(&large)),
            "the largest level must not be inlined"
        );
        assert!(!html.contains("<script"), "unescaped caption:\n{html}");
        assert!(
            html.contains("evil&quot;&gt;&lt;script&gt;alert(&#39;x&#39;)&amp;"),
            "caption not escaped for HTML:\n{html}"
        );
        assert!(html.contains("<h2>out/3.16 107 york street</h2>"), "{html}");
        assert!(html.contains("1600&times;1200"), "{html}");
        assert!(html.contains("standard"), "{html}");
        assert!(html.contains("AdobeRGB"), "{html}");
        assert!(html.contains("lrsalvage"), "{html}");
    }

    #[test]
    fn contact_sheet_uses_a_placeholder_for_a_malformed_payload() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let payload_path = cache.join("broken.lrprev");
        std::fs::write(&payload_path, b"not an lrprev file").unwrap();

        let mut broken = recovered_item(
            "broken",
            payload_path.clone(),
            PathBuf::from("out").join("broken.jpg"),
        );
        broken.width = None;
        broken.height = None;

        // Unselected items belong in the plan, not on the sheet.
        let mut skipped = recovered_item(
            "skipped",
            payload_path,
            PathBuf::from("out").join("skipped.jpg"),
        );
        skipped.selected = false;

        let plan = Plan {
            items: vec![broken, skipped],
            stats: Stats {
                images: 2,
                recoverable: 2,
                ..Stats::default()
            },
        };

        let sheet = dir.path().join("sheet.html");
        write_contact_sheet(&plan, &sheet, &cache).unwrap();
        let html = std::fs::read_to_string(&sheet).expect("the sheet is still written");
        assert!(html.contains("class=\"placeholder\""), "{html}");
        assert!(!html.contains("data:image"), "{html}");
        assert_eq!(html.matches("<img").count(), 0, "{html}");
        assert_eq!(html.matches("<figure>").count(), 1, "{html}");
        assert!(!html.contains("skipped"), "{html}");
        assert!(html.contains("recovered"), "{html}");
    }

    #[test]
    fn item_lines_give_every_item_one_stable_verdict_line() {
        let recovered = recovered_item(
            "107 York St-1",
            PathBuf::from("cache/payload.lrprev"),
            PathBuf::from("out")
                .join("3.16 107 york street")
                .join("107 York St-1.jpg"),
        );
        let unmapped = PlanItem {
            status: Status::Unmapped,
            image: None,
            payload: Some(PathBuf::from("cache/orphan.lrprev")),
            output: None,
            width: None,
            height: None,
            color_profile: None,
            quality: None,
            orientation: Orientation::Unknown,
            tier: None,
            declared: None,
            selected: false,
            note: None,
        };
        let plan = plan_with_items(vec![recovered, unmapped]);

        let lines = item_lines(&plan);
        assert_eq!(
            lines,
            item_lines(&plan),
            "the same plan gives the same lines"
        );
        assert_eq!(lines.len(), 2, "one line per item, selected or not");
        assert_eq!(
            lines[0],
            "recovered  standard  1600x1200  AdobeRGB  3.16 107 york street/107 York St-1.jpg"
        );
        // Nothing is written for an unmapped payload, so the last column is
        // the same placeholder as every other unknown fact.
        assert_eq!(lines[1], "unmapped  -  -  -  -");
    }

    #[test]
    fn item_lines_use_the_file_name_the_planner_chose() {
        // `--all-levels` and a path collision both change the file name, not
        // the folder: the line must show what is actually written.
        let item = recovered_item(
            "IMG_0007",
            PathBuf::from("cache/payload.lrprev"),
            PathBuf::from("out")
                .join("3.16 107 york street")
                .join("IMG_0007 (2).jpg"),
        );
        let lines = item_lines(&plan_with_items(vec![item]));
        assert!(
            lines[0].ends_with("3.16 107 york street/IMG_0007 (2).jpg"),
            "{}",
            lines[0]
        );
    }

    /// `target` written relative to `base`, so a test can reproduce a relative
    /// `--previews` argument without changing the process working directory.
    fn relative_to(base: &Path, target: &Path) -> PathBuf {
        let base: Vec<_> = base.components().collect();
        let target: Vec<_> = target.components().collect();
        let common = base
            .iter()
            .zip(&target)
            .take_while(|(left, right)| left == right)
            .count();
        let mut out = PathBuf::new();
        for _ in common..base.len() {
            out.push("..");
        }
        for component in &target[common..] {
            out.push(component.as_os_str());
        }
        out
    }

    /// The joining rule itself, independent of the filesystem: a payload path
    /// that already carries the cache prefix must not be joined twice.
    #[test]
    fn resolve_payload_is_idempotent() {
        let cache = Path::new("cache");
        assert_eq!(
            resolve_payload(Path::new("cache/payload.lrprev"), cache),
            PathBuf::from("cache/payload.lrprev")
        );
        assert_eq!(
            resolve_payload(Path::new("payload.lrprev"), cache),
            PathBuf::from("cache/payload.lrprev")
        );
        let absolute = std::env::temp_dir().join("payload.lrprev");
        assert_eq!(resolve_payload(&absolute, cache), absolute);
    }

    #[test]
    fn contact_sheet_resolves_a_relative_cache_root_once() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let small = jpeg(160, 120);
        let mut payload = chunk("header", b"pyramid = { }", 0);
        payload.extend(chunk("level_1", &small, 0));
        std::fs::write(cache.join("payload.lrprev"), &payload).unwrap();

        let cwd = std::env::current_dir().unwrap();
        let relative_cache = relative_to(&cwd, &cache);
        assert!(!relative_cache.is_absolute(), "the premise of this test");
        if relative_cache.has_root() {
            // Windows CI puts the temp dir on a different volume than the
            // working directory, and a relative path cannot cross volumes.
            // The join logic itself is covered by
            // `resolve_payload_is_idempotent`.
            return;
        }

        // A relative `--previews` makes the planner put the prefix into the
        // payload path itself, which is how the old code joined it twice.
        let item = recovered_item(
            "relative",
            relative_cache.join("payload.lrprev"),
            PathBuf::from("out").join("relative.jpg"),
        );
        let plan = Plan {
            items: vec![item],
            stats: Stats {
                images: 1,
                recoverable: 1,
                ..Stats::default()
            },
        };

        let sheet = dir.path().join("sheet.html");
        write_contact_sheet(&plan, &sheet, &relative_cache).unwrap();
        let html = std::fs::read_to_string(&sheet).unwrap();
        assert!(
            html.contains(&STANDARD.encode(&small)),
            "the thumbnail must be inlined, not dropped:\n{html}"
        );
        assert!(!html.contains("class=\"placeholder\""), "{html}");
    }
}
