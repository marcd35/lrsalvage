//! Scenario composition over the byte shapes.
//!
//! A **vault** is one temporary directory holding the three things a run reads
//! and one it writes: `<dir>/catalog/<name>.lrcat`, `<dir>/<name> Previews.lrdata`,
//! the originals a row needs to still exist, and `<dir>/out`. Nothing is
//! committed; the tree is dropped with the vault's `TempDir`.
//!
//! One thing breaks that one-directory rule on purpose: `same_destination_as`
//! points a second vault's `out` at a first vault's, so two catalogs recover into
//! one destination (row M15). Then the output tree belongs to the *other* vault's
//! `TempDir`, which is why that vault has to outlive this one's `write()`.
//!
//! An archetype constructor (`Vault::modern()`, `Vault::legacy()`, …) fixes what
//! the **cache** holds — the layout, whether the index exists, the catalog's
//! version strings — because those are decided once per cache. Composition adds
//! the **rows** inside it: images, pyramids, level files, tombstones, orphans.
//! `write()` materialises the tree; `catalog()`, `previews()`, `out()` and
//! `plan()` read it back.
//!
//! Every method that writes bytes can be handed raw bytes instead of the shape
//! it would build (`Level::raw`, `UnrecognisedFile::raw`,
//! `preoccupied_destination(relative, bytes)`), so a byte-shaped row never needs
//! a writer of its own.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::plan::{Options, Plan};
use lrsalvage_core::previews_db;
use rusqlite::{Connection, params};
use tempfile::TempDir;

use crate::bytes;

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

/// The catalog tables `catalog::Catalog` reads, plus the version row every
/// Lightroom catalog carries (ADR 0004).
const CATALOG_SCHEMA: &str = r#"
    CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
      orientation TEXT, fileFormat TEXT, rootFile INTEGER);
    CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, folder INTEGER,
      baseName TEXT, extension TEXT);
    CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, pathFromRoot TEXT,
      rootFolder INTEGER);
    CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, absolutePath TEXT);
    CREATE TABLE Adobe_variablesTable (id_local INTEGER PRIMARY KEY, name TEXT,
      value TEXT);
"#;

/// `Adobe_AdditionalMetadata` is created only when a vault asks for it: a
/// catalog that predates the table must still be salvageable.
const METADATA_SCHEMA: &str = r#"
    CREATE TABLE Adobe_AdditionalMetadata (image INTEGER, xmp BLOB);
"#;

/// Every table of `previews.db`, verbatim from docs/format-notes.md §2.1 —
/// columns declared without types, which is legal SQLite and what Adobe writes.
/// `{FILE_SIZE}` is the one addition the modern store made (§5.4).
const PREVIEWS_SCHEMA: &str = r#"
    CREATE TABLE StoreInfo (version NOT NULL);
    CREATE TABLE Pyramid (uuid NOT NULL, digest NOT NULL, colorProfile NOT NULL,
      fileTimeStamp, quality NOT NULL, croppedWidth NOT NULL,
      croppedHeight NOT NULL, pyramidFileTimeStamp, fingerprint, fromProxy);
    CREATE TABLE PyramidLevel (uuid NOT NULL, digest NOT NULL, level NOT NULL,
      longDimension NOT NULL, lastAccess NOT NULL, width NOT NULL,
      height NOT NULL{FILE_SIZE});
    CREATE TABLE DeletedPyramid (uuid NOT NULL, digest NOT NULL,
      deletedTimeStamp NOT NULL);
    CREATE TABLE ImageCacheEntry (imageId NOT NULL, uuid NOT NULL,
      digest NOT NULL, orientation);
    CREATE TABLE MotionPreview (uuid NOT NULL, digest NOT NULL,
      frameCount NOT NULL, longSide NOT NULL, fileTimeStamp NOT NULL,
      motionPreviewFileTimeStamp);
    CREATE TABLE DeletedMotionPreview (uuid NOT NULL, digest NOT NULL,
      deletedTimeStamp NOT NULL);
    CREATE TABLE MotionPreviewCacheEntry (imageId PRIMARY KEY, uuid NOT NULL,
      digest NOT NULL);
    CREATE TABLE FacePreview (uuid NOT NULL, digest NOT NULL,
      fileTimeStamp NOT NULL, facePreviewFileTimeStamp);
    CREATE TABLE DeletedFacePreview (uuid NOT NULL, digest NOT NULL,
      deletedTimeStamp NOT NULL);
    CREATE TABLE FacePreviewCacheEntry (imageId PRIMARY KEY, uuid NOT NULL,
      digest NOT NULL);
"#;

/// `docs/format-notes.md` §5.4: `StoreInfo.version` is `13.0` in the modern
/// store too — the store format did not follow the catalog.
const STORE_VERSION: &str = "13.0";

/// The extension every synthetic original is named with.
const EXTENSION: &str = "CR2";

// ---------------------------------------------------------------------------
// Small value types
// ---------------------------------------------------------------------------

/// The cache-level fact: what payload representation the cache holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Loose payload files only.
    Modern,
    /// `.lrprev` containers only.
    Legacy,
    /// Both representations, per pyramid.
    Mixed,
    /// An index and no payload files.
    None,
    /// Payload-like files of no known shape.
    Unrecognised,
}

/// How one payload is stored on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    LevelFile,
    Container,
}

/// One level: the row's claim about it and the bytes that carry it.
///
/// Dimensions are the frame's, `long_dimension` is what the row stores (REAL in
/// every measured cache, §5.7) and therefore what the leaf's suffix rounds.
#[derive(Debug, Clone)]
pub struct Level {
    uuid: String,
    digest: String,
    level: u32,
    width: u32,
    height: u32,
    long_dimension: Option<f64>,
    raw: Option<Vec<u8>>,
}

impl Level {
    /// A level of the pyramid `uuid`, as `width` x `height`.
    pub fn sized(uuid: &str, width: u32, height: u32) -> Self {
        Level {
            uuid: uuid.to_string(),
            digest: digest_of(uuid),
            level: 1,
            width,
            height,
            long_dimension: None,
            raw: None,
        }
    }

    /// The pyramid this level belongs to, when it is not the default `uuid`.
    #[must_use]
    pub fn digest(mut self, digest: &str) -> Self {
        self.digest = digest.to_string();
        self
    }

    /// The `PyramidLevel.level` number a row for this level would carry
    /// (default `1`). A container's chunks are numbered by position instead, so
    /// a composed element never has to number itself.
    #[must_use]
    pub fn number(mut self, level: u32) -> Self {
        self.level = level;
        self
    }

    /// Override `PyramidLevel.longDimension`, which is a REAL: `101.0` is what a
    /// row reads back, and `101` is the leaf suffix.
    #[must_use]
    pub fn long_dimension(mut self, value: f64) -> Self {
        self.long_dimension = Some(value);
        self
    }

    /// Write these bytes as the level's payload instead of a `jpeg(w, h)`.
    #[must_use]
    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.raw = Some(bytes.to_vec());
        self
    }

    fn declared_long_dimension(&self) -> f64 {
        self.long_dimension
            .unwrap_or_else(|| f64::from(self.width.max(self.height)))
    }
}

/// One `Adobe_images` row and the `ImageCacheEntry` that claims its pyramid.
#[derive(Debug, Clone)]
pub struct ImageRow {
    id: i64,
    base_name: String,
    file: Option<i64>,
    copy_name: Option<String>,
    orientation: String,
    xmp: Option<String>,
    original_on_disk: bool,
    has_file_row: bool,
    cache: Option<(String, String)>,
}

impl ImageRow {
    /// A catalog image whose original is at `<root>/<base_name>.CR2`.
    pub fn new(id: i64, base_name: &str) -> Self {
        ImageRow {
            id,
            base_name: base_name.to_string(),
            file: None,
            copy_name: None,
            orientation: "AB".to_string(),
            xmp: None,
            original_on_disk: false,
            has_file_row: true,
            cache: None,
        }
    }

    /// Share another image's `AgLibraryFile` row — two images over one file,
    /// which is how a virtual copy is stored.
    #[must_use]
    pub fn file(mut self, file_id: i64) -> Self {
        self.file = Some(file_id);
        self
    }

    /// The `AgLibraryFile` row itself is absent (its file row was deleted), so
    /// the reconstructed original path has no base name.
    #[must_use]
    pub fn without_file(mut self) -> Self {
        self.has_file_row = false;
        self
    }

    #[must_use]
    pub fn copy(mut self, copy_name: &str) -> Self {
        self.copy_name = Some(copy_name.to_string());
        self
    }

    /// The catalog's two-letter orientation code (`AB`, `BC`, …).
    #[must_use]
    pub fn orientation(mut self, code: &str) -> Self {
        self.orientation = code.to_string();
        self
    }

    /// Store this XMP packet in `Adobe_AdditionalMetadata`.
    #[must_use]
    pub fn xmp(mut self, xml: &str) -> Self {
        self.xmp = Some(xml.to_string());
        self
    }

    /// Write the original file at the path the catalog reconstructs.
    #[must_use]
    pub fn original_on_disk(mut self) -> Self {
        self.original_on_disk = true;
        self
    }

    /// The image has a cache entry pointing at the pyramid `uuid`.
    #[must_use]
    pub fn cache(mut self, uuid: &str) -> Self {
        self.cache = Some((uuid.to_string(), digest_of(uuid)));
        self
    }

    fn file_id(&self) -> i64 {
        self.file.unwrap_or(self.id)
    }
}

/// One `Pyramid` row and the `PyramidLevel` rows declared for it.
#[derive(Debug, Clone)]
pub struct PyramidRow {
    uuid: String,
    digest: String,
    color_profile: String,
    quality: String,
    cropped: Option<(u32, u32)>,
    levels: Vec<(u32, u32)>,
}

impl PyramidRow {
    /// A pyramid for the image whose cache entry names `uuid`.
    pub fn new(uuid: &str) -> Self {
        PyramidRow {
            uuid: uuid.to_string(),
            digest: digest_of(uuid),
            color_profile: "ACR".to_string(),
            quality: "HIGH".to_string(),
            cropped: None,
            levels: Vec::new(),
        }
    }

    #[must_use]
    pub fn digest(mut self, digest: &str) -> Self {
        self.digest = digest.to_string();
        self
    }

    /// `Pyramid.colorProfile` — the name our ICC reconstruction starts from.
    #[must_use]
    pub fn profile(mut self, name: &str) -> Self {
        self.color_profile = name.to_string();
        self
    }

    /// `Pyramid.quality`.
    #[must_use]
    pub fn quality(mut self, name: &str) -> Self {
        self.quality = name.to_string();
        self
    }

    /// `Pyramid.croppedWidth` / `croppedHeight`. Defaults to the first declared
    /// level, which is what every synthetic cache here claims.
    #[must_use]
    pub fn cropped(mut self, width: u32, height: u32) -> Self {
        self.cropped = Some((width, height));
        self
    }

    /// Declare one `PyramidLevel` row. A level declared here has a row whether
    /// or not its file exists elsewhere in the composition.
    #[must_use]
    pub fn level(mut self, width: u32, height: u32) -> Self {
        self.levels.push((width, height));
        self
    }

    fn declared_cropped(&self) -> (u32, u32) {
        self.cropped
            .or_else(|| self.levels.first().copied())
            .unwrap_or((0, 0))
    }
}

/// A payload-like file that is none of the shapes the reader knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnrecognisedShape {
    /// The right shard, a leaf with no numeric suffix.
    NoSuffix,
    /// The right leaf in the wrong shard folder.
    WrongShard,
    /// A leaf whose uuid does not parse.
    BadUuid,
}

/// One such file, optionally with the exact bytes to write.
#[derive(Debug, Clone)]
pub struct UnrecognisedFile {
    shape: UnrecognisedShape,
    raw: Option<Vec<u8>>,
}

impl UnrecognisedFile {
    pub fn no_suffix() -> Self {
        Self::new(UnrecognisedShape::NoSuffix)
    }

    pub fn wrong_shard() -> Self {
        Self::new(UnrecognisedShape::WrongShard)
    }

    pub fn bad_uuid() -> Self {
        Self::new(UnrecognisedShape::BadUuid)
    }

    fn new(shape: UnrecognisedShape) -> Self {
        UnrecognisedFile { shape, raw: None }
    }

    /// Write these bytes instead of the shape's default JPEG.
    #[must_use]
    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.raw = Some(bytes.to_vec());
        self
    }

    fn payload(&self) -> Vec<u8> {
        self.raw.clone().unwrap_or_else(|| bytes::jpeg(800, 600))
    }
}

/// A payload the composition asked for, before it is a file.
#[derive(Debug, Clone)]
struct Payload {
    uuid: String,
    digest: String,
    representation: Representation,
    width: u32,
    height: u32,
    long_dimension: f64,
    data: Option<Vec<u8>>,
}

impl Payload {
    fn from_level(level: &Level, representation: Representation) -> Self {
        Payload {
            uuid: level.uuid.clone(),
            digest: level.digest.clone(),
            representation,
            width: level.width,
            height: level.height,
            long_dimension: level.declared_long_dimension(),
            // A level's raw override is its own bytes; a container carries them
            // as the chunk payload, a level file as the whole file.
            data: level.raw.clone(),
        }
    }

    fn payload(&self) -> Vec<u8> {
        self.data.clone().unwrap_or_else(|| {
            bytes::jpeg(
                u16::try_from(self.width).expect("a JPEG dimension fits a u16"),
                u16::try_from(self.height).expect("a JPEG dimension fits a u16"),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// The vault
// ---------------------------------------------------------------------------

/// A synthetic cache plus the catalog that reads it, built inside a `TempDir`.
#[derive(Debug)]
pub struct Vault {
    dir: TempDir,
    name: String,
    catalog_path: PathBuf,
    previews_path: PathBuf,
    out_path: PathBuf,
    layout: Layout,
    has_index: bool,
    /// Bytes to write at `previews.db` instead of an index: the torn file U4
    /// degrades on.
    raw_index: Option<Vec<u8>>,
    db_version: String,
    crs_version: String,
    root_folder: Option<String>,
    metadata_table: bool,
    images: Vec<ImageRow>,
    pyramids: Vec<PyramidRow>,
    level_rows: Vec<Level>,
    payloads: Vec<Payload>,
    tombstones: Vec<(String, String, f64)>,
    /// Files written verbatim at a container's address — the deliberately
    /// unreadable `.lrprev` `superseded` leaves behind.
    verbatim: Vec<(String, String, Vec<u8>)>,
    unrecognised: Vec<UnrecognisedFile>,
    preoccupied: Vec<(PathBuf, Vec<u8>)>,
    /// What `write()` actually put on disk, recorded as it wrote: the shape
    /// counts a row asserts are derived from these, not from a number copied out
    /// of the composition.
    level_files: Vec<PathBuf>,
    containers: Vec<PathBuf>,
    written: bool,
}

impl Vault {
    // -- archetypes ---------------------------------------------------------

    /// An index and loose payload files, at the catalog's modern versions.
    pub fn modern() -> Self {
        Self::archetype(Layout::Modern, "1504001", "18.0.0")
    }

    /// An index and `.lrprev` containers.
    pub fn legacy() -> Self {
        Self::archetype(Layout::Legacy, "1200001", "15.4.0")
    }

    /// An index with both representations, resolved per pyramid.
    pub fn mixed() -> Self {
        Self::archetype(Layout::Mixed, "1504001", "18.0.0")
    }

    /// A modern cache whose winners are broken three ways.
    pub fn damaged() -> Self {
        Self::archetype(Layout::Modern, "1504001", "18.0.0")
    }

    /// An index that holds no rows and no payload files.
    pub fn empty() -> Self {
        Self::archetype(Layout::None, "1200001", "15.4.0")
    }

    /// Loose payload files and **no** `previews.db`.
    pub fn unindexed() -> Self {
        let mut vault = Self::archetype(Layout::Modern, "1504001", "18.0.0");
        vault.has_index = false;
        vault
    }

    /// A version no release produced: no number may refuse a run (ADR 0004).
    pub fn unrecognised() -> Self {
        Self::archetype(Layout::Unrecognised, "9900001", "99.0.0")
    }

    fn archetype(layout: Layout, db_version: &str, crs_version: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temporary vault directory");
        let (catalog_path, previews_path, out_path) = paths(dir.path(), "2023");
        Vault {
            dir,
            name: "2023".to_string(),
            catalog_path,
            previews_path,
            out_path,
            layout,
            has_index: true,
            raw_index: None,
            db_version: db_version.to_string(),
            crs_version: crs_version.to_string(),
            root_folder: None,
            metadata_table: true,
            images: Vec::new(),
            pyramids: Vec::new(),
            level_rows: Vec::new(),
            payloads: Vec::new(),
            tombstones: Vec::new(),
            verbatim: Vec::new(),
            unrecognised: Vec::new(),
            preoccupied: Vec::new(),
            level_files: Vec::new(),
            containers: Vec::new(),
            written: false,
        }
    }

    // -- catalog-level knobs ------------------------------------------------

    /// The catalog and cache are named `<name>.lrcat` / `<name> Previews.lrdata`.
    #[must_use]
    pub fn catalog_name(mut self, name: &str) -> Self {
        let (catalog_path, previews_path, out_path) = paths(self.dir.path(), name);
        self.name = name.to_string();
        self.catalog_path = catalog_path;
        self.previews_path = previews_path;
        self.out_path = out_path;
        self
    }

    /// The catalog's root folder as Lightroom stores it: POSIX separators, a
    /// trailing slash. Defaults to `<dir>/originals/`.
    #[must_use]
    pub fn root_folder(mut self, absolute_path: &str) -> Self {
        self.root_folder = Some(absolute_path.to_string());
        self
    }

    /// Omit `Adobe_AdditionalMetadata`, as a catalog that predates it does.
    #[must_use]
    pub fn without_metadata_table(mut self) -> Self {
        self.metadata_table = false;
        self
    }

    /// Recover into another vault's output tree: two catalogs, one destination.
    ///
    /// The two vaults keep their own catalog, cache and originals; only `out` is
    /// shared, which is what a library split across machines — or a second
    /// catalog recovered into the same folder — leaves behind (row M15). The
    /// other vault must outlive this one's `write()`, since it drops the
    /// directory the output tree lives in.
    #[must_use]
    pub fn same_destination_as(mut self, other: &Vault) -> Self {
        self.out_path = other.out_path.clone();
        self
    }

    // -- composition --------------------------------------------------------

    /// One catalog image: its row, its original path, its orientation, its XMP.
    #[must_use]
    pub fn image(mut self, row: ImageRow) -> Self {
        self.images.push(row);
        self
    }

    /// One pyramid: the `Pyramid` row and one `PyramidLevel` row per declared
    /// level, carrying the profile and quality names `previews.db` records.
    #[must_use]
    pub fn pyramid(mut self, row: PyramidRow) -> Self {
        self.pyramids.push(row);
        self
    }

    /// A level file at its derived leaf — or, in a legacy vault, a `level_N`
    /// chunk of that pyramid's container. No row is written.
    #[must_use]
    pub fn level_file(self, level: Level) -> Self {
        let representation = self.representation();
        self.payload(level, representation)
    }

    /// The pyramid's levels as one `.lrprev`, whatever the cache holds: the
    /// legacy representation inside a cache that also has level files, which is
    /// how a per-pyramid resolution is proved (row X2). No row is written.
    #[must_use]
    pub fn container(self, levels: &[Level]) -> Self {
        self.payloads(levels, Representation::Container)
    }

    /// A level file the widened glob must find: the file exists, the row does
    /// not. This is [`Vault::level_file`] named for the row it lacks.
    #[must_use]
    pub fn file_without_row(self, level: Level) -> Self {
        self.level_file(level)
    }

    /// A `PyramidLevel` row whose file is absent — a pruned level.
    #[must_use]
    pub fn row_without_file(mut self, level: Level) -> Self {
        self.level_rows.push(level);
        self
    }

    /// A `DeletedPyramid` row. It proves a preview once existed; it is never a
    /// cause (ADR 0006 ¶5).
    #[must_use]
    pub fn tombstone(mut self, uuid: &str, timestamp: f64) -> Self {
        self.tombstones
            .push((uuid.to_string(), digest_of(uuid), timestamp));
        self
    }

    /// Payload files for a pyramid no image claims: files, no rows, no entry.
    #[must_use]
    pub fn unclaimed(self, levels: &[Level]) -> Self {
        let representation = self.representation();
        self.payloads(levels, representation)
    }

    /// A pyramid with level files **and** a deliberately unreadable `.lrprev`,
    /// which must never be read (ADR 0006 ¶6).
    #[must_use]
    pub fn superseded(mut self, levels: &[Level]) -> Self {
        let Some(first) = levels.first() else {
            return self;
        };
        let address = first.uuid.clone();
        let digest = first.digest.clone();
        self = self.payloads(levels, Representation::LevelFile);
        // Not a container: reading it would fail the row (ADR 0006 ¶6).
        self.verbatim.push((
            address,
            digest,
            b"superseded: not an AgHg container\n".to_vec(),
        ));
        self
    }

    /// A payload-like file of no known shape: written where it is, mapped by
    /// nothing.
    #[must_use]
    pub fn unrecognised_file(mut self, file: UnrecognisedFile) -> Self {
        self.unrecognised.push(file);
        self
    }

    /// A file already sitting at `relative` inside the output tree.
    #[must_use]
    pub fn preoccupied_destination(mut self, relative: &str, bytes: &[u8]) -> Self {
        self.preoccupied
            .push((PathBuf::from(relative), bytes.to_vec()));
        self
    }

    /// Write these bytes at `previews.db` instead of an index: a file SQLite
    /// cannot read, degrading exactly like a missing one (ADR 0006 ¶2).
    #[must_use]
    pub fn raw_index(mut self, bytes: &[u8]) -> Self {
        self.raw_index = Some(bytes.to_vec());
        self
    }

    fn representation(&self) -> Representation {
        match self.layout {
            Layout::Legacy => Representation::Container,
            // `mixed` composes level files by default; a container in a mixed
            // cache comes from `superseded`, and a lone one from a legacy vault.
            _ => Representation::LevelFile,
        }
    }

    fn payload(mut self, level: Level, representation: Representation) -> Self {
        self.payloads
            .push(Payload::from_level(&level, representation));
        self
    }

    fn payloads(mut self, levels: &[Level], representation: Representation) -> Self {
        for level in levels {
            self = self.payload(level.clone(), representation);
        }
        self
    }

    // -- writing ------------------------------------------------------------

    /// Materialise the tree and hand the same value back, ready to read.
    ///
    /// # Panics
    ///
    /// Panics if the vault was already written, or if a temp directory or
    /// database cannot be created — a fixture failure is a test failure.
    #[must_use]
    pub fn write(mut self) -> Self {
        assert!(!self.written, "a vault is written once");

        fs::create_dir_all(self.catalog().parent().expect("catalog has a parent"))
            .expect("create the catalog directory");
        fs::create_dir_all(self.previews()).expect("create the preview cache");
        fs::create_dir_all(self.out()).expect("create the output root");

        self.write_catalog();
        if self.has_index {
            self.write_previews();
        }
        self.write_payloads();
        self.write_unrecognised();
        self.write_preoccupied();
        self.write_originals();

        self.written = true;
        self
    }

    // -- reading ------------------------------------------------------------

    /// The vault's temporary directory. Dropping the vault drops it.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// `<dir>/catalog/<name>.lrcat`.
    pub fn catalog(&self) -> &Path {
        &self.catalog_path
    }

    /// `<dir>/<name> Previews.lrdata`.
    pub fn previews(&self) -> &Path {
        &self.previews_path
    }

    /// `<dir>/out` — or the other vault's, after
    /// [`Vault::same_destination_as`].
    pub fn out(&self) -> &Path {
        &self.out_path
    }

    /// Every level file the composition wrote, in write order.
    pub fn level_files(&self) -> &[PathBuf] {
        &self.level_files
    }

    /// Every `.lrprev` the composition wrote — a container's address, including
    /// the deliberately unreadable one a superseded pyramid leaves behind.
    pub fn containers(&self) -> &[PathBuf] {
        &self.containers
    }

    /// The plan a v1 build makes for this vault: catalog plus index plus cache.
    ///
    /// An indexless vault yields a plan too: an absent index is a mode, so the
    /// index degrades to an empty one rather than refusing the vault
    /// (`adr-0006-degrade-without-guessing` ¶1–¶2).
    ///
    /// # Panics
    ///
    /// Panics if the vault was not written, or if either store cannot be read.
    pub fn plan(&self, options: &Options) -> Plan {
        assert!(self.written, "call write() before plan()");
        let (_snapshot, catalog) = Catalog::open(self.catalog()).expect("open the vault catalog");
        let index = previews_db::open(&self.previews().join("previews.db")).index;
        Plan::build(&catalog, &index, self.previews(), self.out(), options).expect("build a plan")
    }

    // -- writers ------------------------------------------------------------

    fn write_catalog(&self) {
        let conn = Connection::open(self.catalog()).expect("create the catalog");
        conn.execute_batch(CATALOG_SCHEMA).expect("catalog schema");
        if self.metadata_table {
            conn.execute_batch(METADATA_SCHEMA)
                .expect("metadata schema");
            for image in &self.images {
                if let Some(xml) = &image.xmp {
                    conn.execute(
                        "INSERT INTO Adobe_AdditionalMetadata (image, xmp) VALUES (?1, ?2)",
                        params![image.id, bytes::xmp_blob(xml)],
                    )
                    .expect("xmp row");
                }
            }
        }

        conn.execute(
            "INSERT INTO AgLibraryRootFolder (id_local, absolutePath) VALUES (1, ?1)",
            [self.root_folder_value()],
        )
        .expect("root folder");
        conn.execute(
            "INSERT INTO AgLibraryFolder (id_local, pathFromRoot, rootFolder) VALUES (10, '', 1)",
            [],
        )
        .expect("folder");

        // One file row per distinct file id, first writer wins.
        let mut files: BTreeMap<i64, &ImageRow> = BTreeMap::new();
        for image in self.images.iter().filter(|image| image.has_file_row) {
            files.entry(image.file_id()).or_insert(image);
        }
        for (id, image) in &files {
            conn.execute(
                "INSERT INTO AgLibraryFile (id_local, folder, baseName, extension) \
                 VALUES (?1, 10, ?2, ?3)",
                params![id, image.base_name, EXTENSION],
            )
            .expect("file row");
        }

        for image in &self.images {
            let root_file = if image.has_file_row {
                Some(image.file_id())
            } else {
                None
            };
            conn.execute(
                "INSERT INTO Adobe_images \
                 (id_local, copyName, orientation, fileFormat, rootFile) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    image.id,
                    image.copy_name,
                    image.orientation,
                    EXTENSION,
                    root_file
                ],
            )
            .expect("image row");
        }

        for (id, (name, value)) in [
            ("Adobe_DBVersion", &self.db_version),
            ("Adobe_CrsVersion", &self.crs_version),
        ]
        .into_iter()
        .enumerate()
        {
            conn.execute(
                "INSERT INTO Adobe_variablesTable (id_local, name, value) VALUES (?1, ?2, ?3)",
                params![id as i64 + 1, name, value],
            )
            .expect("version row");
        }
    }

    fn write_previews(&self) {
        let db = self.previews().join("previews.db");
        if let Some(bytes) = &self.raw_index {
            write_file(&db, bytes);
            return;
        }
        let conn = Connection::open(db).expect("create previews.db");
        let modern = self.layout != Layout::Legacy;
        conn.execute_batch(
            &PREVIEWS_SCHEMA.replace("{FILE_SIZE}", if modern { ", fileSize" } else { "" }),
        )
        .expect("previews schema");
        conn.execute(
            "INSERT INTO StoreInfo (version) VALUES (?1)",
            [STORE_VERSION],
        )
        .expect("store info");

        let level_columns = if modern {
            "uuid, digest, level, longDimension, lastAccess, width, height, fileSize"
        } else {
            "uuid, digest, level, longDimension, lastAccess, width, height"
        };
        let mut level_values = String::from("?1, ?2, ?3, ?4, ?5, ?6, ?7");
        if modern {
            // Nothing reads `fileSize`, and a synthetic number here would be a
            // fact no row pins: a level file is written to its own bytes, and
            // §5.4 measures `fileSize` against them rather than the reverse.
            level_values.push_str(", NULL");
        }

        for pyramid in &self.pyramids {
            let (cropped_width, cropped_height) = pyramid.declared_cropped();
            conn.execute(
                "INSERT INTO Pyramid (uuid, digest, colorProfile, quality, \
                 croppedWidth, croppedHeight, fileTimeStamp, pyramidFileTimeStamp, \
                 fingerprint, fromProxy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL)",
                params![
                    pyramid.uuid,
                    pyramid.digest,
                    pyramid.color_profile,
                    pyramid.quality,
                    f64::from(cropped_width),
                    f64::from(cropped_height),
                ],
            )
            .expect("pyramid");
            for (level, (width, height)) in pyramid.levels.iter().enumerate() {
                conn.execute(
                    &format!("INSERT INTO PyramidLevel ({level_columns}) VALUES ({level_values})"),
                    params![
                        pyramid.uuid,
                        pyramid.digest,
                        level as f64,
                        f64::from(*width.max(height)),
                        0.0,
                        f64::from(*width),
                        f64::from(*height),
                    ],
                )
                .expect("pyramid level");
            }
        }

        // A row whose file is absent: the row is the only claim (§5.5).
        for level in &self.level_rows {
            conn.execute(
                &format!("INSERT INTO PyramidLevel ({level_columns}) VALUES ({level_values})"),
                params![
                    level.uuid,
                    level.digest,
                    f64::from(level.level),
                    level.declared_long_dimension(),
                    0.0,
                    f64::from(level.width),
                    f64::from(level.height),
                ],
            )
            .expect("pruned pyramid level");
        }

        for image in &self.images {
            let Some((uuid, digest)) = &image.cache else {
                continue;
            };
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![image.id as f64, uuid, digest, image.orientation],
            )
            .expect("cache entry");
        }

        for (uuid, digest, timestamp) in &self.tombstones {
            conn.execute(
                "INSERT INTO DeletedPyramid (uuid, digest, deletedTimeStamp) \
                 VALUES (?1, ?2, ?3)",
                params![uuid, digest, timestamp],
            )
            .expect("tombstone");
        }
    }

    fn write_payloads(&mut self) {
        // Containers are one file per pyramid: group the levels back together.
        let mut containers: Vec<((String, String), Vec<&Payload>)> = Vec::new();

        for payload in &self.payloads {
            match payload.representation {
                Representation::LevelFile => {
                    let path = self
                        .previews()
                        .join(self.shard(&payload.uuid))
                        .join(bytes::leaf(
                            &payload.uuid,
                            &payload.digest,
                            payload.long_dimension.round().max(0.0) as u32,
                        ));
                    write_file(&path, &payload.payload());
                    self.level_files.push(path);
                }
                Representation::Container => {
                    let key = (payload.uuid.clone(), payload.digest.clone());
                    match containers.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, levels)) => levels.push(payload),
                        None => containers.push((key, vec![payload])),
                    }
                }
            }
        }

        for ((uuid, digest), levels) in containers {
            let shapes: Vec<(u16, u16)> = levels
                .iter()
                .map(|level| {
                    (
                        u16::try_from(level.width).expect("a JPEG dimension fits a u16"),
                        u16::try_from(level.height).expect("a JPEG dimension fits a u16"),
                    )
                })
                .collect();
            let mut file = bytes::chunk("header", &bytes::header(&shapes), 0);
            for (position, level) in levels.iter().enumerate() {
                file.extend(bytes::chunk(
                    &format!("level_{}", position + 1),
                    &level.payload(),
                    0,
                ));
            }
            write_file(&self.container_path(&uuid, &digest), &file);
            self.containers.push(self.container_path(&uuid, &digest));
        }

        for (uuid, digest, bytes) in &self.verbatim {
            write_file(&self.container_path(uuid, digest), bytes);
            self.containers.push(self.container_path(uuid, digest));
        }
    }

    /// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`.
    fn container_path(&self, uuid: &str, digest: &str) -> PathBuf {
        self.previews()
            .join(self.shard(uuid))
            .join(format!("{uuid}-{digest}.lrprev"))
    }

    fn write_unrecognised(&self) {
        for file in &self.unrecognised {
            let path = match file.shape {
                UnrecognisedShape::NoSuffix => {
                    // Right shard, the leaf shapecast: no `_<long_edge>`.
                    let uuid = "AAAA0007-1111-4111-8111-000000000007";
                    self.previews()
                        .join(self.shard(uuid))
                        .join(format!("{uuid}-{}", digest_of(uuid)))
                }
                UnrecognisedShape::WrongShard => {
                    // Right leaf, a shard folder its uuid does not derive.
                    let uuid = "AAAA0008-1111-4111-8111-000000000008";
                    self.previews().join("0").join("0000").join(bytes::leaf(
                        uuid,
                        &digest_of(uuid),
                        1616,
                    ))
                }
                UnrecognisedShape::BadUuid => {
                    // A leaf whose uuid cannot be parsed.
                    self.previews()
                        .join("Z")
                        .join("ZZZZ")
                        .join("not-a-uuid-0123456789abcdef_1616")
                }
            };
            write_file(&path, &file.payload());
        }
    }

    fn write_preoccupied(&self) {
        for (relative, bytes) in &self.preoccupied {
            write_file(&self.out().join(relative), bytes);
        }
    }

    fn write_originals(&self) {
        for image in self.images.iter().filter(|image| image.original_on_disk) {
            let path = self.original_path(image);
            write_file(&path, b"not a preview");
        }
    }

    /// `<root><pathFromRoot><baseName>.CR2`, joined component-wise like
    /// `catalog::original_path` so both separators are honoured.
    fn original_path(&self, image: &ImageRow) -> PathBuf {
        let mut path = PathBuf::from(self.root_folder_value().trim_end_matches(['/', '\\']));
        for part in image.base_name.split(['/', '\\']) {
            if !part.is_empty() {
                path.push(part);
            }
        }
        path.set_extension(EXTENSION);
        path
    }

    fn root_folder_value(&self) -> String {
        match &self.root_folder {
            Some(absolute) => absolute.clone(),
            None => format!(
                "{}/",
                self.dir
                    .path()
                    .join("originals")
                    .to_string_lossy()
                    .replace('\\', "/")
            ),
        }
    }

    /// The two shard directories a uuid lives under (§5.2).
    fn shard(&self, uuid: &str) -> PathBuf {
        let (first, first_four) = bytes::leaf_dir(uuid);
        PathBuf::from(first).join(first_four)
    }
}

/// A 32-character lower-case hex digest derived from the uuid.
///
/// Every synthetic pyramid gets one, and both the pyramid's rows and the file
/// that carries it derive it the same way, so they always agree. Real caches
/// share one digest store-wide (§5.2); nothing here depends on that.
pub fn digest_of(uuid: &str) -> String {
    let hex: String = uuid
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    let seed = if hex.is_empty() { "0" } else { hex.as_str() };
    let mut digest = String::with_capacity(32);
    while digest.len() < 32 {
        digest.push_str(seed);
    }
    digest.truncate(32);
    digest
}

/// The three paths a vault's name derives.
fn paths(dir: &Path, name: &str) -> (PathBuf, PathBuf, PathBuf) {
    (
        dir.join("catalog").join(format!("{name}.lrcat")),
        dir.join(format!("{name} Previews.lrdata")),
        dir.join("out"),
    )
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create a fixture directory");
    }
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

#[cfg(test)]
mod tests {
    use lrsalvage_core::previews_db::PyramidKey;

    use super::*;

    const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    tiff:Make="SONY"/>
 </rdf:RDF>
</x:xmpmeta>"#;

    const UUID: &str = "AAAA0001-1111-4111-8111-000000000001";

    #[test]
    fn a_legacy_vault_writes_a_container_the_plan_can_recover() {
        let vault = Vault::legacy()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID).xmp(XMP))
            .pyramid(PyramidRow::new(UUID).profile("AdobeRGB").level(1616, 1077))
            .level_file(Level::sized(UUID, 1616, 1077))
            .write();

        // The container is at the legacy address, not at a level file.
        let payload = vault
            .previews()
            .join("A")
            .join("AAAA")
            .join(format!("{UUID}-{}.lrprev", digest_of(UUID)));
        assert!(payload.is_file(), "{}", payload.display());

        let plan = vault.plan(&Options::default());
        assert_eq!(plan.stats.images, 1);
        assert_eq!(plan.stats.recoverable, 1);
        let item = &plan.items[0];
        assert_eq!(item.color_profile.as_deref(), Some("AdobeRGB"));
        assert_eq!((item.width, item.height), (Some(1616), Some(1077)));
    }

    #[test]
    fn a_modern_vault_writes_a_level_file_at_its_derived_leaf() {
        // Row longDimension is REAL and the suffix is the rounded integer (§5.7).
        let vault = Vault::modern()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .pyramid(PyramidRow::new(UUID).level(101, 68))
            .level_file(Level::sized(UUID, 101, 68).long_dimension(101.0))
            .write();

        let leaf =
            vault
                .previews()
                .join("A")
                .join("AAAA")
                .join(bytes::leaf(UUID, &digest_of(UUID), 101));
        assert!(leaf.is_file(), "{}", leaf.display());

        let index = previews_db::open(&vault.previews().join("previews.db")).index;
        assert_eq!(
            index
                .best_level(&PyramidKey::new(UUID, digest_of(UUID)))
                .expect("declared level")
                .width,
            101
        );
    }

    #[test]
    fn the_xmp_blob_inflates_back_into_metadata() {
        let vault = Vault::modern()
            .image(ImageRow::new(1, "IMG_0001").xmp(XMP))
            .write();
        let (_snapshot, catalog) = Catalog::open(vault.catalog()).expect("open");
        let metadata = catalog
            .xmp(1)
            .expect("readable metadata")
            .expect("metadata row");
        assert_eq!(metadata.make.as_deref(), Some("SONY"));
    }

    #[test]
    fn an_unindexed_vault_has_payload_files_and_no_database() {
        let vault = Vault::unindexed()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .level_file(Level::sized(UUID, 1616, 1077))
            .write();
        assert!(!vault.previews().join("previews.db").exists());
        assert!(vault.previews().is_dir());
    }

    #[test]
    fn a_preoccupied_destination_lands_in_the_output_tree() {
        let vault = Vault::modern()
            .preoccupied_destination("2015/IMG_0001.jpg", &bytes::jpeg(1616, 1077))
            .write();
        assert!(vault.out().join("2015").join("IMG_0001.jpg").is_file());
    }

    #[test]
    fn two_vaults_can_share_one_destination() {
        let first = Vault::modern()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .level_file(Level::sized(UUID, 1616, 1077))
            .write();
        let second = Vault::modern()
            .same_destination_as(&first)
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .level_file(Level::sized(UUID, 1616, 1077))
            .write();

        // One output tree, two of everything else: the second catalog's name
        // derives the same destination from its own `rootFolder` (row M15).
        assert_eq!(second.out(), first.out());
        assert!(second.out().is_dir(), "the shared tree is the first's");
        assert_ne!(second.catalog(), first.catalog());
        assert_ne!(second.previews(), first.previews());
    }

    #[test]
    fn a_superseded_pyramid_has_level_files_and_an_unreadable_container() {
        let vault = Vault::mixed()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .pyramid(PyramidRow::new(UUID).level(1616, 1077))
            .superseded(&[Level::sized(UUID, 1616, 1077)])
            .write();

        let leaf =
            vault
                .previews()
                .join("A")
                .join("AAAA")
                .join(bytes::leaf(UUID, &digest_of(UUID), 1616));
        assert!(leaf.is_file(), "{}", leaf.display());
        assert_eq!(
            vault.level_files(),
            &[leaf],
            "the level file the composition wrote"
        );
        assert_eq!(
            vault.containers(),
            &[vault.container_path(UUID, &digest_of(UUID))],
            "and the superseded container beside it"
        );

        // The container beside it is not one, so reading it fails the row
        // rather than superseding the level file (ADR 0006 ¶6).
        let container = vault.container_path(UUID, &digest_of(UUID));
        let bytes = fs::read(&container).expect("the superseded container");
        assert!(
            lrsalvage_core::lrprev::Lrprev::parse(&bytes).is_err(),
            "{} must not parse as a container",
            container.display()
        );
    }

    #[test]
    fn a_pruned_row_and_an_orphan_file_are_written_apart() {
        let pruned = "AAAA0002-1111-4111-8111-000000000002";
        let orphan = "AAAA0003-1111-4111-8111-000000000003";
        let vault = Vault::damaged()
            .image(ImageRow::new(1, "IMG_0001").cache(pruned))
            .row_without_file(Level::sized(pruned, 1616, 1077))
            .file_without_row(Level::sized(pruned, 808, 539))
            .file_without_row(Level::sized(orphan, 640, 427))
            .tombstone(orphan, 700_000_000.0)
            .write();

        let index = previews_db::open(&vault.previews().join("previews.db")).index;
        // The row exists with no file behind it, and the file beside it has no row.
        let pruned_key = PyramidKey::new(pruned, digest_of(pruned));
        assert_eq!(index.levels(&pruned_key).len(), 1);
        assert_eq!(index.levels(&pruned_key)[0].width, 1616);
        assert!(
            index
                .levels(&PyramidKey::new(orphan, digest_of(orphan)))
                .is_empty()
        );

        let file = vault.previews().join("A").join("AAAA").join(bytes::leaf(
            pruned,
            &digest_of(pruned),
            808,
        ));
        assert!(file.is_file(), "{}", file.display());

        let conn = rusqlite::Connection::open(vault.previews().join("previews.db"))
            .expect("open previews.db");
        let tombstones: i64 = conn
            .query_row("SELECT count(*) FROM DeletedPyramid", [], |row| row.get(0))
            .expect("count tombstones");
        assert_eq!(tombstones, 1);
    }

    #[test]
    fn a_digest_is_thirty_two_lower_case_hex_characters() {
        let digest = digest_of("ZZZZ0009-1111-4111-8111-000000000009");
        assert_eq!(digest.len(), 32);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }
}
