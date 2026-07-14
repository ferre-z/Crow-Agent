### Task 6.5 — Voice input (optional, defer if too expensive)

**Files:**
- New: `crates/crow-desktop/src/frontend/components/voice-button.ts`
- Modify: `crates/crow-desktop/src/frontend/components/composer.ts`
- New: `src/transcribe.rs` (Whisper API client — optional, can be deferred)

**Why this exists:** talking is faster than typing. The Codex desktop supports voice input. We'd take the same pattern.

**Status:** if Whisper dependency is too heavy or too expensive, defer this task to a future phase. The shell of the feature is small; the audio model integration is the work.

**If implementing:**
- Composer has a microphone button. Click to start recording (uses `MediaRecorder` API in the webview).
- Click again to stop. Send the audio blob to the Tauri backend.
- Backend POSTs to a Whisper endpoint (or runs whisper.cpp locally). Returns the transcript.
- Transcript populates the composer (as if typed).
- The user can edit the transcript before sending.

**Interfaces (exact):**

```rust
// src/transcribe.rs
pub async fn transcribe(audio: Vec<u8>, mime: String) -> Result<String, TranscribeError>;
```

**If deferring:**
- Document the deferral in a decision log.
- Skip the voice-button UI for v0.
- Keep the API surface so it's easy to add later.

**Acceptance (if implemented):**
- Manual test: click mic → recording starts → speak → click mic again → transcript appears in composer.
- `cargo build --workspace` is clean.

**Forbidden:**
- No recording without a clear visual indicator (the button must show "recording" state).
- No sending audio to a server without user consent (a one-time opt-in dialog).
- No storing audio (transcript only, audio is discarded).

**Dependency:** if local Whisper, `whisper-rs` or `whisper-cpp-rs`. If API, just `reqwest` (already in via genai's transitive deps).
