//! Cleans raw Whisper output.

use std::sync::LazyLock;

use regex::Regex;

static TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\d{2}:\d{2}:\d{2}\.\d{3}\s*-->\s*\d{2}:\d{2}:\d{2}\.\d{3}\]").unwrap());
/// Non-speech markers such as [BLANK_AUDIO] or [MUSIC].
static SPECIAL_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[A-Z0-9_]+\]").unwrap());

/// Whisper, trained on video subtitles, fills silence or noise with a video's closing lines. A chunk that
/// says nothing but one of these is dropped; the same words inside real speech are kept.
const SILENCE_PHRASES: &[&str] = &[
    // Thanks for watching, in its usual forms
    "ご視聴ありがとうございました",
    "ご視聴いただきありがとうございました",
    "最後までご視聴いただきありがとうございました",
    "ご清聴ありがとうございました",
    // Channel sign-offs
    "チャンネル登録よろしくお願いします",
    "チャンネル登録お願いします",
    "おやすみなさい",
];

pub fn clean_whisper_output(raw: &str) -> String {
    let text = TIMESTAMP.replace_all(raw, "");
    SPECIAL_TOKEN.replace_all(&text, "").trim().to_owned()
}

pub fn is_known_hallucination(text: &str) -> bool {
    let words: String = text.chars().filter(|c| !c.is_whitespace() && !"。、．.!！".contains(*c)).collect();
    SILENCE_PHRASES.contains(&words.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_timestamps_and_markers() {
        assert_eq!(clean_whisper_output("[00:00:01.000 --> 00:00:03.500] 傘を忘れました [MUSIC]\n"), "傘を忘れました");
    }

    #[test]
    fn a_chunk_of_only_a_closing_line_is_a_hallucination() {
        assert!(is_known_hallucination(" チャンネル登録お願いします！ "));
        assert!(!is_known_hallucination("発表は以上です。ご清聴ありがとうございました。"));
    }
}
