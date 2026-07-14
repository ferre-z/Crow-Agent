### Task 2.5 — AGENTS.md discovery + context compiler

`src/context.rs`:
- `pub struct CompiledContext { system_prompt: String, instructions: Vec<(PathBuf, String, [u8; 32])>, total_hash: [u8; 32] }`
- `pub fn compile(project_root: &Path, cwd: &Path) -> Result<CompiledContext, ContextError>`
- Walks `project_root → cwd`, reads each `AGENTS.md`, records path + content + SHA-256
- Reads `system_prompt.md` (versioned in repo) for the system prompt
- `walk` uses `ignore::Walk` to respect `.gitignore`

**Spec:** §12.
**Acceptance:** 10+ tests — nested 3-deep resolution, AGENTS.md in unrelated subdir ignored, hash changes when content changes, missing root file is not an error (just empty list), permissions denied on a parent → returns the accessible prefix + warning entry.
