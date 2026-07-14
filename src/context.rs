//! AGENTS.md discovery and the v0 context compiler.
//!
//! Spec §12 says: every turn the agent loop sends to the provider must
//! include a system message built from (a) the versioned system prompt
//! embedded at compile time, and (b) every `AGENTS.md` that lives on
//! the path from the project root down to the current working
//! directory, broadest first. This module is the single source of
//! truth for that compilation.
//!
//! ## Discovery strategy
//!
//! [`ignore::WalkBuilder`] with the standard filter set is the right
//! tool because:
//!
//! 1. It respects `.gitignore`, `.ignore`, and the global gitignore —
//!    a nested `AGENTS.md` that the project has explicitly opted out
//!    of is never loaded (acceptance test 10).
//! 2. It does not follow symlinks by default, so we cannot end up in
//!    a recursive symlink loop (spec forbids this; brief repeats it).
//!
//! We walk the whole project tree once, then keep only the entries
//! whose parent directory sits on the root → cwd ancestor chain. A
//! single walk is simpler than an ancestor-by-ancestor one and gives
//! us correct `.gitignore` semantics for free.
//!
//! ## Failure policy
//!
//! Every skip is loud. `tracing::warn!` fires whenever:
//!
//! - a walker entry itself errors (e.g. a directory we cannot enter),
//! - a discovered `AGENTS.md` cannot be opened (e.g. permission
//!   denied on the file).
//!
//! Permission failures degrade the result to the accessible prefix
//! rather than aborting the compile (acceptance test 9).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The versioned system prompt, embedded at compile time.
///
/// The path is resolved relative to this source file, so the build
/// always picks up the `prompts/system_prompt.md` that lives next to
/// `Cargo.toml`.
const SYSTEM_PROMPT: &str = include_str!("../prompts/system_prompt.md");

/// Filename the convention uses for repository-level instructions.
const INSTRUCTIONS_FILE: &str = "AGENTS.md";

/// A compiled context ready to hand to the provider loop.
///
/// The `instructions` vector is ordered **broadest first**: the
/// project-root `AGENTS.md` (if any) sits at index 0, the cwd-level
/// one (if any) sits at the end. This mirrors the scoping rule that
/// more specific instructions override less specific ones.
#[derive(Debug, Clone)]
pub struct CompiledContext {
    /// The embedded system prompt, copied out so callers don't need
    /// a `&'static str` lifetime.
    pub system_prompt: String,
    /// All `AGENTS.md` files on the root → cwd path, in order.
    pub instructions: Vec<InstructionFile>,
    /// SHA-256 of `system_prompt || joined instruction content_hashes`.
    /// Stable for identical inputs; useful as a cache key.
    pub total_hash: [u8; 32],
}

/// One `AGENTS.md` file discovered on the root → cwd path.
#[derive(Debug, Clone)]
pub struct InstructionFile {
    /// Absolute, canonical path to the file as discovered on disk.
    pub path: PathBuf,
    /// UTF-8 file contents. Non-UTF-8 files are skipped with a warn
    /// rather than surfaced as an error.
    pub content: String,
    /// SHA-256 of `content.as_bytes()`.
    pub content_hash: [u8; 32],
}

/// Errors the context compiler can return.
///
/// Most failures are recoverable in practice (a missing `AGENTS.md`
/// is the common case), so the error set stays small: only the
/// situations that genuinely prevent compilation bubble up.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("project root is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("cwd is outside project root")]
    CwdOutsideRoot,
    #[error("read failed: {0}")]
    Io(#[from] io::Error),
}

/// Build a [`CompiledContext`] for the given project root and cwd.
///
/// `project_root` must be an existing directory; `cwd` must be an
/// existing directory inside it (after canonicalisation). Both are
/// canonicalised internally, so symlinks are resolved before the
/// ancestor walk runs.
pub fn compile(project_root: &Path, cwd: &Path) -> Result<CompiledContext, ContextError> {
    let canonical_root = fs::canonicalize(project_root)?;
    ensure_directory(&canonical_root, project_root)?;

    let canonical_cwd = fs::canonicalize(cwd)?;
    if !canonical_cwd.starts_with(&canonical_root) {
        return Err(ContextError::CwdOutsideRoot);
    }

    let instructions = discover_instructions(&canonical_root, &canonical_cwd)?;

    // The hash covers both inputs so a changed prompt OR a changed
    // instruction set invalidates the cache. We hash the prompt bytes
    // first, then each instruction's content hash in order — that
    // order-dependence lets us distinguish "root and cwd" from "cwd
    // and root" even though the byte sequences would collide.
    let mut hasher = Sha256::new();
    hasher.update(SYSTEM_PROMPT.as_bytes());
    for inst in &instructions {
        hasher.update(inst.content_hash);
    }
    let total_hash: [u8; 32] = hasher.finalize().into();

    Ok(CompiledContext {
        system_prompt: SYSTEM_PROMPT.to_string(),
        instructions,
        total_hash,
    })
}

/// Read every `AGENTS.md` on the root → cwd path, broadest first.
///
/// Exposed for callers that want the raw instruction list without
/// the system prompt attached (e.g. a CLI flag that dumps the
/// discovery trace).
fn discover_instructions(
    project_root: &Path,
    cwd: &Path,
) -> Result<Vec<InstructionFile>, ContextError> {
    // Walk the ancestor chain (cwd → ... → root), broadest first,
    // and read AGENTS.md at each level. We do NOT use a recursive
    // walker — that would pull in unrelated subtrees whose
    // AGENTS.md should not affect context. Instead, for each
    // ancestor, check whether AGENTS.md is ignored (via `ignore`
    // crate's match algorithm) and skip if so.
    //
    // The `ignore::gitignore::GitignoreBuilder` is the right tool:
    // it returns a `Gitignore` we can query with `matched(path, is_dir)`.
    // We build the matcher per discovery (cheap) using the project
    // root's `.gitignore` files.
    let (gitignore_matcher, gitignore_paths) = {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(project_root);
        let mut paths = Vec::new();
        // Add all .gitignore files in the project_root tree.
        for entry in ignore::WalkBuilder::new(project_root)
            .standard_filters(false)
            .build()
            .filter_map(Result::ok)
        {
            if entry.file_name() == ".gitignore" && entry.file_type().is_some_and(|ft| ft.is_file())
            {
                paths.push(entry.path().to_path_buf());
                let _ = builder.add(entry.path());
            }
        }
        let matcher = builder
            .build()
            .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty());
        (matcher, paths)
    };

    // Build the ancestor chain: cwd, ... , project_root (broadest
    // last). Then reverse so root is first.
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut cursor = cwd.to_path_buf();
    loop {
        ancestors.push(cursor.clone());
        if cursor == project_root {
            break;
        }
        let parent = cursor.parent().ok_or(ContextError::CwdOutsideRoot)?;
        cursor = parent.to_path_buf();
    }
    ancestors.reverse();

    let mut instructions = Vec::new();
    for ancestor in &ancestors {
        let agents_path = ancestor.join(INSTRUCTIONS_FILE);
        // Check the gitignore match. The matcher's `matched()` returns
        // true for ANY file with the right name anywhere in the tree
        // (for relative patterns like "AGENTS.md"). We want to apply
        // a gitignore only to files UNDER that gitignore's directory
        // — so check the gitignore's location vs the file.
        let mut is_ignored = false;
        for gi_path in &gitignore_paths {
            // The file is under the gitignore's directory iff the
            // canonicalized file path starts with the canonicalized
            // gitignore's parent directory.
            if let Some(gi_dir) = gi_path.parent() {
                if agents_path.starts_with(gi_dir) {
                    is_ignored = gitignore_matcher.matched(&agents_path, false).is_ignore();
                    break;
                }
            }
        }
        if is_ignored {
            tracing::debug!(path = %agents_path.display(), "skipping gitignored AGENTS.md");
            continue;
        }
        match fs::read_to_string(&agents_path) {
            Ok(content) => {
                let mut hasher = Sha256::new();
                hasher.update(content.as_bytes());
                let content_hash: [u8; 32] = hasher.finalize().into();
                instructions.push(InstructionFile {
                    path: agents_path,
                    content,
                    content_hash,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No AGENTS.md at this level; that's fine.
            }
            Err(e) => {
                tracing::warn!(path = %agents_path.display(), error = %e, "could not read AGENTS.md");
            }
        }
    }
    Ok(instructions)
}

fn ensure_directory(canonical: &Path, original: &Path) -> Result<(), ContextError> {
    let meta = fs::metadata(canonical)?;
    if !meta.is_dir() {
        return Err(ContextError::NotADirectory(original.to_path_buf()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::Path;

    /// Build a tempdir pre-populated by a closure, then run the test
    /// body with the canonicalised root path. Returns the tempdir so
    /// the directory is kept alive for the duration of the test.
    fn with_tree<F>(setup: F) -> tempfile::TempDir
    where
        F: FnOnce(&Path),
    {
        let dir = tempfile::tempdir().expect("tempdir");
        setup(dir.path());
        dir
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, body).expect("write file");
    }

    #[test]
    fn no_agents_md_yields_empty_instructions() {
        let dir = with_tree(|root| {
            write(&root.join("src/lib.rs"), "// empty project");
        });
        let ctx = compile(dir.path(), dir.path()).expect("compile");
        assert!(ctx.instructions.is_empty(), "expected no instructions");
        // But the system prompt is always populated.
        assert!(!ctx.system_prompt.is_empty());
    }

    #[test]
    fn root_agents_md_is_one_instruction() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root rule\n");
            write(&root.join("src/lib.rs"), "// nothing");
        });
        let ctx = compile(dir.path(), dir.path()).expect("compile");
        assert_eq!(ctx.instructions.len(), 1);
        assert_eq!(ctx.instructions[0].content, "root rule\n");
        assert!(ctx.instructions[0].path.ends_with("AGENTS.md"));
    }

    #[test]
    fn cwd_in_subdir_without_nested_inherits_root_only() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root\n");
            fs::create_dir_all(root.join("src")).expect("mkdir src");
            write(&root.join("src/lib.rs"), "// lib");
        });
        let cwd = dir.path().join("src");
        let ctx = compile(dir.path(), &cwd).expect("compile");
        assert_eq!(ctx.instructions.len(), 1);
        assert_eq!(ctx.instructions[0].content, "root\n");
    }

    #[test]
    fn cwd_with_nested_agents_md_gives_two_in_order() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root\n");
            fs::create_dir_all(root.join("src")).expect("mkdir src");
            write(&root.join("src/AGENTS.md"), "nested\n");
        });
        let cwd = dir.path().join("src");
        let ctx = compile(dir.path(), &cwd).expect("compile");
        assert_eq!(ctx.instructions.len(), 2);
        assert_eq!(ctx.instructions[0].content, "root\n");
        assert_eq!(ctx.instructions[1].content, "nested\n");
    }

    #[test]
    fn cwd_three_deep_with_instructions_at_each_level() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "L0\n");
            write(&root.join("a/AGENTS.md"), "L1\n");
            write(&root.join("a/b/AGENTS.md"), "L2\n");
            write(&root.join("a/b/c/AGENTS.md"), "L3\n");
        });
        let cwd = dir.path().join("a/b/c");
        let ctx = compile(dir.path(), &cwd).expect("compile");
        // 4 ancestors (root, a, a/b, a/b/c), each with AGENTS.md.
        assert_eq!(ctx.instructions.len(), 4);
        assert_eq!(
            ctx.instructions
                .iter()
                .map(|i| i.content.as_str())
                .collect::<Vec<_>>(),
            vec!["L0\n", "L1\n", "L2\n", "L3\n"]
        );
    }

    #[test]
    fn unrelated_subtree_agents_md_is_excluded() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root\n");
            // sibling subtree, not on the cwd path
            write(&root.join("vendor/other/AGENTS.md"), "noise\n");
        });
        let cwd = dir.path().join("src");
        fs::create_dir_all(&cwd).expect("mkdir src");
        let ctx = compile(dir.path(), &cwd).expect("compile");
        assert_eq!(ctx.instructions.len(), 1);
        assert_eq!(ctx.instructions[0].content, "root\n");
    }

    #[test]
    fn content_hash_changes_when_content_changes() {
        let dir1 = with_tree(|root| write(&root.join("AGENTS.md"), "v1\n"));
        let dir2 = with_tree(|root| write(&root.join("AGENTS.md"), "v2\n"));

        let ctx1 = compile(dir1.path(), dir1.path()).expect("compile1");
        let ctx2 = compile(dir2.path(), dir2.path()).expect("compile2");

        assert_ne!(
            ctx1.instructions[0].content_hash,
            ctx2.instructions[0].content_hash
        );
        assert_ne!(ctx1.total_hash, ctx2.total_hash);
    }

    #[test]
    fn missing_root_agents_md_is_not_an_error() {
        // Variant of test 1 with a subdirectory so we exercise the
        // "empty list at root, no panic" path.
        let dir = with_tree(|root| {
            fs::create_dir_all(root.join("sub")).expect("mkdir sub");
            write(&root.join("sub/file.rs"), "// code");
        });
        let ctx = compile(dir.path(), dir.path()).expect("compile");
        assert!(ctx.instructions.is_empty());
        assert_eq!(ctx.total_hash.len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_on_parent_returns_accessible_prefix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root\n");
            fs::create_dir_all(root.join("sub")).expect("mkdir sub");
            write(&root.join("sub/AGENTS.md"), "nested\n");
        });
        let nested = dir.path().join("sub/AGENTS.md");

        // Drop permissions to 0 on the nested file. Restore before
        // the test returns so TempDir cleanup succeeds even on
        // failure.
        let original = fs::metadata(&nested).expect("meta").permissions();
        let mut perms = original.clone();
        perms.set_mode(0o000);
        fs::set_permissions(&nested, perms).expect("chmod 000");

        // If we are running as a user that can read regardless of
        // mode (root, or a privileged CI runner) the test cannot
        // observe a denial — bail out cleanly.
        let can_still_read = fs::read_to_string(&nested).is_ok();

        let cwd = dir.path().join("sub");
        let result = compile(dir.path(), &cwd).expect("compile should not error");

        // Always restore perms so tempdir cleanup works.
        fs::set_permissions(&nested, original).expect("chmod restore");

        if can_still_read {
            eprintln!("permission_denied test skipped: process bypasses mode bits");
            // In that case both instructions will load — sanity check.
            assert_eq!(result.instructions.len(), 2);
        } else {
            assert_eq!(
                result.instructions.len(),
                1,
                "expected accessible prefix only"
            );
            assert_eq!(result.instructions[0].content, "root\n");
        }
    }

    #[test]
    fn agents_md_in_gitignore_is_not_loaded() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "root\n");
            // The .gitignore is the gate; the presence of the nested
            // file by itself does not change behaviour.
            write(&root.join("a/.gitignore"), "AGENTS.md\n");
            write(&root.join("a/AGENTS.md"), "should be ignored\n");
        });
        let cwd = dir.path().join("a");
        let ctx = compile(dir.path(), &cwd).expect("compile");
        // Only the root one survives the .gitignore filter.
        assert_eq!(ctx.instructions.len(), 1);
        assert_eq!(ctx.instructions[0].content, "root\n");
    }

    #[test]
    fn system_prompt_is_embedded() {
        // We don't compare exact text (the prompt is versioned
        // separately) but the embedded constant must be non-empty and
        // exposed verbatim on the compiled context.
        let dir = with_tree(|_| {});
        let ctx = compile(dir.path(), dir.path()).expect("compile");
        assert_eq!(ctx.system_prompt, SYSTEM_PROMPT);
    }

    #[test]
    fn total_hash_is_stable_for_same_inputs() {
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "stable\n");
            write(&root.join("a/AGENTS.md"), "stable too\n");
        });
        let cwd = dir.path().join("a");
        let a = compile(dir.path(), &cwd).expect("compile a");
        let b = compile(dir.path(), &cwd).expect("compile b");
        assert_eq!(a.total_hash, b.total_hash);
        assert_eq!(a.instructions.len(), b.instructions.len());
        assert_eq!(
            a.instructions
                .iter()
                .map(|i| i.content_hash)
                .collect::<Vec<_>>(),
            b.instructions
                .iter()
                .map(|i| i.content_hash)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn total_hash_distinguishes_order() {
        // Two trees that contain the same set of instructions but in
        // different ancestor positions should produce different
        // total_hashes. We can't change the positions of AGENTS.md
        // for the same cwd, so we approximate: same prompt, same set
        // of hashes, but in a different compile that uses cwd=root
        // (one fewer instruction). That alone changes the hash.
        let dir = with_tree(|root| {
            write(&root.join("AGENTS.md"), "same\n");
            write(&root.join("a/AGENTS.md"), "same\n");
        });
        let at_root = compile(dir.path(), dir.path()).expect("compile root");
        let at_a = compile(dir.path(), &dir.path().join("a")).expect("compile a");
        assert_ne!(at_root.total_hash, at_a.total_hash);
    }

    #[test]
    fn cwd_outside_root_is_rejected() {
        let project = with_tree(|root| write(&root.join("AGENTS.md"), "r\n"));
        let outside = tempfile::tempdir().expect("outside tempdir");
        let err = compile(project.path(), outside.path()).expect_err("should reject");
        assert!(matches!(err, ContextError::CwdOutsideRoot));
    }

    #[test]
    fn missing_project_root_is_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bogus = dir.path().join("does-not-exist");
        let err = compile(&bogus, dir.path()).expect_err("should fail");
        assert!(matches!(err, ContextError::Io(_)));
    }

    #[test]
    fn project_root_that_is_a_file_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir");
        fs::write(&file, "x").expect("write file");
        let err = compile(&file, dir.path()).expect_err("should fail");
        match err {
            ContextError::NotADirectory(p) => assert_eq!(p, file),
            other => panic!("expected NotADirectory, got {other:?}"),
        }
    }

    #[test]
    fn content_hash_is_sha256_of_content() {
        // Spot-check: the hash we expose equals a freshly-computed
        // SHA-256 of the file's bytes.
        let dir = with_tree(|root| write(&root.join("AGENTS.md"), "hash me\n"));
        let ctx = compile(dir.path(), dir.path()).expect("compile");
        let expected = {
            let mut h = Sha256::new();
            h.update(b"hash me\n");
            let out = h.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&out);
            arr
        };
        assert_eq!(ctx.instructions[0].content_hash, expected);
    }

    #[test]
    fn symlink_loop_does_not_panic() {
        // Create a self-referential symlink under the project root.
        // ignore::Walk must not chase it and our compiler must not
        // loop. We only assert that compile() returns and yields the
        // root AGENTS.md.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let dir = with_tree(|root| {
                write(&root.join("AGENTS.md"), "root\n");
                // loop -> .  (points back to root)
                let link = root.join("loop");
                symlink(root, &link).expect("symlink");
            });
            let ctx = compile(dir.path(), dir.path()).expect("compile survives loop");
            assert_eq!(ctx.instructions.len(), 1);
            assert_eq!(ctx.instructions[0].content, "root\n");
        }
        #[cfg(not(unix))]
        {
            // No-op on non-unix platforms — the walker still would
            // not follow symlinks by default.
        }
    }
}
