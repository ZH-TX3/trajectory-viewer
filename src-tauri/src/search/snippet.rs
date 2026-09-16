// ── Snippet extraction ───────────────────────────────────────────────────
//
// FTS5's own `snippet()` operates on the tokenized body, so Chinese results
// would come back as `修 复 轨 迹` — unreadable. Instead we locate the match
// in the ORIGINAL text and cut a window around it, returning the offset so
// the frontend can highlight precisely.

/// Characters of context to keep on each side of the match.
const CONTEXT: usize = 60;

/// Locate `needle` (case-insensitively) in `haystack`.
///
/// Returns a byte range. Comparison is done on lowercased copies, but the
/// returned offsets index the ORIGINAL string, so callers can slice it.
fn find_ci(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let hay_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let byte_pos = hay_lower.find(&needle_lower)?;

    // Map the lowercased byte offset back onto the original. `to_lowercase`
    // can change byte lengths (e.g. 'İ'), so walk char-by-char to stay exact.
    let prefix_chars = hay_lower[..byte_pos].chars().count();
    let needle_chars = needle_lower.chars().count();

    let start_byte = haystack
        .char_indices()
        .nth(prefix_chars)
        .map(|(i, _)| i)
        .unwrap_or(haystack.len());
    let end_byte = haystack
        .char_indices()
        .nth(prefix_chars + needle_chars)
        .map(|(i, _)| i)
        .unwrap_or(haystack.len());

    Some((start_byte, end_byte))
}

/// Build a display snippet around the first matching query term.
///
/// Falls back to the title when the body has no literal match (e.g. the hit
/// came from the title). Returns `(snippet, match_start, match_len)` where
/// the offsets are in CHARACTERS within `snippet`, or `None` when the match
/// could not be located in the body.
pub fn build(body: &str, title: &str, query: &str) -> (String, Option<usize>, usize) {
    // Try each query term in turn; the first one found in the body wins.
    for term in query.split_whitespace() {
        let Some((start_byte, end_byte)) = find_ci(body, term) else {
            continue;
        };

        let match_start_chars = body[..start_byte].chars().count();
        let match_len = body[start_byte..end_byte].chars().count();
        let total_chars = body.chars().count();

        let win_start = match_start_chars.saturating_sub(CONTEXT);
        let win_end = (match_start_chars + match_len + CONTEXT).min(total_chars);

        let mut snippet: String = body
            .chars()
            .skip(win_start)
            .take(win_end - win_start)
            .collect();

        if win_start > 0 {
            snippet.insert(0, '…');
        }
        if win_end < total_chars {
            snippet.push('…');
        }

        // Shift the match offset past any leading ellipsis we added.
        let lead = if win_start > 0 { 1 } else { 0 };
        return (
            snippet,
            Some(match_start_chars - win_start + lead),
            match_len,
        );
    }

    // No literal body match — surface the title so the row is still readable.
    let snippet = if title.is_empty() {
        body.chars().take(CONTEXT * 2).collect()
    } else {
        title.to_string()
    };
    (snippet, None, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_chinese_match_and_reports_char_offsets() {
        let body = "这里讨论修复轨迹查看器的白屏问题以及后续优化";
        let (snippet, start, len) = build(body, "标题", "白屏");

        let start = start.expect("match should be located");
        let matched: String = snippet.chars().skip(start).take(len).collect();
        assert_eq!(matched, "白屏");
    }

    #[test]
    fn finds_an_english_match_case_insensitively() {
        let body = "We need to Fix The White Screen bug";
        let (snippet, start, len) = build(body, "", "white");
        let start = start.unwrap();
        let matched: String = snippet.chars().skip(start).take(len).collect();
        assert_eq!(matched.to_lowercase(), "white");
        assert!(snippet.contains("Screen") || snippet.contains("Screen"));
    }

    #[test]
    fn short_bodies_are_returned_whole_without_ellipsis() {
        let (snippet, start, len) = build("白屏", "", "白屏");
        assert_eq!(snippet, "白屏");
        assert_eq!(start, Some(0));
        assert_eq!(len, 2);
    }

    #[test]
    fn long_bodies_are_windowed_with_ellipsis() {
        let body = format!("{}{}{}", "前".repeat(200), "目标词", "后".repeat(200));
        let (snippet, start, len) = build(&body, "", "目标词");
        assert!(snippet.starts_with('…'), "should elide the head");
        assert!(snippet.ends_with('…'), "should elide the tail");
        assert!(snippet.chars().count() < body.chars().count());

        // Offsets must still point at the match inside the snippet.
        let start = start.unwrap();
        let matched: String = snippet.chars().skip(start).take(len).collect();
        assert_eq!(matched, "目标词");
    }

    #[test]
    fn falls_back_to_the_title_when_the_body_has_no_match() {
        // The hit came from the title, so the body has no literal term.
        let (snippet, start, len) = build("unrelated content", "重构计划", "重构");
        assert_eq!(snippet, "重构计划");
        assert_eq!(start, None);
        assert_eq!(len, 0);
    }

    #[test]
    fn tries_each_query_term_until_one_matches() {
        let body = "only the second term appears here";
        let (_, start, len) = build(body, "", "missing term");
        let start = start.expect("second term should match");
        assert!(start > 0);
        assert_eq!(len, "term".chars().count());
    }

    #[test]
    fn handles_empty_body() {
        let (snippet, start, len) = build("", "Some Title", "query");
        assert_eq!(snippet, "Some Title");
        assert_eq!(start, None);
        assert_eq!(len, 0);
    }

    #[test]
    fn multibyte_content_keeps_offsets_aligned() {
        // Emoji and CJK mixed — offsets must be character-based, not byte-based.
        let body = "🎉 开始 修复轨迹 结束 🚀";
        let (snippet, start, len) = build(body, "", "轨迹");
        let start = start.unwrap();
        let matched: String = snippet.chars().skip(start).take(len).collect();
        assert_eq!(matched, "轨迹");
    }
}
