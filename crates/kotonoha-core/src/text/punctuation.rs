//! Final punctuation and spacing clean-up.

use std::sync::LazyLock;

use regex::Regex;

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

const SENTENCE_ENDINGS: &[char] = &['。', '！', '？', '!', '?', '…', '」', '）', ')'];

pub fn finish_sentence(text: &str) -> String {
    let trimmed = text.trim().trim_end_matches(['、', ',']);
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = trimmed.to_owned();
    if !out.ends_with(SENTENCE_ENDINGS) {
        out.push('。');
    }
    let out = SPACES.replace_all(&out, " ");
    BLANK_LINES.replace_all(&out, "\n\n").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_a_period_and_trims() {
        assert_eq!(finish_sentence("  明日は休みです、  "), "明日は休みです。");
        assert_eq!(finish_sentence("間に合いますか？"), "間に合いますか？");
        assert_eq!(finish_sentence("   "), "");
        assert_eq!(finish_sentence("a  b\n\n\n\nc"), "a b\n\nc。");
    }
}
