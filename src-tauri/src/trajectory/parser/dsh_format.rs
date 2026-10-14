// ── DSH session-format adapter ───────────────────────────────────────────
//
// Bridges DSH session-log generations to the shape the normalizer in `super`
// is written against. The normalizer stays version-agnostic; everything that
// depends on a format version lives here.
//
// Why this exists: the top-level `assistant/chunk` event was replaced by the
// batched `assistant/attempt` in **format v2**. v0 and v1 emit
// `assistant/chunk`; v2+ fold the same chunk objects into
// `assistant/attempt.data.stream[]`. Without this layer a v2+ log parses
// "successfully" while silently losing precise TTFT, durations and streamed
// token usage — no error, just wrong numbers.
//
// Verified against the official schemas (docs/persistence-changes/
// historical-formats/vN.schema.json): `assistant/chunk` is present in v0/v1 and
// absent from v2 onward; v2, v3 and v4 share an identical
// `assistant/attempt.data` shape `{step, stream, turn}`, and each `stream`
// entry with `type: "chunk"` carries a `chunk` object isomorphic to the v0
// `assistant/chunk.data.chunk`.

use serde_json::Value;

/// Baseline generation the normalizer in `super` is written against.
/// Logs at or below this version need no adaptation.
pub const BASELINE_VERSION: u64 = 1;

/// Highest generation this adapter knows how to read.
/// Anything above is reported rather than silently guessed at.
pub const MAX_KNOWN_VERSION: u64 = 4;

/// Outcome of inspecting a log's header version.
#[derive(Debug, PartialEq, Eq)]
pub enum VersionSupport {
    /// At or below the baseline: the normalizer reads this directly.
    Native,
    /// Newer, but a known generation this adapter understands.
    Lowered(u64),
    /// Newer than anything this build knows. Still parsed best-effort; the
    /// caller surfaces this rather than pretending the numbers are exact.
    Unknown(u64),
}

/// Read the format version from a parsed session header line.
///
/// A missing or non-numeric `version` is treated as the baseline: real v0
/// headers do carry `"version": 0`, but being lenient here keeps truncated or
/// hand-written fixtures working.
pub fn header_version(header: &Value) -> u64 {
    header["version"].as_u64().unwrap_or(BASELINE_VERSION)
}

/// Classify a header version against what this adapter can read.
pub fn classify(version: u64) -> VersionSupport {
    if version <= BASELINE_VERSION {
        VersionSupport::Native
    } else if version <= MAX_KNOWN_VERSION {
        VersionSupport::Lowered(version)
    } else {
        VersionSupport::Unknown(version)
    }
}

/// One streaming chunk recovered from a v2+ `assistant/attempt` event.
pub struct AttemptChunk<'a> {
    pub turn: Option<usize>,
    pub step: Option<usize>,
    /// The chunk's own timestamp, falling back to the attempt's.
    pub time: i64,
    pub chunk: &'a Value,
}

/// Recover the streaming chunks a v2+ `assistant/attempt` event batches.
///
/// Returns an empty vec for every other event type, so callers can treat this
/// as a pure "expand if applicable" step.
///
/// Only `stream` entries of `type: "chunk"` are returned — those are what the
/// baseline normalizer consumes. The `reasoning-chunks` / `text-chunks` /
/// `tool-call-chunks` carriers are ignored: the normalizer skips the
/// same-named top-level events too, so expanding them would add work without
/// changing the result.
pub fn attempt_chunks(line: &Value) -> Vec<AttemptChunk<'_>> {
    if line["type"].as_str() != Some("assistant/attempt") {
        return Vec::new();
    }

    let data = &line["data"];
    let Some(stream) = data["stream"].as_array() else {
        return Vec::new();
    };

    let turn = data["turn"].as_u64().map(|value| value as usize);
    let step = data["step"].as_u64().map(|value| value as usize);
    let attempt_time = line["time"].as_i64().unwrap_or(0);

    stream
        .iter()
        .filter(|entry| entry["type"].as_str() == Some("chunk") && entry["chunk"].is_object())
        .map(|entry| AttemptChunk {
            turn,
            step,
            time: entry["time"].as_i64().unwrap_or(attempt_time),
            chunk: &entry["chunk"],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn attempt_with(stream: Value) -> Value {
        json!({
            "type": "assistant/attempt",
            "seq": 7,
            "time": 1_787_023_345_000i64,
            "data": { "turn": 2, "step": 1, "stream": stream }
        })
    }

    #[test]
    fn version_below_baseline_is_native() {
        assert_eq!(classify(0), VersionSupport::Native);
        assert_eq!(classify(1), VersionSupport::Native);
    }

    #[test]
    fn known_newer_versions_are_read() {
        assert_eq!(classify(2), VersionSupport::Lowered(2));
        assert_eq!(classify(4), VersionSupport::Lowered(4));
    }

    #[test]
    fn future_version_is_flagged_unknown() {
        assert_eq!(classify(5), VersionSupport::Unknown(5));
        assert_eq!(classify(99), VersionSupport::Unknown(99));
    }

    #[test]
    fn header_version_defaults_to_baseline() {
        assert_eq!(header_version(&json!({ "version": 0 })), 0);
        assert_eq!(header_version(&json!({ "version": 4 })), 4);
        assert_eq!(
            header_version(&json!({ "type": "session" })),
            BASELINE_VERSION
        );
    }

    #[test]
    fn chunk_entries_are_recovered_with_turn_and_step() {
        let line = attempt_with(json!([
            { "type": "chunk", "time": 1_787_023_345_100i64,
              "chunk": { "type": "block-start", "index": 0, "blockType": "text" } },
            { "type": "chunk", "time": 1_787_023_345_200i64,
              "chunk": { "type": "text-delta", "index": 0, "text": "he" } },
            { "type": "chunk", "time": 1_787_023_345_300i64,
              "chunk": { "type": "usage", "usage": { "inputTokens": 100, "outputTokens": 25 } } }
        ]));

        let chunks = attempt_chunks(&line);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1].chunk["type"], "text-delta");
        assert_eq!(chunks[1].chunk["text"], "he");
        // turn/step are hoisted from the attempt so the normalizer's
        // (turn, step) keying keeps working.
        assert_eq!(chunks[0].turn, Some(2));
        assert_eq!(chunks[0].step, Some(1));
        // the per-chunk time wins over the attempt timestamp
        assert_eq!(chunks[1].time, 1_787_023_345_200i64);
    }

    #[test]
    fn non_chunk_stream_entries_are_ignored() {
        let line = attempt_with(json!([
            { "type": "reasoning-chunks", "dt": 5, "index": 0,
              "texts": ["a"], "time0": 1i64 },
            { "type": "text-chunks", "dt": 5, "index": 0,
              "texts": ["b"], "time0": 2i64 },
            { "type": "tool-call-chunks", "args": [], "dt": 5, "id": "x",
              "index": 0, "name": "bash", "time0": 3i64 }
        ]));
        assert!(attempt_chunks(&line).is_empty());
    }

    #[test]
    fn missing_chunk_time_falls_back_to_attempt_time() {
        let line = attempt_with(json!([
            { "type": "chunk", "chunk": { "type": "text-delta", "index": 0, "text": "x" } }
        ]));
        let chunks = attempt_chunks(&line);
        assert_eq!(chunks[0].time, 1_787_023_345_000i64);
    }

    #[test]
    fn malformed_entries_are_dropped_not_returned() {
        let line = attempt_with(json!([
            { "type": "chunk" },                                  // no chunk object
            { "type": "chunk", "chunk": "not-an-object" },
            { "type": "chunk", "chunk": { "type": "text-delta", "text": "ok" } }
        ]));
        let chunks = attempt_chunks(&line);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk["text"], "ok");
    }

    #[test]
    fn other_event_types_yield_nothing() {
        let user_msg = json!({ "type": "user/message", "seq": 1, "data": {} });
        assert!(attempt_chunks(&user_msg).is_empty());
    }

    #[test]
    fn attempt_without_stream_yields_nothing() {
        let line = json!({
            "type": "assistant/attempt", "seq": 1, "time": 0i64,
            "data": { "turn": 1, "step": 1 }
        });
        assert!(attempt_chunks(&line).is_empty());
    }
}
