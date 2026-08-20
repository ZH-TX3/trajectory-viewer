// ── Shared Utility Functions ──────────────────────────────────────────────

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, FixedOffset};

/// Read the first `head_n` lines and last `tail_n` lines from a file.
/// For small files (< 16 KB), reads all lines once to avoid unnecessary seeking.
pub fn read_head_tail_lines(
    path: &Path,
    head_n: usize,
    tail_n: usize,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();

    // For small files, read all lines once and split
    if file_len < 16_384 {
        let reader = BufReader::new(file);
        let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let head = all.iter().take(head_n).cloned().collect();
        let skip = all.len().saturating_sub(tail_n);
        let tail = all.into_iter().skip(skip).collect();
        return Ok((head, tail));
    }

    // Read head lines from the beginning
    let reader = BufReader::new(file);
    let head: Vec<String> = reader.lines().take(head_n).map_while(Result::ok).collect();

    // Seek to last ~16 KB for tail lines
    let seek_pos = file_len.saturating_sub(16_384);
    let mut file2 = File::open(path)?;
    file2.seek(SeekFrom::Start(seek_pos))?;
    let tail_reader = BufReader::new(file2);
    let all_tail: Vec<String> = tail_reader.lines().map_while(Result::ok).collect();

    // Skip first partial line if we seeked into the middle of a line
    let skip_first = if seek_pos > 0 { 1 } else { 0 };
    let usable: Vec<String> = all_tail.into_iter().skip(skip_first).collect();
    let skip = usable.len().saturating_sub(tail_n);
    let tail = usable.into_iter().skip(skip).collect();

    Ok((head, tail))
}

/// Parse a timestamp value to milliseconds since epoch.
/// Supports integers (seconds or milliseconds) and RFC3339 strings.
pub fn parse_timestamp_to_ms(value: &serde_json::Value) -> Option<i64> {
    // Integer: milliseconds (>1e12) or seconds
    if let Some(n) = value.as_i64() {
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    if let Some(n) = value.as_f64() {
        let n = n as i64;
        return Some(if n > 1_000_000_000_000 { n } else { n * 1000 });
    }
    // RFC3339 string
    let raw = value.as_str()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt: DateTime<FixedOffset>| dt.timestamp_millis())
}

/// Estimate per-event duration from the following event timestamp and
/// approximate TTFT for assistant messages.
///
/// Providers that already record precise values (e.g. DSH from streaming
/// chunk timestamps) keep them; only events without a value get the estimate.
pub fn finalize_trajectory_timings(events: &mut [crate::trajectory::TrajectoryEvent]) {
    for i in 0..events.len() {
        let ts = events[i].ts;

        if events[i].duration_ms.is_none() {
            let next_ts = events.get(i + 1).map(|event| event.ts).unwrap_or(ts);
            if next_ts > ts {
                events[i].duration_ms = Some(next_ts - ts);
            }
        }

        if events[i].event_type == "assistant-message" && events[i].ttft_ms.is_none() {
            if let Some(duration_ms) = events[i].duration_ms {
                // Rough estimate: TTFT ≈ one-third of total duration, capped at 3s
                events[i].ttft_ms = Some((duration_ms / 3).min(3000));
            }
        }
    }
}
