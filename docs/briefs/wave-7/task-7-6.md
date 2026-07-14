### Task 7.6 — Onboarding flow

**Files:**
- New: `crates/crow-desktop/src/frontend/components/onboarding.ts`
- New: `crates/crow-desktop/src/frontend/components/welcome-step.ts`
- New: `crates/crow-desktop/src/frontend/components/signin-step.ts`
- New: `crates/crow-desktop/src/frontend/components/pick-project-step.ts`
- New: `crates/crow-desktop/src/frontend/components/hello-world-step.ts`

**Why this exists:** first-time users need to set up their API key and pick a project before they can use the app. The onboarding flow should be quick (target: <2 minutes) and informative.

**Spec references:** none — UX polish.

**Steps:**
1. **Welcome.** "Welcome to Crow. Crow is a small autonomous coding agent. Let's get you set up." Two buttons: "Get started" or "Skip — I'm using an existing install".
2. **Sign in.** The `Sign in` UI from 6.3. If the key is already in the keyring, skip this step.
3. **Pick a project.** Same as the project picker from 5.3.
4. **Hello world.** A scripted sample task: "Create a hello.md file in the project root with 'Hello, Crow!' inside." The user clicks "Run" → the agent does it → the user sees the result. This validates the full stack end-to-end.
5. **Done.** "You're all set. Click 'New chat' to start."

**State:**
- `Onboarding` state in the frontend's `localStorage`: `null` (not started) / `in_progress` / `complete`. If `null`, show the onboarding flow on first launch.
- Each step can be skipped individually.

**Procedure:**
1. Build the 5 step components.
2. Wire the flow: each step's "Next" button advances to the next step.
3. The hello-world step uses a `ScriptedProvider` (not real genai) so the onboarding works without an API key.
4. On completion, set `localStorage.onboarding = "complete"` and transition to the main app.

**Acceptance:**
- Manual test: clear localStorage → relaunch app → onboarding starts.
- Manual test: skip the hello-world step → main app shows.
- Manual test: do the full onboarding → main app shows with the new project selected.
- `cargo build --workspace` is clean.

**Forbidden:**
- No skipping the sign-in step if there's no API key (the user will see errors immediately).
- No scripted provider for non-onboarding sessions.

**Dependency:** none new.
