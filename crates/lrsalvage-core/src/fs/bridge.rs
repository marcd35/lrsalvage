//! The host bridge: the one call a page's storage answers through.
//!
//! A page has no filesystem and no synchronous handle, so the seam's synchronous
//! operations on a picked folder are answered by the host — a worker's JavaScript,
//! blocking however it has to. This module is the protocol between the two:
//! operation codes, a UTF-8 JSON request, a `[kind][bytes]` answer, and the
//! chunking and paging that keep one operation from needing the whole file or the
//! whole listing in memory at once.
//!
//! It is compiled on both targets because the protocol is the half of the browser
//! backend that can be wrong without a browser in the room: its tests run against
//! a fake host (`cargo test -p lrsalvage-core`), which is the only way a chunk
//! loop or a listing page is checked before a Chromium run exists.
//!
//! The JS half is `crates/lrsalvage-web/web/lrsalvage-fs.js`; the two lists of
//! operation codes below and there are one contract.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{Value, json};

/// The host's one blocking call.
///
/// `op` is an operation code; the request is UTF-8 JSON of `request_len` bytes at
/// `request_ptr`; the answer is written into `out_ptr`, at most `out_cap` bytes.
/// The return value is how many bytes were written, or a negative number when the
/// bridge itself failed, with a UTF-8 message left in the answer buffer.
pub(crate) type BridgeCall = unsafe extern "C" fn(
    op: u32,
    request_ptr: *const u8,
    request_len: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32;

pub(crate) const OP_READ: u32 = 0;
pub(crate) const OP_STAT: u32 = 1;
pub(crate) const OP_LIST: u32 = 2;
pub(crate) const OP_WALK: u32 = 3;
pub(crate) const OP_MKDIR: u32 = 4;
pub(crate) const OP_WRITE: u32 = 5;
pub(crate) const OP_CREATE_NEW: u32 = 6;
pub(crate) const OP_WRITE_ATOMIC: u32 = 7;
pub(crate) const OP_REMOVE: u32 = 8;
pub(crate) const OP_CANONICALIZE: u32 = 9;

/// The answer's first byte: `OK`, or why not.
const KIND_OK: u8 = 0;
const KIND_NOT_FOUND: u8 = 1;
const KIND_PERMISSION: u8 = 2;
const KIND_EXISTS: u8 = 3;
const KIND_UNSUPPORTED: u8 = 4;
const KIND_INVALID: u8 = 5;
const KIND_FULL: u8 = 6;

/// The largest answer the host may write in one call.
const OUT_CAP: usize = 512 * 1024;
/// How many bytes one read asks for. A payload sniff wants two and a level file
/// wants its whole self; this is the buffer's bound, not the answer's.
pub(crate) const CHUNK: usize = 256 * 1024;
/// How many names one listing page carries.
const PAGE: usize = 512;

const NO_BRIDGE: &str = "no browser bridge is installed on this target";

thread_local! {
    static BRIDGE: RefCell<Option<BridgeCall>> = const { RefCell::new(None) };
    /// One answer buffer, reused: a payload read is the hot path and a fresh
    /// half-megabyte per call would outgrow the heap this target has.
    static ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Take the host's call. Called once, before the first operation.
pub(crate) fn install(call: BridgeCall) {
    BRIDGE.with(|slot| *slot.borrow_mut() = Some(call));
}

/// Whether a host has wired one.
#[cfg(test)]
fn installed() -> bool {
    BRIDGE.with(|slot| slot.borrow().is_some())
}

fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

fn kind_of(code: u8) -> io::ErrorKind {
    match code {
        KIND_NOT_FOUND => io::ErrorKind::NotFound,
        KIND_PERMISSION => io::ErrorKind::PermissionDenied,
        KIND_EXISTS => io::ErrorKind::AlreadyExists,
        KIND_UNSUPPORTED => io::ErrorKind::Unsupported,
        KIND_INVALID => io::ErrorKind::InvalidInput,
        KIND_FULL => io::ErrorKind::StorageFull,
        _ => io::ErrorKind::Other,
    }
}

/// One call, with a JSON request and a byte answer.
fn call(op: u32, request: &Value) -> io::Result<Vec<u8>> {
    let call = BRIDGE
        .with(|slot| *slot.borrow())
        .ok_or_else(|| unsupported(NO_BRIDGE))?;
    let request = serde_json::to_vec(request)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;

    let mut out = ANSWER.with(|buffer| std::mem::take(&mut *buffer.borrow_mut()));
    if out.len() < OUT_CAP {
        out.resize(OUT_CAP, 0);
    }
    // SAFETY: `call` is the host's function. Both ranges are this call's own
    // allocations, the host writes only inside `out` (which is `out.len()` bytes
    // long), and it neither retains a pointer nor re-enters this module.
    let status = unsafe {
        call(
            op,
            request.as_ptr(),
            request.len(),
            out.as_mut_ptr(),
            out.len(),
        )
    };

    let answer = if status < 0 {
        let end = out.iter().position(|byte| *byte == 0).unwrap_or(out.len());
        Err(io::Error::other(format!(
            "the browser bridge failed: {}",
            String::from_utf8_lossy(&out[..end])
        )))
    } else if status == 0 {
        Err(io::Error::other("the browser bridge answered nothing"))
    } else {
        let length = (status as usize).min(out.len());
        let kind = out[0];
        let payload = out[1..length].to_vec();
        if kind == KIND_OK {
            Ok(payload)
        } else {
            Err(io::Error::new(
                kind_of(kind),
                String::from_utf8_lossy(&payload).into_owned(),
            ))
        }
    };

    out.clear();
    ANSWER.with(|buffer| *buffer.borrow_mut() = out);
    answer
}

fn call_json(op: u32, request: &Value) -> io::Result<Value> {
    let bytes = call(op, request)?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

/// What the host saw at a path.
#[derive(Debug)]
pub(crate) struct Stat {
    pub(crate) exists: bool,
    pub(crate) is_file: bool,
    pub(crate) len: u64,
}

fn render(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// One chunk of a file, shorter at the end.
pub(crate) fn read(path: &Path, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    let len = len.min(CHUNK);
    call(
        OP_READ,
        &json!({ "path": render(path), "offset": offset, "len": len }),
    )
}

pub(crate) fn stat(path: &Path) -> io::Result<Stat> {
    let answer = call_json(OP_STAT, &json!({ "path": render(path) }))?;
    Ok(Stat {
        exists: answer["exists"].as_bool().unwrap_or(false),
        is_file: answer["file"].as_bool().unwrap_or(false),
        len: answer["len"].as_u64().unwrap_or(0),
    })
}

/// One page of a directory's entries, and whether that was the last page.
pub(crate) fn list(path: &Path, from: usize) -> io::Result<(Vec<String>, bool)> {
    let answer = call_json(
        OP_LIST,
        &json!({ "path": render(path), "from": from, "count": PAGE }),
    )?;
    let names: Vec<String> = answer["names"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok((names, answer["done"].as_bool() != Some(false)))
}

/// One page of a tree's files, relative to `path`, and whether that was the last.
pub(crate) fn walk(path: &Path, from: usize) -> io::Result<(Vec<String>, bool)> {
    let answer = call_json(
        OP_WALK,
        &json!({ "path": render(path), "from": from, "count": PAGE }),
    )?;
    let names: Vec<String> = answer["names"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok((names, answer["done"].as_bool() != Some(false)))
}

pub(crate) fn mkdir(path: &Path) -> io::Result<()> {
    call(OP_MKDIR, &json!({ "path": render(path) })).map(|_| ())
}

fn body(op: u32, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    call(op, &json!({ "path": render(path), "bytes": encoded })).map(|_| ())
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    body(OP_WRITE, path, bytes)
}

pub(crate) fn create_new(path: &Path) -> io::Result<()> {
    call(OP_CREATE_NEW, &json!({ "path": render(path) })).map(|_| ())
}

/// One `createWritable` … `close`: the platform's atomic commit.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    body(OP_WRITE_ATOMIC, path, bytes)
}

pub(crate) fn remove(path: &Path) -> io::Result<()> {
    call(OP_REMOVE, &json!({ "path": render(path) })).map(|_| ())
}

pub(crate) fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let answer = call_json(OP_CANONICALIZE, &json!({ "path": render(path) }))?;
    answer["path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "the bridge answered no path"))
}

#[cfg(test)]
mod tests {
    //! A fake host, so the protocol's arithmetic is checked on the host before a
    //! browser exists: the request shapes, the answer envelope, the chunk loop's
    //! end condition, and a listing that needs more than one page.
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Tree {
        files: BTreeMap<String, Vec<u8>>,
        dirs: Vec<String>,
    }

    thread_local! {
        static TREE: RefCell<Tree> = RefCell::new(Tree::default());
    }

    fn write_answer(out: *mut u8, out_cap: usize, kind: u8, body: &[u8]) -> i32 {
        let total = 1 + body.len();
        assert!(
            total <= out_cap,
            "the fake host must not overflow the answer buffer"
        );
        unsafe {
            std::ptr::write_bytes(out, 0, 1);
            *out = kind;
            if !body.is_empty() {
                std::ptr::copy_nonoverlapping(body.as_ptr(), out.add(1), body.len());
            }
        }
        total as i32
    }

    fn answer(out: *mut u8, out_cap: usize, kind: u8, body: Vec<u8>) -> i32 {
        write_answer(out, out_cap, kind, &body)
    }

    fn missing(out: *mut u8, out_cap: usize, path: &str) -> i32 {
        answer(out, out_cap, KIND_NOT_FOUND, path.as_bytes().to_vec())
    }

    /// The fake's own dispatch: it decodes exactly what the real host decodes.
    unsafe extern "C" fn fake(
        op: u32,
        request: *const u8,
        len: usize,
        out: *mut u8,
        cap: usize,
    ) -> i32 {
        let request = unsafe { std::slice::from_raw_parts(request, len) };
        let Ok(request) = serde_json::from_slice::<Value>(request) else {
            return answer(out, cap, KIND_INVALID, b"bad request".to_vec());
        };
        let path = request["path"].as_str().unwrap_or_default().to_owned();
        match op {
            OP_READ => {
                let offset = request["offset"].as_u64().unwrap_or(0) as usize;
                let want = request["len"].as_u64().unwrap_or(0) as usize;
                TREE.with(|tree| {
                    let tree = tree.borrow();
                    let Some(bytes) = tree.files.get(&path) else {
                        return missing(out, cap, &path);
                    };
                    let start = offset.min(bytes.len());
                    let end = (start + want).min(bytes.len());
                    answer(out, cap, KIND_OK, bytes[start..end].to_vec())
                })
            }
            OP_STAT => TREE.with(|tree| {
                let tree = tree.borrow();
                let body = if let Some(bytes) = tree.files.get(&path) {
                    json!({ "exists": true, "file": true, "len": bytes.len() })
                } else if tree.dirs.iter().any(|dir| dir == &path) {
                    json!({ "exists": true, "file": false, "len": 0 })
                } else {
                    json!({ "exists": false, "file": false, "len": 0 })
                };
                answer(out, cap, KIND_OK, body.to_string().into_bytes())
            }),
            OP_LIST | OP_WALK => {
                let from = request["from"].as_u64().unwrap_or(0) as usize;
                let count = request["count"].as_u64().unwrap_or(0) as usize;
                TREE.with(|tree| {
                    let tree = tree.borrow();
                    if !tree.dirs.iter().any(|dir| dir == &path) && !tree.files.contains_key(&path)
                    {
                        return missing(out, cap, &path);
                    }
                    let prefix = format!("{}/", path.trim_end_matches('/'));
                    let mut names: Vec<String> = tree
                        .files
                        .keys()
                        .filter_map(|name| name.strip_prefix(&prefix))
                        .filter(|rest| op == OP_WALK || !rest.contains('/'))
                        .map(str::to_owned)
                        .collect();
                    names.sort();
                    let page: Vec<String> = names.iter().skip(from).take(count).cloned().collect();
                    let done = from + page.len() >= names.len();
                    let body = json!({ "names": page, "done": done });
                    answer(out, cap, KIND_OK, body.to_string().into_bytes())
                })
            }
            OP_MKDIR => TREE.with(|tree| {
                tree.borrow_mut().dirs.push(path);
                answer(out, cap, KIND_OK, Vec::new())
            }),
            OP_WRITE | OP_WRITE_ATOMIC => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(request["bytes"].as_str().unwrap_or_default())
                    .unwrap_or_default();
                TREE.with(|tree| {
                    tree.borrow_mut().files.insert(path, bytes);
                    answer(out, cap, KIND_OK, Vec::new())
                })
            }
            OP_CREATE_NEW => TREE.with(|tree| {
                let mut tree = tree.borrow_mut();
                if tree.files.contains_key(&path) {
                    return answer(out, cap, KIND_EXISTS, path.into_bytes());
                }
                tree.files.insert(path, Vec::new());
                answer(out, cap, KIND_OK, Vec::new())
            }),
            OP_REMOVE => TREE.with(|tree| {
                let mut tree = tree.borrow_mut();
                if tree.files.remove(&path).is_some() {
                    answer(out, cap, KIND_OK, Vec::new())
                } else {
                    missing(out, cap, &path)
                }
            }),
            OP_CANONICALIZE => {
                let normalised = crate::fs::mounts::normalise(Path::new(&path));
                let body = json!({ "path": normalised.to_string_lossy() });
                answer(out, cap, KIND_OK, body.to_string().into_bytes())
            }
            _ => answer(out, cap, KIND_UNSUPPORTED, b"no such operation".to_vec()),
        }
    }

    fn host(files: &[(&str, &[u8])], dirs: &[&str]) {
        install(fake);
        TREE.with(|tree| {
            let mut tree = tree.borrow_mut();
            *tree = Tree::default();
            for (path, bytes) in files {
                tree.files.insert((*path).to_owned(), bytes.to_vec());
            }
            for dir in dirs {
                tree.dirs.push((*dir).to_owned());
            }
        });
    }

    #[test]
    fn a_file_reads_back_through_its_chunks() {
        let body: Vec<u8> = (0..(CHUNK * 2 + 7)).map(|index| index as u8).collect();
        host(&[("/source/big.bin", &body)], &["/source"]);

        let mut read = Vec::new();
        let mut offset = 0u64;
        while offset < body.len() as u64 {
            let chunk = read_chunk(&mut read, Path::new("/source/big.bin"), offset).expect("chunk");
            assert!(chunk > 0);
            offset += chunk as u64;
        }
        assert_eq!(read, body, "every byte, in order");
    }

    /// The test's own read loop, separate from the backend's, so the backend's
    /// loop is not checked against itself.
    fn read_chunk(out: &mut Vec<u8>, path: &Path, offset: u64) -> io::Result<usize> {
        let chunk = read(path, offset, CHUNK)?;
        out.extend_from_slice(&chunk);
        Ok(chunk.len())
    }

    #[test]
    fn a_short_file_answers_what_it_has() {
        host(&[("/source/two.bin", b"\xFF\xD8rest")], &["/source"]);
        assert_eq!(
            read(Path::new("/source/two.bin"), 0, 2).expect("two"),
            b"\xFF\xD8"
        );
        assert_eq!(
            read(Path::new("/source/two.bin"), 0, 4096).expect("short"),
            b"\xFF\xD8rest"
        );
        assert_eq!(
            read(Path::new("/source/two.bin"), 999, 2).expect("past the end"),
            b"",
            "past the end is an empty chunk, not an error"
        );
    }

    #[test]
    fn a_listing_pages_until_it_says_it_is_done() {
        let names: Vec<String> = (0..(PAGE + 3))
            .map(|index| format!("/source/f{index}"))
            .collect();
        let files: Vec<(&str, &[u8])> = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect();
        host(&files, &["/source"]);

        let mut listed = Vec::new();
        let mut from = 0usize;
        loop {
            let (page, done) = list(Path::new("/source"), from).expect("page");
            from += page.len();
            listed.extend(page);
            if done {
                break;
            }
        }
        listed.sort();
        assert_eq!(listed.len(), PAGE + 3, "every name, across two pages");
    }

    #[test]
    fn a_walk_answers_descendents_and_a_listing_does_not() {
        host(
            &[("/cache/top_640", b"t"), ("/cache/0F/0F12/deep_640", b"d")],
            &["/cache", "/cache/0F", "/cache/0F/0F12"],
        );
        let (page, _) = list(Path::new("/cache"), 0).expect("list");
        assert_eq!(page, vec!["top_640"], "one level, files only");
        let (mut page, _) = walk(Path::new("/cache"), 0).expect("walk");
        page.sort();
        assert_eq!(page, vec!["0F/0F12/deep_640", "top_640"]);
    }

    #[test]
    fn stat_separates_a_file_a_directory_and_nothing() {
        host(&[("/source/present.bin", b"x")], &["/source"]);
        let file = stat(Path::new("/source/present.bin")).expect("stat");
        assert!(file.exists && file.is_file);
        assert_eq!(file.len, 1);
        let dir = stat(Path::new("/source")).expect("stat");
        assert!(dir.exists && !dir.is_file);
        let absent = stat(Path::new("/source/absent")).expect("stat");
        assert!(!absent.exists && !absent.is_file);
    }

    #[test]
    fn writing_creating_and_removing_report_what_the_host_did() {
        host(&[], &["/source"]);
        write(Path::new("/source/w.bin"), b"bytes").expect("write");
        assert_eq!(
            read(Path::new("/source/w.bin"), 0, 64).expect("read"),
            b"bytes"
        );

        create_new(Path::new("/source/n.bin")).expect("create");
        let error = create_new(Path::new("/source/n.bin")).expect_err("a taken path");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

        write_atomic(Path::new("/source/w.bin"), b"new").expect("atomic");
        assert_eq!(
            read(Path::new("/source/w.bin"), 0, 64).expect("read"),
            b"new"
        );

        remove(Path::new("/source/w.bin")).expect("remove");
        assert_eq!(
            remove(Path::new("/source/w.bin")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn canonicalize_answers_a_normalised_path() {
        host(&[], &["/store"]);
        assert_eq!(
            canonicalize(Path::new("/store/a/./b/../c")).expect("canonical"),
            PathBuf::from("/store/a/c")
        );
    }

    #[test]
    fn without_a_host_every_call_is_unsupported() {
        BRIDGE.with(|slot| *slot.borrow_mut() = None);
        assert!(!installed());
        assert_eq!(
            stat(Path::new("/source/x")).unwrap_err().kind(),
            io::ErrorKind::Unsupported,
            "a page that wired nothing has to say so, not report absence"
        );
    }

    #[test]
    fn a_host_that_is_out_of_space_is_a_storage_full_error() {
        // The one answer code with no operation of its own: a quota refusal
        // arrives as `KIND_FULL` from whichever write the browser ran out on, and
        // the seam has to report it as `StorageFull` for a caller to tell it from
        // a permission problem.
        unsafe extern "C" fn full(
            _op: u32,
            _request: *const u8,
            _len: usize,
            out: *mut u8,
            cap: usize,
        ) -> i32 {
            answer(out, cap, KIND_FULL, b"quota exceeded".to_vec())
        }
        install(full);
        assert_eq!(
            stat(Path::new("/source/x")).unwrap_err().kind(),
            io::ErrorKind::StorageFull
        );
        BRIDGE.with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn an_unknown_operation_is_refused_rather_than_guessed() {
        host(&[], &[]);
        assert_eq!(
            call(999, &json!({})).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
