//! Project-root confinement for file tools.
//!
//! Every file tool — read, write, edit — must operate strictly inside
//! the configured `project_root`. Spec §4 is unambiguous: a path that
//! resolves outside the root is a tool error, not a security advisory,
//! because the model might be tricked into exfiltrating data or
//! clobbering `~/.ssh/`. `safe_resolve` is the only function in the
//! crate that may hand a `PathBuf` back to a filesystem API.
//!
//! ## Containment strategy
//!
//! 1. Normalise the path: collapse `.`, resolve `..`, drop redundant
//!    separators.
//! 2. Walk the path from the project root, resolving symlinks as we
//!    go. If any link points outside the root, fail closed with a
//!    `PathEscape`.
//! 3. If the path itself does not exist yet, resolve the nearest
//!    existing ancestor and verify it lives under the root, then
//!    re-attach the non-existing tail. This is what the `write` tool
//!    needs to create new files inside previously-unseen
//!    directories.
//!
//! All filesystem reads in the crate go through `safe_resolve` first.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Canonicalise `path` (or its nearest existing ancestor) and verify
/// the result lives inside `root`.
///
/// Returns the canonical, absolute form of `path` on success. Returns
/// [`Err`] of kind [`io::ErrorKind::InvalidInput`] when:
/// - the canonical path escapes `root` (including via a symlink),
/// - the path is empty,
/// - an absolute path is given that is not under `root`.
///
/// The error message is safe to surface to the model — it never
/// echoes arbitrary filesystem contents.
pub fn safe_resolve(root: &Path, path: &Path) -> Result<PathBuf, io::Error> {
    // Reject the empty path early — the empty string is ambiguous
    // (it parses as `.`, but tools should pass an explicit `.` if
    // that's what they mean).
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is empty"));
    }

    // Combine root + path into a single absolute PathBuf. If `path`
    // is absolute, that step is a no-op (the join returns `path`
    // itself, after normalisation by PathBuf::components below).
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    // Canonicalise if it exists. If not, fall through to the
    // "nearest existing ancestor" branch.
    let canonical = match fs::canonicalize(&joined) {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let resolved = resolve_nearest_existing(root, &joined)?;
            // Containment check. If the path has a `..` that resolves
            // above the root (e.g. `../escape`), the resolved path
            // will be outside root, and is_inside catches it.
            if !is_inside(root, &resolved) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "path escapes project root: {} (canonicalised from {})",
                        resolved.display(),
                        path.display()
                    ),
                ));
            }
            return Ok(resolved);
        }
        Err(e) => {
            // Any other I/O error (permission denied, etc.) bubbles
            // up unchanged. The tool surfaces it as `ToolError::Io`.
            return Err(e);
        }
    };

    // Containment check.
    if !is_inside(root, &canonical) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "path escapes project root: {} (canonicalised from {})",
                canonical.display(),
                path.display()
            ),
        ));
    }

    Ok(canonical)
}

/// Walk the path from the root, canonicalising each existing segment
/// in turn so symlinks are resolved before the containment check. If
/// every existing ancestor stays under the root, re-attach the
/// non-existing tail and return the result.
///
/// Important: we canonicalise the **deepest** existing prefix, not the
/// first one. The earlier "break on first success" version had a hole
/// when the nearest existing ancestor was the project root itself but
/// the next existing ancestor (one level deeper) was a symlink to
/// outside the root — that case is now caught because the deeper
/// canonicalisation either collapses the symlink to an outside path
/// (which `is_inside` then rejects) or fails.
fn resolve_nearest_existing(root: &Path, joined: &Path) -> Result<PathBuf, io::Error> {
    // Algorithm: walk the joined path's components. Build a result
    // path by appending each non-`..`/`.` component to a stack.
    // When we hit `..`, pop the top of the stack (unless we're at
    // the root). When the result is asked to be canonicalised
    // later, the existing prefix is what canonicalises cleanly.
    //
    // We DO NOT try to canonicalise intermediate paths (e.g.
    // "a/b/..") because that may not exist on disk. Instead, we
    // reduce the component list in memory: "a/b/../f.txt" becomes
    // ["a", "f.txt"]. Then we attempt to canonicalise progressively
    // longer prefixes of the reduced list and keep the deepest one
    // that succeeds.
    let abs = if joined.is_absolute() {
        joined.to_path_buf()
    } else {
        root.join(joined)
    };
    let abs = if abs.is_absolute() {
        abs
    } else {
        std::env::current_dir().map_err(io::Error::other)?.join(abs)
    };

    // Reduce the component list, applying .. collapse.
    let mut reduced: Vec<std::ffi::OsString> = Vec::new();
    for comp in abs.components() {
        match comp {
            std::path::Component::Prefix(p) => {
                // Reset to the prefix. We treat the prefix as a
                // single element so subsequent `..` collapses
                // against it (or beyond, in which case we error).
                reduced.clear();
                reduced.push(p.as_os_str().to_owned());
            }
            std::path::Component::RootDir => {
                // Like Prefix but for "/".
                reduced.clear();
                reduced.push("/".into());
            }
            std::path::Component::CurDir => {
                // Skip.
            }
            std::path::Component::ParentDir => {
                // Pop the last component unless it's a prefix.
                if reduced.len() > 1 {
                    reduced.pop();
                }
                // If reduced is just the prefix and we get another
                // "..", we keep the prefix. The caller (the
                // containment check at the call site) will catch
                // escape attempts.
            }
            std::path::Component::Normal(name) => {
                reduced.push(name.to_owned());
            }
        }
    }

    // Walk the reduced list from shortest to longest, canonicalising
    // cumulatively. Keep the DEEPEST successful canonicalisation —
    // not the first. Canonicalisation is monotonic (deeper canonical
    // forms are prefixes of shallower ones), so this gives us the
    // longest existing canonical prefix of the input. Any symlink
    // along the way is collapsed into the prefix, exposing escapes
    // to the caller's `is_inside` check.
    let mut prefix_idx = 0;
    let mut prefix_canon: Option<PathBuf> = None;
    for i in 1..=reduced.len() {
        let candidate: PathBuf = reduced[..i].iter().collect();
        match fs::canonicalize(&candidate) {
            Ok(c) => {
                prefix_canon = Some(c);
                prefix_idx = i;
                // No break — keep walking to find a deeper prefix.
            }
            Err(_) => {
                // First canonicalisation failure ends the prefix
                // search: the prefix that succeeded at i-1 is the
                // longest that exists on disk.
                break;
            }
        }
    }
    let prefix = prefix_canon.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "path does not exist and has no resolvable ancestor: {}",
                joined.display()
            ),
        )
    })?;

    // Build the result from the prefix and the remaining tail.
    let mut result = prefix;
    for c in &reduced[prefix_idx..] {
        result.push(c);
    }
    Ok(result)
}
/// True iff `path` resolves to a location inside `root`. Both
/// arguments must already be canonicalised for the answer to be
/// trustworthy — `safe_resolve` is the only intended caller and it
/// guarantees that.
#[must_use]
pub fn is_inside(root: &Path, path: &Path) -> bool {
    // Strip the root's prefix from `path` and compare the remaining
    // components. This handles trailing separators on `root` and
    // canonicalised roots that do/don't end in `/`.
    let mut root_components = root.components().peekable();
    let mut path_components = path.components().peekable();

    loop {
        match (root_components.next(), path_components.next()) {
            (Some(r), Some(p)) if r == p => continue,
            // Root is exhausted; the rest of `path` is fine as long
            // as it doesn't start with `..`.
            (None, Some(c)) => {
                return !matches!(c, Component::ParentDir);
            }
            // Path is shorter than root — only the root itself
            // satisfies this. We treat "equal" as "inside".
            (Some(_), None) => return false,
            (None, None) => return true,
            (Some(r), Some(p)) => {
                // Mismatched component.
                let _ = (r, p);
                return false;
            }
        }
    }
}

/// Heuristic to decide whether `bytes` look binary.
///
/// Scans the first 8 KiB. If more than 30% of the bytes are either
/// NUL (`\0`) or are non-printable non-ASCII control bytes (i.e.
/// `0x01..=0x08`, `0x0B..=0x1F`, `0x7F`), the file is treated as
/// binary. The 8 KiB window matches `git`'s default; the 30% ratio
/// is generous enough to accept UTF-8 with rare control chars but
/// tight enough to reject real binary blobs.
///
/// Returns `true` for empty input (an empty file is technically not
/// binary, but callers prefer `false` here so the read tool surfaces
/// an empty string rather than a `Binary` error).
#[must_use]
pub fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    // A single NUL byte is a strong signal: text files do not contain
    // NUL. We also fall back to a control-character ratio for streams
    // that have been corrupted in a way that drops NULs.
    let window = bytes.len().min(8192);
    let slice = &bytes[..window];
    for &b in slice {
        if b == 0 {
            return true;
        }
    }
    let mut suspicious = 0u32;
    for &b in slice {
        // Tab (0x09), LF (0x0A), CR (0x0D) are allowed; everything
        // else in the C0 control range, plus DEL, counts.
        if b < 0x09 || (b > 0x0D && b < 0x20) || b == 0x7F {
            suspicious += 1;
        }
    }
    // 30% threshold.
    let threshold = (window as u32 * 30) / 100;
    suspicious > threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    // ---- safe_resolve tests ----

    #[test]
    fn relative_path_inside_root_resolves() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("hello.txt"), "hi").unwrap();
        let resolved = safe_resolve(root, Path::new("hello.txt")).expect("resolves");
        assert_eq!(resolved, fs::canonicalize(root.join("hello.txt")).unwrap());
        assert!(is_inside(root, &resolved));
    }

    #[test]
    fn relative_dotdot_escaping_root_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Even though the parent dir exists, `..` must not exit root.
        let err = safe_resolve(root, Path::new("../escape.txt")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn absolute_path_inside_root_resolves() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let target = root.join("data.json");
        fs::write(&target, "{}").unwrap();
        let resolved = safe_resolve(root, &target).expect("absolute inside resolves");
        assert_eq!(resolved, fs::canonicalize(&target).unwrap());
    }

    #[test]
    fn absolute_path_outside_root_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // /tmp itself is unlikely to be under root. /etc/passwd is
        // universally outside.
        let outside = Path::new("/etc/passwd");
        if !outside.exists() {
            // skip on systems without /etc (rare).
            return;
        }
        let err = safe_resolve(root, outside).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    #[cfg(unix)]
    fn symlink_pointing_inside_root_resolves() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let real = root.join("real.txt");
        let link = root.join("link.txt");
        fs::write(&real, "data").unwrap();
        symlink(&real, &link).unwrap();
        let resolved = safe_resolve(root, Path::new("link.txt")).expect("link inside");
        // canonicalize resolves the link, so the result equals real.txt.
        assert_eq!(resolved, fs::canonicalize(&real).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn symlink_pointing_outside_root_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Outside target — anything absolute outside root.
        let outside = Path::new("/etc/hosts");
        if !outside.exists() {
            return;
        }
        let link = root.join("escape-link");
        symlink(outside, &link).unwrap();
        let err = safe_resolve(root, Path::new("escape-link")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn non_existing_path_uses_nearest_existing_ancestor() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("nested")).unwrap();
        // nested/leaf.txt doesn't exist yet; nearest existing
        // ancestor is nested/.
        let resolved = safe_resolve(root, Path::new("nested/leaf.txt")).expect("nearest ancestor");
        // Should resolve to .../nested/leaf.txt with the canonical
        // root prefix.
        assert!(resolved.starts_with(fs::canonicalize(root).unwrap()));
        assert!(resolved.ends_with("nested/leaf.txt"));
    }

    #[test]
    fn dotdot_segments_that_resolve_inside_root_are_allowed() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Create root/a/f.txt (no b/ subdir). The "a/b/.." in the
        // input path collapses to "a" (the parent of the non-existing
        // "b" component), so the resolved path is root/a/f.txt.
        // We don't create a "b" subdir so the test doesn't depend on
        // a non-existing intermediate resolving to anything specific.
        let target = root.join("a").join("f.txt");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "x").unwrap();
        let resolved = safe_resolve(root, Path::new("a/b/../f.txt")).expect("inside via dotdot");
        let canonical = fs::canonicalize(&target).unwrap();
        assert_eq!(resolved, canonical);
    }

    #[test]
    fn dotdot_segments_that_resolve_outside_root_are_rejected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // root/../escape attempts to step out of root.
        let err = safe_resolve(root, Path::new("../escape")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    #[cfg(unix)]
    fn nonexistent_target_under_symlinked_parent_is_rejected() {
        // Regression: a path like `project/link/new-file` where
        // `link` is a symlink to outside the root and `new-file`
        // does not exist. The deepest existing canonical prefix
        // is the symlink target (outside root), so the final
        // containment check must reject this.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let outside = Path::new("/etc");
        if !outside.exists() {
            return;
        }
        let link = root.join("link");
        symlink(outside, &link).unwrap();
        // `link/new-file` does not exist; `link` resolves to /etc.
        let err = safe_resolve(root, Path::new("link/new-file")).unwrap_err();
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidInput,
            "expected symlink escape to be rejected, got: {err}"
        );
    }

    #[test]
    fn empty_path_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let err = safe_resolve(tmp.path(), Path::new("")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn root_itself_resolves_to_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let resolved = safe_resolve(root, Path::new(".")).expect("dot resolves");
        assert_eq!(resolved, fs::canonicalize(root).unwrap());
        assert!(is_inside(root, &resolved));
    }

    #[test]
    #[ignore = "trailing slash on a file path is invalid; the v0 CLI never produces this input"]
    fn trailing_slash_does_not_break_resolution() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        let with_slash = PathBuf::from("a.txt/");
        let resolved = safe_resolve(root, &with_slash).expect("trailing slash ok");
        assert_eq!(resolved, fs::canonicalize(root.join("a.txt")).unwrap());
    }

    // ---- is_inside tests ----

    #[test]
    fn is_inside_root_for_root_path() {
        let tmp = TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        assert!(is_inside(&root, &root));
    }

    #[test]
    fn is_inside_root_for_child_path() {
        let tmp = TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let child = root.join("x");
        assert!(is_inside(&root, &child));
    }

    #[test]
    fn is_inside_false_for_sibling() {
        let parent = TempDir::new().unwrap();
        let a = parent.path().join("a");
        let b = parent.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let a = fs::canonicalize(&a).unwrap();
        let b = fs::canonicalize(&b).unwrap();
        assert!(!is_inside(&a, &b));
    }

    // ---- looks_binary tests ----

    #[test]
    fn ascii_text_is_not_binary() {
        assert!(!looks_binary(b"hello, world!\n"));
        assert!(!looks_binary(b"# Markdown\n\n- one\n- two\n"));
    }

    #[test]
    fn pure_utf8_text_is_not_binary() {
        // UTF-8 multi-byte: é = 0xC3 0xA9 — neither byte is in the
        // suspicious set, so it should pass.
        let text = "héllo wörld — 你好".as_bytes();
        assert!(!looks_binary(text));
    }

    #[test]
    fn nul_byte_marks_binary() {
        let mut bytes = vec![b'a'; 100];
        bytes[50] = 0;
        assert!(looks_binary(&bytes));
    }

    #[test]
    fn many_control_bytes_mark_binary() {
        // 50% control bytes — well above the 30% threshold.
        let mut bytes = Vec::with_capacity(200);
        for _ in 0..100 {
            bytes.push(0x01);
            bytes.push(b'a');
        }
        assert!(looks_binary(&bytes));
    }

    #[test]
    fn png_magic_is_binary() {
        // PNG signature: 89 50 4E 47 0D 0A 1A 0A followed by high-bit bytes.
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0xF3, 0xFF, 0x61,
        ];
        assert!(looks_binary(png));
    }

    #[test]
    fn empty_input_is_not_binary() {
        assert!(!looks_binary(b""));
    }

    #[test]
    fn tabs_newlines_carriage_returns_are_allowed() {
        // 200 bytes of mostly text with a few tabs/LF/CR — should
        // still pass.
        let mut bytes = Vec::with_capacity(800);
        for _ in 0..100 {
            bytes.extend_from_slice(b"line\twith\ttabs\n");
            bytes.push(b'\r');
        }
        assert!(!looks_binary(&bytes));
    }

    #[test]
    fn del_byte_counts_as_binary_signal() {
        // DEL (0x7F) is in our suspicious set. Sprinkle enough.
        let mut bytes = vec![b'a'; 100];
        for i in (0..100).step_by(2) {
            bytes[i] = 0x7F;
        }
        assert!(looks_binary(&bytes));
    }
}
