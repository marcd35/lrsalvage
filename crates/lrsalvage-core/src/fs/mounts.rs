//! The browser's namespace: a table of path prefixes, and which storage answers.
//!
//! The seam's signatures take `&Path` and nothing else, so on this target a path
//! has to say where it resolves to without a second argument. It says it with a
//! prefix: a mount is a root and a kind, the longest matching root wins, and a
//! path under none of them does not resolve at all.
//!
//! Nothing here talks to a browser. That is deliberate — the arithmetic is the
//! part of the browser half that can be wrong on the host, so it is compiled on
//! both targets and its tests run natively (`cargo test -p lrsalvage-core`).

use std::path::{Component, Path, PathBuf};

/// Which storage answers for a mount's paths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// The origin-private pool, answered in this process.
    Store,
    /// A directory handle or a picked file, answered by the host's bridge.
    Bridge,
}

/// One root, and what answers under it.
#[derive(Clone, Debug)]
pub(crate) struct Mount {
    pub(crate) root: PathBuf,
    pub(crate) kind: Kind,
}

impl Mount {
    pub(crate) fn new(root: &Path, store: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            kind: if store { Kind::Store } else { Kind::Bridge },
        }
    }
}

/// Whether `path` is at or under `root`, on a component boundary.
///
/// `Path::starts_with` already compares whole components, which is the property
/// that matters: `/source-two` is not under `/source`, however the strings look.
pub(crate) fn is_under(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// The mount that answers for `path`: the longest root that contains it.
pub(crate) fn find<'a>(mounts: &'a [Mount], path: &Path) -> Option<&'a Mount> {
    mounts.iter().find(|mount| is_under(path, &mount.root))
}

/// Order the table so the first prefix that matches is the most specific one.
pub(crate) fn sort_deepest_first(mounts: &mut [Mount]) {
    mounts.sort_by(|left, right| {
        right
            .root
            .components()
            .count()
            .cmp(&left.root.components().count())
    });
}

/// A directory's prefix in the flat store, with its trailing separator.
///
/// The separator is what makes `…/lrsalvage-1` and `…/lrsalvage-10` different
/// directories, which a plain string prefix would not.
pub(crate) fn dir_prefix(name: &str) -> String {
    format!("{}/", name.trim_end_matches('/'))
}

/// A path with no `.` and no `..`, and its root kept.
///
/// `canonicalize` promises an absolute, symlink-free spelling. This namespace has
/// no symlinks and no `..` that means anything else, so normalising the
/// components is the whole of the promise here.
pub(crate) fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deepest_mount_answers_first() {
        let mut mounts = [
            Mount::new(Path::new("/source"), false),
            Mount::new(Path::new("/source/catalog.lrcat"), false),
            Mount::new(Path::new("/store"), true),
        ];
        sort_deepest_first(&mut mounts);
        assert_eq!(
            find(&mounts, Path::new("/source/catalog.lrcat"))
                .expect("the nested mount")
                .root,
            Path::new("/source/catalog.lrcat")
        );
        assert_eq!(
            find(&mounts, Path::new("/source/catalog.lrcat-wal"))
                .expect("the mount that contains it")
                .root,
            Path::new("/source")
        );
        assert_eq!(
            find(&mounts, Path::new("/store/x")).expect("a mount").kind,
            Kind::Store
        );
        assert!(
            find(&mounts, Path::new("/elsewhere/x")).is_none(),
            "a path no mount answers for does not resolve"
        );
    }

    #[test]
    fn a_sibling_with_a_shared_prefix_is_not_under_the_mount() {
        let mounts = [Mount::new(Path::new("/source"), false)];
        assert!(find(&mounts, Path::new("/source-two/x")).is_none());
        assert!(find(&mounts, Path::new("/source")).is_some());
    }

    #[test]
    fn a_directory_prefix_is_segment_aligned() {
        let one = dir_prefix("/store/scratch/lrsalvage-1");
        assert_eq!(one, "/store/scratch/lrsalvage-1/");
        assert!("/store/scratch/lrsalvage-1/cat.lrcat".starts_with(&one));
        assert!(
            !"/store/scratch/lrsalvage-10/x".starts_with(&one),
            "ten is not one"
        );
        assert_eq!(dir_prefix("/store/a/"), "/store/a/");
    }

    #[test]
    fn normalise_resolves_dots_and_keeps_the_root() {
        assert_eq!(
            normalise(Path::new("/store/a/./b/../c")),
            PathBuf::from("/store/a/c")
        );
        assert_eq!(normalise(Path::new("/store/a")), PathBuf::from("/store/a"));
        assert_eq!(normalise(Path::new("/")), PathBuf::from("/"));
    }
}
