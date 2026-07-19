//! Append-only JSONL session log writer.
//!
//! Spec §10 (durable entries) and §16 (crash recovery).
//!
//! Each [`SessionWriter::append`] serialises exactly one
//! [`SessionEntry`] as a single JSON line, writes it, and `fsync`s
//! before returning. There is no in-memory buffer, no background flush
//! thread, and no compaction — the file is the source of truth and
//! grows by one line per call.
//!
//! Crash semantics: a crash between the `write` and the `fsync` can
//! produce a truncated final line. [`read_entries_with_recovery`]
//! distinguishes a truncated tail (the only realistic crash case)
//! from corruption in the middle of the file, so the next process
//! can resume from the last good entry without manual repair.

use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::ids::{SessionId, Timestamp};
use crate::session_entry::SessionEntry;

/// Errors surfaced by the session-writer API.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Wraps a filesystem-level `std::io::Error`.
    #[error("session: I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialisation or deserialisation failed.
    #[error("session: JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Another writer already holds this session file open.
    ///
    /// Returned by [`SessionWriter::open`] when the per-path sibling
    /// `.lock` file already exists and its owner is still alive (or
    /// the lock was created very recently). Stale locks are
    /// auto-evicted by age.
    #[error("session: another writer already holds {path}")]
    Busy {
        /// Path that was already locked.
        path: PathBuf,
    },
    /// The lockfile is older than [`LOCK_MAX_AGE_SECS`]. Surfaced
    /// only by callers that want to distinguish staleness; the open
    /// path evicts such locks transparently.
    #[error("session: stale lockfile at {path}")]
    StaleLock {
        /// Lockfile path.
        path: PathBuf,
    },
}

/// How long a `.lock` file may live before we consider it stale and
/// evict it. Two minutes is well over any legitimate write's lifetime
/// (each `append` returns in milliseconds) but short enough that a
/// crashed process recovers quickly.
#[allow(dead_code)]
pub const LOCK_MAX_AGE_SECS: u64 = 120;

/// Append-only JSONL writer for a single session log.
///
/// Each [`append`](Self::append) writes one JSON object followed by a
/// newline, then fsyncs the file. There is no in-memory buffering, so
/// the writer's view of `seq()` always matches the number of durable
/// lines on disk.
#[derive(Debug)]
pub struct SessionWriter {
    file: File,
    path: PathBuf,
    seq: u64,
}

impl SessionWriter {
    /// Open (or create) the session log at `path`.
    ///
    /// On Unix the JSONL file is created with mode `0600`. A sibling
    /// `<path>.lock` file is created with `O_CREAT | O_EXCL`; if it
    /// already exists and is fresh (younger than
    /// [`LOCK_MAX_AGE_SECS`]), the call returns [`SessionError::Busy`].
    /// A stale lock is evicted transparently so the next writer can
    /// take over after a crash.
    ///
    /// If the JSONL file already exists, the writer recovers the
    /// trailing sequence number so subsequent `append`s continue
    /// rather than starting at 0 (which would cause
    /// [`crate::agent::Agent`] to write a duplicate
    /// `SessionStarted`).
    ///
    /// # Errors
    ///
    /// - [`SessionError::Busy`] if a fresh lockfile already exists.
    /// - [`SessionError::Io`] for any filesystem failure.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        let lock_path = lock_path_for(&path);

        // Take an exclusive per-path lock by creating the lockfile with
        // O_CREAT|O_EXCL. If one already exists, check its age — a
        // crash before `Drop` runs can leave a stale lock behind.
        let mut lock_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .await
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Try to evict a stale lockfile. If eviction fails or
                // the lock is fresh, treat as Busy.
                if try_evict_stale_lock(&lock_path).await? {
                    // Retry once after eviction.
                    match OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&lock_path)
                        .await
                    {
                        Ok(f) => f,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            return Err(SessionError::Busy { path });
                        }
                        Err(e) => return Err(SessionError::Io(e)),
                    }
                } else {
                    return Err(SessionError::Busy { path });
                }
            }
            Err(e) => return Err(SessionError::Io(e)),
        };
        // Best-effort marker write — failures here just mean the
        // lockfile remains empty, which is still a valid lock.
        let _ = lock_file.write_all(b"crow\n").await;

        // Open the real log with create+append (never truncate). Mode
        // 0600 is enforced on Unix; on other platforms the caller's
        // umask applies.
        let file = match open_log_file(&path).await {
            Ok(f) => f,
            Err(e) => {
                // Don't strand the lockfile if we couldn't open the log.
                let _ = tokio::fs::remove_file(&lock_path).await;
                return Err(SessionError::Io(e));
            }
        };

        // Recover the trailing sequence number so a reopen does NOT
        // append a duplicate SessionStarted. This must come BEFORE
        // any caller sees the writer.
        let seq = count_existing_entries(&path).await?;

        Ok(SessionWriter { file, path, seq })
    }

    /// Append `entry` as one JSON line and `fsync`.
    ///
    /// The on-disk shape is exactly one `<JSON object>\n`. Because the
    /// file is opened in append mode the write is atomic from the
    /// file's perspective (line-by-line, never interleaved with another
    /// writer — see [`open`](Self::open)).
    ///
    /// # Errors
    ///
    /// Returns the [`serde_json`] error from serialisation, or the I/O
    /// error from the `write` / `fsync`.
    pub async fn append(&mut self, entry: SessionEntry) -> Result<(), SessionError> {
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');

        self.file.write_all(&line).await?;
        self.file.sync_all().await?;

        self.seq += 1;
        Ok(())
    }

    /// Force any kernel buffers to disk and return.
    ///
    /// The JSONL writer has no internal buffer, so this is the same as
    /// `fsync` — it's exposed so the agent loop has a uniform
    /// `finish()` call across all durable sinks.
    ///
    /// # Errors
    ///
    /// Propagates the I/O error from `sync_all`.
    pub async fn finish(&mut self) -> Result<(), SessionError> {
        self.file.sync_all().await?;
        Ok(())
    }

    /// Path of the JSONL file this writer is writing to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of `append` calls that have succeeded since this writer
    /// was opened (or, after recovery, the trailing sequence of the
    /// existing log at open time).
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        // Best-effort lockfile cleanup. The writer has already
        // fsync'd every append, so a crash before this runs only
        // leaves a stale .lock file, which the next open() evicts
        // based on age.
        let lock = lock_path_for(&self.path);
        let _ = std::fs::remove_file(&lock);
    }
}

/// Open the JSONL log file in append mode with strict `0600`
/// permissions on Unix.
async fn open_log_file(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        // `tokio::fs::OpenOptions` exposes the same Unix extensions as
        // `std::fs::OpenOptions`, including `.mode()`. Setting 0600 here
        // means the JSONL log is owner-readable/writable only.
        OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .await
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(path)
            .await
    }
}

/// `path.lock` — used to detect concurrent writers.
fn lock_path_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

/// Count the parseable lines already on disk for `path`, returning
/// 0 for a missing or empty file. Used to recover the trailing
/// sequence number at writer open time.
async fn count_existing_entries(path: &Path) -> Result<u64, SessionError> {
    let bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(SessionError::Io(e)),
    };
    let mut count = 0u64;
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if serde_json::from_slice::<SessionEntry>(line).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

/// Evict a stale lockfile (one older than [`LOCK_MAX_AGE_SECS`]).
/// Returns `Ok(true)` if the lock was evicted, `Ok(false)` if it was
/// fresh or could not be evicted.
async fn try_evict_stale_lock(lock_path: &Path) -> Result<bool, SessionError> {
    let meta = match tokio::fs::metadata(lock_path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(SessionError::Io(e)),
    };
    let modified = match meta.modified() {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };
    let age = match std::time::SystemTime::now().duration_since(modified) {
        Ok(d) => d,
        Err(_) => return Ok(false),
    };
    if age.as_secs() >= LOCK_MAX_AGE_SECS {
        match tokio::fs::remove_file(lock_path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(SessionError::Io(e)),
        }
    } else {
        Ok(false)
    }
}

/// Outcome of [`read_entries_with_recovery`].
#[derive(Debug)]
pub struct RecoveryReport {
    /// Every parseable entry on disk (in order).
    pub entries: Vec<SessionEntry>,
    /// 1-based line numbers of lines that were unparseable AND
    /// appeared before the final truncated tail. These indicate real
    /// corruption and should be surfaced to the user.
    pub malformed_lines: Vec<usize>,
    /// `true` if the final fragment of the file failed to parse —
    /// i.e. the file ends with a truncated line (the classic
    /// crash-mid-append case). When `true`, `entries` excludes the
    /// truncated line but includes every earlier valid one.
    pub truncated_tail: bool,
}

/// Read every parseable line from the JSONL log at `path` and
/// distinguish a truncated tail from mid-file corruption. The legacy
/// [`read_entries`] is lossy; this is the recovery-aware variant the
/// agent loop should use when resuming.
pub async fn read_entries_with_recovery(
    path: impl AsRef<Path>,
) -> Result<RecoveryReport, SessionError> {
    let bytes = tokio::fs::read(path.as_ref()).await?;
    let mut entries = Vec::new();
    let mut malformed_lines = Vec::new();
    let mut line_no = 0usize;
    let mut truncated_tail = false;
    // A truncated tail is the file ending with content that has no
    // trailing newline. If the last byte is '\n', every segment is a
    // well-formed line; corruption there is real corruption. If the
    // last byte is NOT '\n', the final non-empty segment is the
    // truncated tail.
    let has_trailing_newline = bytes.last() == Some(&b'\n');
    for line in bytes.split(|b| *b == b'\n') {
        line_no += 1;
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<SessionEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // We don't know yet whether this is the tail. Mark
                // it malformed for now; if it turns out to be the
                // tail (last segment, no trailing newline), we'll
                // move it out of malformed_lines below.
                malformed_lines.push(line_no);
            }
        }
    }
    if !has_trailing_newline {
        // The final segment is the truncated tail. If it parsed
        // correctly it stays in `entries`; if not, remove its line
        // number from `malformed_lines` and set `truncated_tail`.
        if let Some(&last_line) = malformed_lines.last() {
            if last_line == line_no {
                malformed_lines.pop();
                truncated_tail = true;
            }
        } else {
            // Last segment was non-empty AND parseable but the file
            // didn't end in '\n'. That's a malformed file (a well-
            // formed log always ends with '\n'); surface as a
            // truncated tail so the caller knows the file is bad.
            truncated_tail = true;
        }
    }
    Ok(RecoveryReport {
        entries,
        malformed_lines,
        truncated_tail,
    })
}

/// Read every parseable line from the JSONL log at `path`.
///
/// Lines that fail to parse — typically a truncated trailing fragment
/// from a crash mid-write — are silently skipped. Empty lines (e.g.,
/// the final newline of a well-formed file) are skipped too.
///
/// This is the lossy legacy API; new code should prefer
/// [`read_entries_with_recovery`].
///
/// # Errors
///
/// Returns the I/O error from reading the file.
pub async fn read_entries(path: impl AsRef<Path>) -> Result<Vec<SessionEntry>, SessionError> {
    let bytes = tokio::fs::read(path.as_ref()).await?;
    let mut entries = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<SessionEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // Drop unparseable lines — spec §16.
            }
        }
    }
    Ok(entries)
}

/// Lightweight metadata extracted from the first line of a session log.
///
/// [`list_sessions`] only reads the `SessionStarted` line of each file
/// so scanning a directory of finished sessions stays cheap even when
/// individual logs grow large.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMeta {
    /// Unique identifier of the session.
    pub session_id: SessionId,
    /// Wall-clock time the session started.
    pub started_at: Timestamp,
    /// Schema version this log was written under.
    pub schema_version: u32,
    /// On-disk path to the JSONL log.
    pub path: PathBuf,
}

/// Enumerate every session log in `dir`, newest-first.
///
/// A "session log" is any regular file whose name ends in `.jsonl`.
/// Each file is opened and its first line is parsed as a
/// [`SessionEntry::SessionStarted`] — anything else is skipped. The
/// returned [`Vec`] is sorted by `started_at` descending, so callers
/// get "latest activity first" without further work.
///
/// # Errors
///
/// Returns the I/O error from `tokio::fs::read_dir`. Per-file read or
/// parse failures are non-fatal: the file is simply omitted from the
/// result.
pub async fn list_sessions(dir: impl AsRef<Path>) -> Result<Vec<SessionMeta>, SessionError> {
    let mut metas = Vec::new();
    let mut entries = tokio::fs::read_dir(dir.as_ref()).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(_) => continue,
        };
        let first_line = bytes.split(|b| *b == b'\n').next().unwrap_or(&[]);
        if first_line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<SessionEntry>(first_line) {
            Ok(SessionEntry::SessionStarted {
                session_id,
                started_at,
                schema_version,
                ..
            }) => metas.push(SessionMeta {
                session_id,
                started_at,
                schema_version,
                path,
            }),
            _ => {
                // Skip files whose first line isn't a SessionStarted.
            }
        }
    }
    // Newest-first ordering.
    metas.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(metas)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::event::{StopReason, Usage, SCHEMA_VERSION};
    use crate::ids::{new_id, MessageId, SessionId, Timestamp};

    use super::*;

    /// Build a [`Timestamp`] from a millisecond offset since the Unix
    /// epoch. Used to make ordering tests deterministic — `now()`
    /// would race the system clock.
    fn ts(millis: u64) -> Timestamp {
        Timestamp(UNIX_EPOCH + Duration::from_millis(millis))
    }

    /// A `SessionStarted` entry with a caller-supplied timestamp.
    fn started(t: Timestamp) -> SessionEntry {
        SessionEntry::SessionStarted {
            schema_version: SCHEMA_VERSION,
            session_id: SessionId(new_id()),
            started_at: t,
            cwd: PathBuf::from("/tmp"),
        }
    }

    /// A `UserMessage` entry stamped at `t`.
    fn user_msg_at(content: &str, t: Timestamp) -> SessionEntry {
        SessionEntry::UserMessage {
            id: MessageId(new_id()),
            content: content.into(),
            timestamp: t,
        }
    }

    /// A `UserMessage` entry stamped with `Timestamp::now()`.
    fn user_msg(content: &str) -> SessionEntry {
        user_msg_at(content, Timestamp::now())
    }

    /// An `AssistantMessage` with a single text part.
    fn assistant_msg(text: &str) -> SessionEntry {
        SessionEntry::AssistantMessage {
            id: MessageId(new_id()),
            parts: vec![crate::message::Part::Text { text: text.into() }],
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
            stop_reason: Some(StopReason::EndTurn),
            timestamp: Timestamp::now(),
        }
    }

    #[tokio::test]
    async fn round_trip_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let mut writer = SessionWriter::open(&path).await.expect("open");
        writer
            .append(started(ts(1_700_000_000_000)))
            .await
            .expect("a1");
        writer.append(user_msg("hello")).await.expect("a2");
        writer.append(assistant_msg("hi")).await.expect("a3");
        writer.finish().await.expect("finish");
        drop(writer);

        let entries = read_entries(&path).await.expect("read");
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0], SessionEntry::SessionStarted { .. }));
        assert!(matches!(
            entries[1],
            SessionEntry::UserMessage { ref content, .. } if content == "hello"
        ));
        assert!(matches!(
            entries[2],
            SessionEntry::AssistantMessage { ref parts, .. }
            if matches!(&parts[0], crate::message::Part::Text { text } if text == "hi")
        ));
    }

    #[tokio::test]
    async fn seq_is_monotonically_increasing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let mut writer = SessionWriter::open(&path).await.expect("open");
        assert_eq!(writer.seq(), 0, "fresh writer starts at seq 0");
        for i in 1..=5u64 {
            writer
                .append(user_msg(&format!("m{i}")))
                .await
                .expect("append");
            assert_eq!(writer.seq(), i, "seq must equal the number of appends");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let writer = SessionWriter::open(&path).await.expect("open");
        drop(writer);

        let meta = std::fs::metadata(&path).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[tokio::test]
    async fn crash_mid_write_truncates_at_byte_50() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let mut writer = SessionWriter::open(&path).await.expect("open");
        writer
            .append(started(ts(1_700_000_000_000)))
            .await
            .expect("a1");
        writer.append(user_msg("one")).await.expect("a2");
        writer.append(user_msg("two")).await.expect("a3");
        writer.append(user_msg("three")).await.expect("a4");
        drop(writer);

        // Simulate a crash that truncated the file at byte 50.
        let bytes = std::fs::read(&path).expect("read");
        assert!(
            bytes.len() > 50,
            "test fixture assumes the log exceeds 50 bytes, got {}",
            bytes.len()
        );
        std::fs::write(&path, &bytes[..50]).expect("truncate");

        // read_entries must skip the unparseable trailing fragment and
        // return the parseable prefix only. byte 50 may land inside any
        // line — including the first — so we cannot assume the
        // SessionStarted survives. The contract is simply: every
        // recovered entry parses, and we recover strictly fewer than the
        // four we wrote.
        let recovered = read_entries(&path).await.expect("read");
        assert!(
            recovered.len() < 4,
            "expected truncated read, got {} entries",
            recovered.len()
        );
        for entry in &recovered {
            let json = serde_json::to_string(entry).expect("re-serialise");
            assert!(json.starts_with('{'));
        }
    }

    #[tokio::test]
    async fn read_entries_skips_unparseable_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        // Hand-craft a file with two valid entries around garbage.
        let good_a = serde_json::to_string(&started(ts(1_700_000_000_000))).expect("ser");
        let good_b =
            serde_json::to_string(&user_msg_at("ping", ts(1_700_000_000_001))).expect("ser");
        let mut body = String::new();
        body.push_str(&good_a);
        body.push('\n');
        body.push_str("<<<<not json>>>>");
        body.push('\n');
        body.push_str(&good_b);
        body.push('\n');
        std::fs::write(&path, body).expect("write");

        let entries = read_entries(&path).await.expect("read");
        assert_eq!(entries.len(), 2, "garbage line should be dropped");
        assert!(matches!(entries[0], SessionEntry::SessionStarted { .. }));
        assert!(matches!(
            entries[1],
            SessionEntry::UserMessage { ref content, .. } if content == "ping"
        ));
    }

    #[tokio::test]
    async fn read_entries_handles_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        // Open and drop without appending — the file exists but is empty.
        let writer = SessionWriter::open(&path).await.expect("open");
        drop(writer);

        let entries = read_entries(&path).await.expect("read");
        assert!(entries.is_empty(), "empty log should yield zero entries");
    }

    #[tokio::test]
    async fn concurrent_open_on_same_path_fails_loudly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let first = SessionWriter::open(&path).await.expect("first open");
        // While `first` is still alive, a second open must fail with
        // SessionError::Busy rather than silently corrupting the log.
        match SessionWriter::open(&path).await {
            Err(SessionError::Busy { .. }) => {}
            other => panic!("expected SessionError::Busy, got {other:?}"),
        }
        drop(first);
        // And once the first writer is released, the path is openable
        // again.
        let _second = SessionWriter::open(&path)
            .await
            .expect("second open after release");
    }

    #[tokio::test]
    async fn open_resumes_existing_log_without_truncating() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");

        // First writer: two entries, then closed.
        {
            let mut writer = SessionWriter::open(&path).await.expect("open");
            writer
                .append(started(ts(1_700_000_000_000)))
                .await
                .expect("a1");
            writer.append(user_msg("first")).await.expect("a2");
        }
        let after_first = read_entries(&path).await.expect("read 1");
        assert_eq!(after_first.len(), 2);

        // Second writer: two more entries, then closed. None of the
        // first two should be lost.
        {
            let mut writer = SessionWriter::open(&path).await.expect("reopen");
            writer.append(user_msg("second")).await.expect("a3");
            writer.append(user_msg("third")).await.expect("a4");
        }
        let after_second = read_entries(&path).await.expect("read 2");
        assert_eq!(after_second.len(), 4);
        let contents: Vec<String> = after_second
            .iter()
            .filter_map(|e| match e {
                SessionEntry::UserMessage { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(contents, vec!["first", "second", "third"]);
    }

    #[tokio::test]
    async fn append_writes_exactly_one_line_with_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let mut writer = SessionWriter::open(&path).await.expect("open");
        writer.append(user_msg("only")).await.expect("append");
        drop(writer);

        let bytes = std::fs::read(&path).expect("read");
        let text = std::str::from_utf8(&bytes).expect("utf-8");
        assert_eq!(text.matches('\n').count(), 1, "exactly one newline");
        // The line, minus its newline, must be valid JSON.
        let trimmed = text.trim_end_matches('\n');
        let entry: SessionEntry = serde_json::from_str(trimmed).expect("valid JSON");
        assert!(matches!(
            entry,
            SessionEntry::UserMessage { ref content, .. } if content == "only"
        ));
    }

    #[tokio::test]
    async fn list_sessions_returns_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Three sessions with strictly increasing started_at values.
        let t_oldest = ts(1_700_000_000_000);
        let t_middle = ts(1_700_000_001_000);
        let t_newest = ts(1_700_000_002_000);

        for (name, t) in [
            ("a.jsonl", t_oldest),
            ("b.jsonl", t_middle),
            ("c.jsonl", t_newest),
        ] {
            let path = dir.path().join(name);
            let mut writer = SessionWriter::open(&path).await.expect("open");
            writer.append(started(t)).await.expect("append");
            drop(writer);
        }

        let metas = list_sessions(dir.path()).await.expect("list");
        assert_eq!(metas.len(), 3);
        assert!(metas[0].started_at >= metas[1].started_at);
        assert!(metas[1].started_at >= metas[2].started_at);
        // The newest entry is `c.jsonl` (largest started_at).
        assert!(
            metas[0].path.ends_with("c.jsonl"),
            "expected newest first, got {:?}",
            metas[0].path
        );
    }

    #[tokio::test]
    async fn list_sessions_skips_non_jsonl_and_garbage() {
        let dir = tempfile::tempdir().expect("tempdir");

        // A valid session log.
        let good = dir.path().join("good.jsonl");
        let mut writer = SessionWriter::open(&good).await.expect("open");
        writer
            .append(started(ts(1_700_000_000_000)))
            .await
            .expect("append");
        drop(writer);

        // A non-JSONL file that must be ignored.
        std::fs::write(dir.path().join("readme.txt"), "hello").expect("write");
        // A .jsonl with a garbage first line that must be ignored.
        std::fs::write(dir.path().join("junk.jsonl"), "not a session").expect("write");
        // A .jsonl that is empty — also ignored.
        std::fs::write(dir.path().join("empty.jsonl"), "").expect("write");

        let metas = list_sessions(dir.path()).await.expect("list");
        assert_eq!(metas.len(), 1, "only the well-formed session remains");
        assert_eq!(metas[0].path, good);
    }

    #[tokio::test]
    async fn list_sessions_returns_empty_for_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let metas = list_sessions(dir.path()).await.expect("list");
        assert!(metas.is_empty());
    }

    // ---- recovery-aware read tests (phase 3) ----

    #[tokio::test]
    async fn reopen_recovers_trailing_sequence_number() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        // Write three entries, drop writer, reopen, write two more.
        {
            let mut w = SessionWriter::open(&path).await.expect("open1");
            w.append(user_msg("a")).await.expect("a");
            w.append(user_msg("b")).await.expect("b");
            w.append(user_msg("c")).await.expect("c");
        }
        {
            let mut w = SessionWriter::open(&path).await.expect("reopen");
            assert_eq!(
                w.seq(),
                3,
                "reopen must inherit the trailing sequence, not reset to 0"
            );
            w.append(user_msg("d")).await.expect("d");
            w.append(user_msg("e")).await.expect("e");
            assert_eq!(w.seq(), 5);
        }
        let entries = read_entries(&path).await.expect("read");
        assert_eq!(entries.len(), 5);
    }

    #[tokio::test]
    async fn recovery_distinguishes_truncated_tail_from_corruption() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        // Build: valid + garbage mid-file + truncated-tail fragment.
        let good_a = serde_json::to_string(&started(ts(1_700_000_000_000))).expect("ser");
        let good_b =
            serde_json::to_string(&user_msg_at("ping", ts(1_700_000_000_001))).expect("ser");
        let mut body = String::new();
        body.push_str(&good_a);
        body.push('\n');
        body.push_str("<<<<corrupt mid-file>>>>");
        body.push('\n');
        body.push_str(&good_b);
        body.push('\n');
        // Truncated tail: a partial JSON line, no trailing newline.
        body.push_str("{\"type\":\"UserMessage\",\"id\":\"01H");
        std::fs::write(&path, body).expect("write");

        let report = read_entries_with_recovery(&path).await.expect("recovery");
        assert_eq!(report.entries.len(), 2, "two valid entries recovered");
        assert_eq!(report.malformed_lines.len(), 1, "one mid-file corruption");
        assert!(report.truncated_tail, "trailing fragment must be reported");
    }

    #[tokio::test]
    async fn recovery_no_truncation_on_clean_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        {
            let mut w = SessionWriter::open(&path).await.expect("open");
            w.append(user_msg("a")).await.expect("a");
            w.append(user_msg("b")).await.expect("b");
        }
        let report = read_entries_with_recovery(&path).await.expect("recovery");
        assert_eq!(report.entries.len(), 2);
        assert!(!report.truncated_tail);
        assert!(report.malformed_lines.is_empty());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn stale_lockfile_is_evicted_on_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        // Plant a stale lockfile by hand (mtime well over the
        // threshold). Without auto-eviction the next open would
        // return Busy forever.
        let lock_path = path.with_extension("jsonl.lock");
        std::fs::write(&lock_path, "crow\n").expect("write lock");
        // Backdate the mtime.
        let past =
            std::time::SystemTime::now() - std::time::Duration::from_secs(LOCK_MAX_AGE_SECS + 60);
        let _ = filetime::set_file_mtime(&lock_path, filetime::FileTime::from_system_time(past));

        let w = SessionWriter::open(&path).await.expect("open evicts stale");
        assert_eq!(w.seq(), 0);
        drop(w);
        assert!(!lock_path.exists(), "lockfile should be cleaned up");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn fresh_lockfile_returns_busy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.jsonl");
        let _first = SessionWriter::open(&path).await.expect("open");
        // Second open while the first is alive must return Busy.
        match SessionWriter::open(&path).await {
            Err(SessionError::Busy { .. }) => {}
            other => panic!("expected Busy, got {other:?}"),
        }
    }
}
