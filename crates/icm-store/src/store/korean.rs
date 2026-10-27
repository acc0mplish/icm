//! Korean query analysis for the existing unicode61 memory index.
//!
//! This is intentionally query-side: no schema or stored content changes,
//! and old binaries can keep writing to the same database. Prefix and
//! substring matches are a useful approximation, not a morpheme index.

use std::borrow::Cow;
use std::sync::OnceLock;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use unicode_normalization::UnicodeNormalization;

use super::truncate_at_char_boundary;

// Queries may come from hooks and untrusted MCP clients. Bound both the
// segmenter's input and the generated SQL / FTS expression.
const MAX_ANALYSIS_BYTES: usize = 4096;
const MAX_TERMS: usize = 12;

fn is_hangul(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11ff | 0x3130..=0x318f | 0xa960..=0xa97f | 0xac00..=0xd7ff)
}

fn segmenter() -> Option<&'static Segmenter> {
    static SEGMENTER: OnceLock<Option<Segmenter>> = OnceLock::new();
    SEGMENTER
        .get_or_init(|| match load_dictionary("embedded://ko-dic") {
            Ok(dictionary) => Some(Segmenter::new(Mode::Normal, dictionary, None)),
            Err(error) => {
                tracing::warn!(%error, "Korean dictionary unavailable; using ordinary recall");
                None
            }
        })
        .as_ref()
}

/// Return distinct NFC Korean content surfaces. Particles and endings are
/// excluded from the extra search, while the unmodified query still goes
/// through the original FTS arm. Analysis errors fall back to that arm.
pub(super) fn terms(query: &str) -> Vec<String> {
    if !query.chars().any(is_hangul) {
        return Vec::new();
    }
    let Some(segmenter) = segmenter() else {
        return Vec::new();
    };
    let text: String = truncate_at_char_boundary(query, MAX_ANALYSIS_BYTES)
        .nfc()
        .collect();
    let mut tokens = match segmenter.segment(Cow::Borrowed(&text)) {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::warn!(%error, "Korean query analysis failed; using ordinary recall");
            return Vec::new();
        }
    };
    let mut terms = Vec::new();
    for token in &mut tokens {
        let surface = token.surface.to_string();
        if surface.is_empty() || !surface.chars().all(is_hangul) {
            continue;
        }
        let details = token.details();
        let tag = details.first().copied().unwrap_or("");
        let head = tag.split('+').next().unwrap_or(tag);
        if head.starts_with('J')
            || head.starts_with('E')
            || head.starts_with('X')
            || matches!(head, "NP" | "NNB" | "NNBC" | "VX" | "VCP" | "VCN")
        {
            continue;
        }
        // A one-character auxiliary/adverb/verb matches too many unrelated
        // words in the substring scan. Keep real one-character nouns such
        // as "집"; the original FTS query still handles other exact terms.
        if surface.chars().count() == 1 && !matches!(head, "NNG" | "NNP") {
            continue;
        }
        if !terms.contains(&surface) {
            terms.push(surface);
            if terms.len() >= MAX_TERMS {
                break;
            }
        }
    }
    terms
}

/// Include decomposed Hangul so databases written in NFC and NFD both work.
pub(super) fn variants(term: &str) -> Vec<String> {
    let nfd: String = term.nfd().collect();
    if nfd == term {
        vec![term.to_owned()]
    } else {
        vec![term.to_owned(), nfd]
    }
}

/// OR extra prefix terms into the natural-language v2 FTS expression. The
/// surfaces consist only of Hangul, so they cannot inject FTS syntax.
pub(super) fn expand_fts(mut original: String, terms: &[String]) -> String {
    for term in terms {
        for variant in variants(term) {
            if !original.is_empty() {
                original.push_str(" OR ");
            }
            original.push('"');
            original.push_str(&variant);
            original.push_str("\"*");
        }
    }
    original
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_surfaces_skip_particles_and_endings_but_keep_short_nouns() {
        let analyzed = terms("사건을 집에서 발생했습니다");
        assert!(analyzed.iter().any(|term| term == "사건"), "{analyzed:?}");
        assert!(analyzed.iter().any(|term| term == "집"), "{analyzed:?}");
        assert!(analyzed.iter().any(|term| term == "발생"), "{analyzed:?}");
        for stop in ["을", "에서", "했", "습니다"] {
            assert!(!analyzed.iter().any(|term| term == stop), "{analyzed:?}");
        }
    }

    #[test]
    fn input_and_generated_terms_are_bounded() {
        assert!(terms("ordinary English INC-2023-Q4-011").is_empty());
        let long = "사건을 수정하고 집에서 발생했습니다 ".repeat(500);
        assert!(terms(&long).len() <= MAX_TERMS);
    }
}
