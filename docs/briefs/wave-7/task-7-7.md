### Task 7.7 — End-to-end test suite

**Files:**
- New: `crates/crow-desktop/e2e/onboarding.spec.ts` (Playwright test)
- New: `crates/crow-desktop/e2e/chat.spec.ts` (Playwright test)
- New: `crates/crow-desktop/e2e/approval.spec.ts` (Playwright test)
- New: `crates/crow-desktop/e2e/plan-mode.spec.ts` (Playwright test)
- Modify: `crates/crow-desktop/package.json` (Playwright dependency)
- New: `crates/crow-desktop/.github/workflows/e2e.yml` (CI for E2E)

**Why this exists:** the desktop is a complex UI. Manual testing doesn't scale. E2E tests catch regressions in the full stack: Tauri IPC, frontend, backend kernel.

**Test cases (Playwright):**

1. **onboarding.spec.ts:** 4 cases
   - First launch shows the welcome step
   - Skip onboarding → main app
   - Complete onboarding with a sample project → main app with the project selected
   - Onboarding state persists across app restarts

2. **chat.spec.ts:** 6 cases
   - New chat button creates a new session
   - Submit a message → response streams in
   - Tool call appears as a card
   - Multi-turn conversation
   - Session list updates with the new session
   - Click a past session → loads the chat

3. **approval.spec.ts:** 5 cases
   - Bash tool call shows an approval card
   - Click "Allow once" → tool runs
   - Click "Deny" → tool returns denied result
   - Click "Allow for session" → same tool auto-allowed
   - 60s timeout → auto-deny

4. **plan-mode.spec.ts:** 3 cases
   - Switch to Plan mode → model responds without mutations
   - Click "Apply plan" → model runs in Build mode
   - Plan mode toggle persists

**Total: 18 E2E tests.**

**Procedure:**
1. Add `playwright` to `crates/crow-desktop/package.json` devDependencies.
2. Write the 4 spec files.
3. Configure Playwright to launch the Tauri app (use `tauri-driver` or run the app in dev mode with `--no-sandbox`).
4. Add a CI workflow that runs E2E on PR.
5. Record a demo video (optional, for the launch).

**Acceptance:**
- All 18 tests pass.
- E2E runs in <5 minutes total.
- The CI workflow is green.
- A demo video is recorded (optional).

**Forbidden:**
- No E2E tests that depend on the network (the scripted provider handles this).
- No flaky tests (use Playwright's `waitFor*` helpers, not `sleep`).

**Dependency:** `playwright`, `@playwright/test`. Optionally `@tauri-apps/e2e` for Tauri integration.
