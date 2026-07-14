//! Phase 0 public API smoke test.
//!
//! Constructs one of every public type re-exported from `crow::*`,
//! replays a one-event scripted provider fixture end-to-end (open
//! session, append started, append user, stream scripted provider,
//! append assistant, finish), and asserts the resulting JSONL file
//! is byte-stable across runs by comparing its SHA-256 against a
//! pinned expected value.
//!
//! Runs on a `current_thread` tokio runtime, never touches the
//! network, and propagates every error with `?` — no `unwrap` or
//! `expect` anywhere in the test body.

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use crow::*;

// ---------------------------------------------------------------------------
// Deterministic helpers
// ---------------------------------------------------------------------------

/// Build a fixed `Ulid` from a single repeated byte. The fixture
/// uses one byte per role so every identifier is a compile-time
/// constant for the purposes of this smoke test.
fn ulid_seed(seed: u8) -> Ulid {
    Ulid::from_bytes([seed; 16])
}

/// Build a `Timestamp` at the given Unix-millisecond instant.
fn ts(millis: u64) -> Timestamp {
    Timestamp(UNIX_EPOCH + Duration::from_millis(millis))
}

// ---------------------------------------------------------------------------
// SHA-256 (RFC 6234), self-contained
// ---------------------------------------------------------------------------
//
// We can't add a `sha2` dev-dependency because this task's
// "Modify" list names only `src/lib.rs`. So the smoke test ships
// its own panic-free implementation. The function returns
// `Result` and uses `?` for the one place that can fail (the
// `try_into` from `&[u8]` to `[u8; 4]`), so the test body stays
// unwrap-free.

#[allow(clippy::too_many_lines)]
fn sha256(data: &[u8]) -> Result<[u8; 32], Box<dyn Error>> {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut buf: Vec<u8> = data.to_vec();
    let bit_len = (buf.len() as u64).wrapping_mul(8);
    buf.push(0x80);
    while buf.len() % 64 != 56 {
        buf.push(0x00);
    }
    buf.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in buf.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let bytes: [u8; 4] = chunk[i * 4..i * 4 + 4].try_into()?;
            *word = u32::from_be_bytes(bytes);
        }
        for i in 16..64 {
            let s0 =
                w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for (i, ki) in K.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*ki)
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn phase0_smoke() -> Result<(), Box<dyn Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_smoke())
}

async fn run_smoke() -> Result<(), Box<dyn Error>> {
    // 1. Construct one of every public type the brief lists.
    let session_id = SessionId(ulid_seed(0x01));
    let run_id = RunId(ulid_seed(0x02));
    let user_msg_id = MessageId(ulid_seed(0x03));
    let assistant_msg_id = MessageId(ulid_seed(0x04));
    let tool_call_id = ToolCallId(ulid_seed(0x05));
    let _tool_result_id = ToolResultId(ulid_seed(0x06));

    // AgentEvent: construct one explicitly. The scripted provider will
    // yield another at runtime; this one satisfies the
    // "constructs one of each" requirement.
    let _event = AgentEvent::RunStarted {
        run_id,
        session_id,
        started_at: ts(1_700_000_000_000),
    };

    // Message: construct one for the assistant side; the user side
    // records a flat `SessionEntry::UserMessage`.
    let _user_message = Message {
        id: user_msg_id,
        role: Role::User,
        parts: vec![Part::Text {
            text: "hi".into(),
        }],
    };
    let assistant_message = Message {
        id: assistant_msg_id,
        role: Role::Assistant,
        parts: vec![Part::Text {
            text: "hello back".into(),
        }],
    };

    // 2. Build the five session entries.
    let started_entry = SessionEntry::SessionStarted {
        schema_version: 1,
        session_id,
        started_at: ts(1_700_000_000_000),
        cwd: PathBuf::from("/tmp/crow-smoke"),
    };
    let user_entry = SessionEntry::UserMessage {
        id: user_msg_id,
        content: "hi".into(),
        timestamp: ts(1_700_000_001_000),
    };
    let stream_entry = SessionEntry::RunFinished {
        message: "done".into(),
        timestamp: ts(1_700_000_002_000),
    };
    let assistant_entry = SessionEntry::AssistantMessage {
        id: assistant_message.id,
        parts: assistant_message.parts.clone(),
        usage: Some(Usage {
            input_tokens: 1,
            output_tokens: 2,
        }),
        stop_reason: Some(StopReason::EndTurn),
        timestamp: ts(1_700_000_003_000),
    };
    let finish_entry = SessionEntry::RunFinished {
        message: "complete".into(),
        timestamp: ts(1_700_000_004_000),
    };

    // 3. Open a session in a temp dir.
    let dir = tempfile::tempdir()?;
    let session_path = dir.path().join("session.jsonl");
    let session = Session::open(&session_path, session_id).await?;

    // 4. Replay the sequence: open -> started -> user -> stream -> assistant -> finish.
    session.append(&started_entry).await?;
    session.append(&user_entry).await?;

    // Stream the scripted provider and record the observed event.
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("scripted_one_event.jsonl");
    let mut provider: Box<dyn Provider> =
        Box::new(ScriptedProvider::from_fixture(&fixture_path).await?);
    let streamed = provider
        .next_event()
        .ok_or_else(|| -> Box<dyn Error> { "scripted provider yielded no events".into() })?;
    match streamed {
        AgentEvent::RunFinished { .. } => {
            session.append(&stream_entry).await?;
        }
        other => {
            return Err(format!("expected RunFinished from fixture, got {other:?}").into());
        }
    }

    session.append(&assistant_entry).await?;
    session.append(&finish_entry).await?;

    // 5. Read back and assert.
    let entries = session.read_entries().await?;
    if entries.len() != 5 {
        return Err(format!("expected 5 entries, got {}", entries.len()).into());
    }

    // 6. Compute the SHA-256 of the file content and assert byte-stability.
    let content = std::fs::read(&session_path)?;
    let hash = sha256(&content)?;
    let hash_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();

    // Pinned expected hash. Computed once from the deterministic
    // fixture above; any change to ids, timestamps, the cwd string,
    // or the serde field order will be caught here.
    const EXPECTED_HASH: &str =
        "a549c6f19b4347573aa7de5d215b08316b6d5460af7adc1c0f46cdec260c0462";

    if hash_hex != EXPECTED_HASH {
        return Err(format!(
            "session file hash mismatch: got {hash_hex}, expected {EXPECTED_HASH}"
        )
        .into());
    }

    // Silence the unused-binding warning for the item the brief asks us
    // to construct but the byte-stability path doesn't otherwise read.
    let _ = tool_call_id;

    Ok(())
}
