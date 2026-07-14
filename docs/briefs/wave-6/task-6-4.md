### Task 6.4 — Image attachments

**Files:**
- Modify: `src/message.rs` (add `Part::Image` variant)
- Modify: `src/provider/genai.rs` (map to OpenAI's `image_url`)
- Modify: `crates/crow-desktop/src/frontend/components/composer.ts` (image picker)
- New: `crates/crow-desktop/src/frontend/components/image-thumb.ts`
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri command: `attach_image`)

**Why this exists:** "look at this screenshot" is a common agent task. The model can describe an image, identify a UI element, suggest a fix, etc. The desktop needs to send images alongside text.

**Spec references:** spec §3.2 (no images in v0; this is a v0.1 extension). Spec §19 (deferred extension seams — provider extensions, here `genai` adds image support).

**Behavior:**
- Composer has a `+` button next to the textarea. Click → OS file picker, filter to PNG/JPEG/GIF, max 5MB.
- Selected images are thumbnailed inline in the composer. Each has a small `x` to remove.
- On submit, the user message contains a `User` role with multiple `Part::Image { data: Vec<u8>, mime: String, name: Option<String> }` plus an optional `Part::Text`.
- The genai adapter maps `Part::Image` to OpenAI's `image_url` field. Base64-encodes the data.

**Interfaces (exact):**

```rust
// src/message.rs addition
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "PascalCase")]
pub enum Part {
    // ... existing
    Image { data: Vec<u8>, mime: String, name: Option<String> },
}
```

```rust
// In src/lib.rs
#[tauri::command]
async fn attach_image(path: PathBuf) -> Result<ImageAttachment, String>;

pub struct ImageAttachment {
    pub data: Vec<u8>,
    pub mime: String,
    pub name: Option<String>,
}
```

**Procedure:**
1. Add `Part::Image` to `message.rs`. Update tests.
2. Modify the genai adapter to convert `Part::Image` to OpenAI's `image_url`. This may require extending the genai `ContentPart` enum (or using a raw JSON approach).
3. Add the image picker to the composer (Tauri command + frontend component).
4. Test: send a message with an image → the genai adapter sends the correct request.
5. Manual test: drag a PNG, send "what's in this image?" → the model responds.

**Acceptance:**
- A unit test in `message.rs` round-trips a `Part::Image`.
- A unit test in `provider/genai.rs` maps `Part::Image` to the right JSON shape.
- Manual test: image attachment works end-to-end.
- `cargo build --workspace` is clean.

**Forbidden:**
- No support for SVG, PDF, or other formats in v0.
- No image compression (the user is responsible for keeping images under 5MB).
- No storing images in the session log (they're transient).

**Dependency:** none new. (genai may need a small extension; if so, it's a feature flag.)
