# Phase 2 (Desktop App) — Post-Mortem and Handover

> This is the post-mortem + handover for **Phase 2** of Crow. Phase 1 (kernel, waves 1-3) is done; phase 2 (desktop app) is the next 4 waves. Written before any code lands, so it captures the *plan* and the *expected lessons* — not the actual ones.

## What's in this folder

- `00-master-plan-phase-2.md` — the master plan. Read this first.
- `waves/` — the wave briefs for desktop waves 4, 5, 6, 7.
- `tasks/` — the per-task briefs (24 tasks total across 4 waves).
- `decisions/` — new decision logs as we go.

## Phase 2 wave map (refreshed)

| Wave | Theme | Tasks | Round strategy | Estimated turns |
|---|---|---|---|---|
| 4 | App-server + approvals | 4.1–4.7 | mostly sequential; 4.1 → 4.2 → 4.3, then 4.4 + 4.5 parallel, 4.6, then 4.7 | ~120K tokens, 4-6h |
| 5 | Tauri shell + web frontend | 5.1–5.8 | mostly sequential; 5.1 → 5.2 → 5.3 → 5.4 → 5.5 → 5.6 → 5.7 → 5.8 (some can be parallel) | ~300K tokens, 8-12h |
| 6 | Approvals UX + keyring + images | 6.1–6.5 | sequential; 6.1 → 6.2 → 6.3 → 6.4 → 6.5 | ~150K tokens, 5-8h |
| 7 | Plan mode + activity pane + notifications | 7.1–7.7 | sequential; 7.1 → 7.2 → 7.3 → 7.4 → 7.5 → 7.6 → 7.7 | ~200K tokens, 6-10h |

## Routing

- MiniMax M3 = default for code (Ferre budget ~100M tok/day)
- Nemotron Ultra = research + small features (Tauri examples, JSON-RPC libs, etc.)
- GLM-5.2 = debugging only

## Process decisions (carried from phase 1 post-mortem)

- Use `claude --dangerously-skip-permissions -p` via `terminal(background=true, notify_on_complete=true)`. `delegate_task` is broken.
- Use `git worktree add` per task, branch from main AFTER previous task's merge. Mostly sequential.
- max-turns 30 is the right budget; 5-turn finishers often hang. Finish manually.
- Each task gets a reviewer dispatch after the implementer (2 reviewers: spec + quality).
- Don't trust the implementer's "all green" — re-run the gate yourself.

## New process decisions for phase 2

- **Tauri tests run via `tauri test`** not `cargo test`. The Tauri runtime requires it.
- **Frontend tests run via `vitest`** (or just `node --test` for plain TS). Don't add a heavyweight test runner.
- **Visual regression tests** for the desktop frontend (Playwright) are wave 7 — we don't need them in waves 5-6 since the UI is changing fast.
- **Cross-platform packaging** (mac .app, win .msi, linux .deb) is wave 5 task 5.8. We can defer Linux support if it adds too much complexity; mac and Windows are the priority.

## Handover to next session

When Ferre returns to test:
1. Run the wave 1+2 (kernel) test suite to confirm the base still works.
2. Read the wave 4 master plan (`docs/waves/00-master-plan-phase-2.md`).
3. Read the wave 4 task briefs (`docs/briefs/wave-4/`).
4. Dispatch wave 4 round A (task 4.1 — app-server skeleton). The app-server is the foundation; everything else builds on it.
5. If 4.1 lands clean, dispatch round B (4.2 — request handlers). Wave 4's other tasks are mostly sequential.

## Open questions for Ferre

1. **JSON-RPC library choice.** Task 4.1 says "hand-rolled or jsonrpsee". `jsonrpsee` is the standard but adds 200KB to the binary. Hand-rolled is ~200 lines. **Recommendation: hand-rolled for the request/response core; only use jsonrpsee if we add a websocket transport later.**
2. **Tauri 2.x stable.** As of January 2026, Tauri 2 is stable. Use `tauri = "2"`.
3. **Frontend framework.** Codex uses a custom-element approach. We can do the same with vanilla TS + a small base class. **Recommendation: vanilla TS + a 50-line `Component` base class.** Skip React/Preact/Svelte.
4. **Default approval policy for `crow serve` in v0.** Three options: (a) `NoOp` (always allow, matches the kernel), (b) `Ask` (prompt for every tool, requires a TTY), (c) configurable per-session via `SessionStart`. **Recommendation: (a) by default, (c) opt-in via `SessionStart.policy: "ask"`.** The desktop overrides per-session.
5. **Voice input.** Defer to v1 unless Ferre specifically wants it. Whisper dependency is non-trivial.
6. **Notifications.** Implement with Tauri's `tauri-plugin-notification`. Cross-platform.
7. **Image attachments.** Implement with the existing `Part::Image` variant added to `message.rs`. The genai adapter (wave 2 task 2.2) handles the OpenAI-compatible `image_url` field.

## Lessons from phase 1 (full list in `docs/wave-1-postmortem.md`)

The headline:
- `delegate_task` is broken; use `claude -p` via terminal.
- max-turns 30 is the right budget; finishers often hang.
- Each implementer needs the post-merge API of its dependencies inlined in the brief.
- Run the quality gate yourself after every implementer.
- Reviewers (spec + quality) catch issues the implementer misses.
- Worktrees branch from main AFTER previous task's merge, not in parallel.
