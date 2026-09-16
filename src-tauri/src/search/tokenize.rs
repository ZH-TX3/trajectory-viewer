// ── Search tokenization ──────────────────────────────────────────────────
//
// FTS5's `unicode61` tokenizer treats a run of CJK characters as ONE token,
// so `轨迹` never matches `修复轨迹查看器`. `trigram` is also unusable here:
// it needs >= 3 characters, and most Chinese words are two (`白屏`, `轨迹`).
//
// The fix is to split CJK runs into per-character tokens before indexing,
// leaving ASCII words intact. Query text goes through the SAME transform, so
// a multi-character query becomes an ordered FTS5 phrase.
//
// Index and query MUST use `index_text` identically — a mismatch means
// silent zero results, so both paths are covered by tests below.

/// CJK Unified Ideographs (incl. Ext-A) plus kana and Hangul.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF     // CJK Ext-A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0x3040..=0x30FF   // Hiragana + Katakana
        | 0xAC00..=0xD7AF   // Hangul Syllables
        | 0x20000..=0x2FA1F // CJK Ext-B..F
    )
}

/// Split CJK characters into individual whitespace-separated tokens.
///
/// ASCII words are preserved whole, so `fix bug` stays two searchable words
/// while `修复轨迹` becomes four per-character tokens.
pub fn index_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut pending_space = false;

    for ch in text.chars() {
        if is_cjk(ch) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push(ch);
            out.push(' ');
            pending_space = false;
        } else if ch.is_whitespace() {
            // Collapse runs of whitespace to a single separator.
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Build an FTS5 MATCH expression from a user query.
///
/// Each whitespace-separated term becomes a quoted phrase over the tokenized
/// form, and all terms must match (AND). Returns `None` when the query has no
/// searchable content, so callers can skip the query entirely.
///
/// Quoting also neutralises FTS5 syntax characters, so a query like `a AND b`
/// or `"` is treated as literal text rather than an operator.
pub fn query_expr(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(index_text)
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();

    if terms.is_empty() {
        return None;
    }
    Some(terms.join(" AND "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_chinese_into_single_characters() {
        assert_eq!(index_text("修复轨迹"), "修 复 轨 迹");
        assert_eq!(index_text("白屏"), "白 屏");
        assert_eq!(index_text("查看器"), "查 看 器");
    }

    #[test]
    fn keeps_ascii_words_intact() {
        assert_eq!(index_text("fix the bug"), "fix the bug");
        assert_eq!(index_text("white screen"), "white screen");
    }

    #[test]
    fn handles_mixed_scripts() {
        // The ASCII word stays whole; the CJK run is split.
        assert_eq!(index_text("修复bug问题"), "修 复 bug 问 题");
        assert_eq!(index_text("Vite 构建"), "Vite 构 建");
    }

    #[test]
    fn collapses_whitespace_and_punctuation_stays() {
        assert_eq!(index_text("  hello   world  "), "hello world");
        assert_eq!(index_text("a\n\tb"), "a b");
    }

    #[test]
    fn empty_and_whitespace_input() {
        assert_eq!(index_text(""), "");
        assert_eq!(index_text("   "), "");
        assert_eq!(index_text("\n\t"), "");
    }

    #[test]
    fn index_and_query_use_the_same_transform() {
        // The invariant the whole feature rests on: every token produced by
        // tokenizing a query must appear in the tokenized document.
        for s in ["白屏", "fix bug", "修复bug", "轨迹查看器"] {
            let expr = query_expr(s).unwrap();
            let doc = index_text(s);
            // Each quoted phrase in the expression must be a substring of the
            // tokenized document (so a document containing `s` matches).
            for phrase in expr.split(" AND ") {
                let inner = phrase.trim_matches('"');
                assert!(
                    doc.contains(inner),
                    "query {s:?}: phrase {inner:?} not found in doc {doc:?}"
                );
            }
        }
    }

    #[test]
    fn query_builds_an_and_of_phrases() {
        assert_eq!(query_expr("白屏").unwrap(), "\"白 屏\"");
        assert_eq!(
            query_expr("white screen").unwrap(),
            "\"white\" AND \"screen\""
        );
        assert_eq!(query_expr("修复 白屏").unwrap(), "\"修 复\" AND \"白 屏\"");
    }

    #[test]
    fn query_returns_none_for_empty_input() {
        assert!(query_expr("").is_none());
        assert!(query_expr("   ").is_none());
        assert!(query_expr("\n\t").is_none());
    }

    #[test]
    fn query_neutralises_fts5_syntax() {
        // A user typing operators must not change the query's meaning.
        let expr = query_expr("a AND b").unwrap();
        assert_eq!(expr, "\"a\" AND \"AND\" AND \"b\"");
        // A bare quote would otherwise be a syntax error.
        assert!(query_expr("\"").is_some());
        assert!(query_expr("a*").is_some());
    }
}
