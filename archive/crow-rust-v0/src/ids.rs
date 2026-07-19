//! Identifier and timestamp newtypes used throughout the agent.
//!
//! Every persistent object — sessions, runs, messages, tool calls —
//! carries one of these ULID-backed ids. ULIDs are 128-bit, k-sortable
//! and encode a millisecond timestamp, which gives us a total order
//! without coordination.
//!
//! [`Timestamp`] is the project-owned wall-clock newtype. It wraps
//! [`std::time::SystemTime`] and serialises to Unix milliseconds in JSON,
//! avoiding a `chrono` dependency.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Re-export of the [`ulid::Ulid`] type so callers can construct one
/// directly when they need to seed a deterministic value (e.g. in tests).
pub use ulid::Ulid;

/// Generate a fresh identifier.
///
/// Wraps [`Ulid::new`] so the rest of the crate does not depend on the
/// `ulid` crate directly.
#[must_use]
pub fn new_id() -> Ulid {
    Ulid::new()
}

/// Identifier for a top-level conversation (a "session" in the v0 spec).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub Ulid);

/// Identifier for a single agent run within a session.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId(pub Ulid);

/// Identifier for a single message (user, assistant, or tool result) in a session.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub Ulid);

/// Identifier for a tool invocation requested by the model.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub Ulid);

/// Identifier for a [`Part::ToolResult`] matching a [`ToolCallId`].
///
/// Kept distinct from [`ToolCallId`] so that the result side can grow
/// its own metadata without forcing a schema migration on the call side.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolResultId(pub Ulid);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session:{}", self.0)
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run:{}", self.0)
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "msg:{}", self.0)
    }
}

impl std::fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool_call:{}", self.0)
    }
}

impl std::fmt::Display for ToolResultId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool_result:{}", self.0)
    }
}

/// Project-owned timestamp.
///
/// Stored as a [`SystemTime`] but serialized as Unix milliseconds (a JSON
/// number) so we don't need to pull in `chrono`. Round-trips to within
/// millisecond precision.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(#[serde(with = "timestamp_serde")] pub SystemTime);

impl Timestamp {
    /// Capture the current wall-clock time, truncated to millisecond
    /// precision so that the JSON round-trip is lossless.
    #[must_use]
    pub fn now() -> Self {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        // Truncate to ms.
        let truncated = UNIX_EPOCH + Duration::from_millis(since_epoch.as_millis() as u64);
        Self(truncated)
    }
}

/// `serde` adapter that maps [`SystemTime`] to a Unix-millisecond `u64`.
mod timestamp_serde {
    use super::{Deserialize, Deserializer, Duration, Serializer, SystemTime, UNIX_EPOCH};

    /// Serialise a [`SystemTime`] as Unix milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if `time` predates the Unix epoch, which we do
    /// not support.
    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        let millis = time
            .duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?
            .as_millis();
        // Truncate to `u64`; this loses precision beyond ~584 million
        // years from epoch, which is acceptable.
        #[allow(clippy::cast_possible_truncation)]
        let millis_u64 = millis as u64;
        serializer.serialize_u64(millis_u64)
    }

    /// Deserialise a Unix-millisecond `u64` into a [`SystemTime`].
    ///
    /// # Errors
    ///
    /// Propagates any deserialiser error from reading the integer.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b, "two back-to-back ids must not collide");
    }

    #[test]
    fn session_id_serialises_round_trip() {
        let id = SessionId(new_id());
        let json = serde_json::to_string(&id).expect("serialize");
        let back: SessionId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }

    #[test]
    fn timestamp_now_round_trips_within_ten_ms() {
        // `Timestamp::now()` is truncated to ms, so the round-trip is exact.
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).expect("serialize");
        let back: Timestamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            ts, back,
            "round-trip must be lossless when the source is already truncated to ms"
        );
    }

    #[test]
    fn timestamp_from_arbitrary_systemtime_truncates_to_ms() {
        // A `SystemTime` with sub-ms precision must round-trip to the
        // truncated value, not the original.
        let precise = UNIX_EPOCH
            + std::time::Duration::from_secs(1_700_000)
            + std::time::Duration::from_nanos(123_456);
        let ts = Timestamp(precise);
        let json = serde_json::to_string(&ts).expect("serialize");
        let back: Timestamp = serde_json::from_str(&json).expect("deserialize");
        // Truncated to whole ms.
        let expected = UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000);
        assert_eq!(back.0, expected);
    }
}
