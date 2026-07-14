---
type: decision
status: accepted
date: 2026-07-14
---

# Decision 06 — patched the cwd_three_deep test

## Context

Task 2.5's brief specified "include cwd in the ancestor chain." The
test author wrote 12+ unit tests, but one of them (cwd_three_deep_with_
instructions_at_each_level) had a buggy expectation:

```rust
// Test creates AGENTS.md at L0 (root), L1 (a/), L2 (a/b/), L3 (a/b/c/
// cwd) and sets cwd = a/b/c. Per the brief "include cwd":
let ctx = compile(dir.path(), &cwd).expect("compile");
assert_eq!(ctx.instructions.len(), 3);  // <-- BUG: should be 4
assert_eq!(
    ctx.instructions.iter().map(|i| i.content.as_str()).collect::<Vec<_>>(),
    vec!["L0\n", "L1\n", "L2\n"]  // <-- BUG: missing L3
);
```

The test asserts only 3 entries and skips L3, contradicting the brief.

The other test that creates a 2-deep setup (cwd_with_nested_agents_md_gives_two_in_order)
expects 2 entries (root, cwd) — consistent with the brief.

## Decision

**Follow the brief, patch the test.** The brief says "broadest first,
most specific (cwd) last." The 3-deep test was wrong.

We patched the test to expect 4 entries and vec!["L0\n", "L1\n", "L2\n", "L3\n"].

A `// FIX:` comment was added to the patched test to make the change
auditable. See src/context.rs lines around `fn cwd_three_deep_with_
instructions_at_each_level`.

## Why not the other way

If we excluded cwd from the chain instead, the 2-deep test would fail
(because cwd is src in that test, and the test expects src's AGENTS.md).
So "include cwd" is consistent with one test and "exclude cwd" is
consistent with the other. The brief disambiguates: include cwd.
