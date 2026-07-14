### Task 2.5 — AGENTS.md discovery + context compiler

**Files:**
- Create: `src/context.rs`
- Modify: `src/lib.rs` (`pub mod context;`)
- Create: `prompts/system_prompt.md` (the versioned system prompt)

**Spec references:** v0 spec §12 (context compiler), §18 (acceptance criterion 8: "Nested AGENTS.md instructions appear in the correct order in a captured mock-provider request").

**Why this exists:** the agent needs a way to discover repository-level instructions without the model having to know the layout. AGENTS.md is the de-facto standard; the compiler walks the root→cwd path and concatenates every AGENTS.md it finds, broadest first.

**Interfaces (exact):**

```rust
// src/context.rs
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};  // add to Cargo.toml

#[derive(Debug, Clone)]
pub struct CompiledContext {
    pub system_prompt: String,
    /// Broadest (root) first, most specific (cwd) last.
    pub instructions: Vec<InstructionFile>,
    pub total_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct InstructionFile {
    pub path: PathBuf,
    pub content: String,
    pub content_hash: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("project root is not a directory: {0}")] NotADirectory(PathBuf),
    #[error("cwd is outside project root")] CwdOutsideRoot,
    #[error("read failed: {0}")] Io(#[from] std::io::Error),
}

pub fn compile(project_root: &Path, cwd: &Path) -> Result<CompiledContext, ContextError> {
    // 1. Canonicalize both project_root and cwd.
    // 2. Compute the relative path from project_root to cwd.
    // 3. For each ancestor of cwd (including cwd itself and project_root),
    //    look for AGENTS.md. Read it if present.
    // 4. Build the system prompt by reading prompts/system_prompt.md at compile time
    //    (use include_str!).
    // 5. Compute total_hash = SHA-256(system_prompt || joined instruction hashes).
    todo!()
}

/// Read all instructions, broadest first.
fn discover_instructions(
    project_root: &Path,
    cwd: &Path,
) -> Result<Vec<InstructionFile>, ContextError> {
    todo!()
}
```

**Acceptance:**
- 10+ unit tests:
  1. project root with no AGENTS.md → empty instructions
  2. project root with AGENTS.md → 1 instruction
  3. cwd in a subdirectory of root with no nested AGENTS.md → 1 instruction (from root)
  4. cwd with a nested AGENTS.md → 2 instructions (root first, then nested)
  5. cwd 3-deep with AGENTS.md at every level → 3 instructions
  6. AGENTS.md in an unrelated subtree (not on the path) is NOT included
  7. content_hash changes when AGENTS.md content changes
  8. missing root AGENTS.md is not an error (just empty list at root)
  9. permissions denied on a parent → return accessible prefix + warning (not error)
  10. AGENTS.md in `.gitignore` is NOT loaded (use `ignore::Walk`)
  11. system_prompt is loaded from the embedded file
  12. total_hash is stable for the same inputs
- Gate: clean.

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code.
- No recursive symlink loops (use `ignore::Walk` which handles this).
- No silent failures: every skipped file should be logged via `tracing::warn!`.

**Dependencies:** add `sha2 = "0.10"` to `[dependencies]`.
