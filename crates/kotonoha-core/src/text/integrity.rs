//! The final check on LLM output, after the edit guard: every number (with its unit), URL, e-mail address,
//! capitalised Latin word and protected dictionary word in the transcript must still be there, spelled the
//! same way, and the text must not have shrunk to a summary or grown.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::chars::char_len;

static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[a-zA-Z0-9./?=_-]+").unwrap());
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
/// Units that belong to the number in front of them, so "10時" and "10分" count as different values.
const UNITS: &[&[&str]] =
    &[&["年", "月", "日", "時", "分", "秒"], &["円", "万", "億", "%"], &["個", "つ", "人", "名", "回", "件", "社"]];
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    let units = UNITS.iter().flat_map(|group| group.iter().copied()).collect::<Vec<_>>().join("|");
    Regex::new(&format!(r"\d+(?:\.\d+)?(?:{units})?")).unwrap()
});
static LATIN_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z][a-zA-Z0-9]+").unwrap());

const HALF_DIGITS: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
const FULL_DIGITS: [char; 10] = ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityFailure {
    /// The candidate is far shorter or longer than the transcript (summarised or padded).
    LengthChanged,
    /// Important tokens are missing, or URLs / addresses were invented.
    TokensChanged { missing: Vec<String>, invented: Vec<String> },
}

/// Returns the candidate (with digit-width variants repaired back to the transcript's spelling),
/// or why it must not be used.
pub fn validate(transcript: &str, candidate: &str, protected_words: &[String]) -> Result<String, IntegrityFailure> {
    let mut repaired = candidate.to_owned();
    let mut missing = Vec::new();
    for entity in important_entities(transcript, protected_words) {
        if repaired.contains(&entity) {
            continue;
        }
        match width_variant_in(&entity, &repaired) {
            Some(variant) => repaired = repaired.replace(&variant, &entity),
            None => missing.push(entity),
        }
    }

    let invented: Vec<String> = important_entities(&repaired, &[])
        .into_iter()
        .filter(|e| (e.contains("://") || e.contains('@')) && !transcript.contains(e.as_str()))
        .collect();

    let raw_len = char_len(transcript.trim());
    let new_len = char_len(repaired.trim());
    if raw_len > 20 && ((new_len as f64) < raw_len as f64 * 0.3 || new_len as f64 > raw_len as f64 * 2.5) {
        return Err(IntegrityFailure::LengthChanged);
    }
    if !missing.is_empty() || !invented.is_empty() {
        return Err(IntegrityFailure::TokensChanged { missing, invented });
    }
    Ok(repaired)
}

/// Numbers (with units), URLs, e-mail addresses, capitalised Latin words and protected words found in `text`.
pub fn important_entities(text: &str, protected_words: &[String]) -> BTreeSet<String> {
    let mut found: BTreeSet<String> =
        protected_words.iter().filter(|w| !w.is_empty() && text.contains(w.as_str())).cloned().collect();
    for re in [&*URL, &*EMAIL, &*NUMBER, &*LATIN_NAME] {
        found.extend(re.find_iter(text).map(|m| m.as_str().to_owned()));
    }
    found
}

/// Finds `entity` written with the other digit width ("3つ" vs "３つ").
fn width_variant_in(entity: &str, text: &str) -> Option<String> {
    let swap = |from: &[char; 10], to: &[char; 10]| -> String {
        entity.chars().map(|c| from.iter().position(|&d| d == c).map_or(c, |i| to[i])).collect()
    };
    [swap(&HALF_DIGITS, &FULL_DIGITS), swap(&FULL_DIGITS, &HALF_DIGITS)]
        .into_iter()
        .find(|v| v != entity && text.contains(v.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_text_passes() {
        let text = "11月3日の13時に、https://example.org/form から申し込んでください。定員は40名で、Zoomで行います。";
        let words = vec!["Zoom".to_owned(), "Slack".to_owned()];
        assert_eq!(validate(text, text, &words).as_deref(), Ok(text));
    }

    #[test]
    fn changed_times_and_amounts_fail() {
        let result =
            validate("受付は9時半からで、参加費は3000円です。", "受付は10時半からで、参加費は300円です。", &[]);
        assert!(matches!(result, Err(IntegrityFailure::TokensChanged { .. })));
    }

    #[test]
    fn full_width_digits_are_put_back() {
        assert_eq!(
            validate("部屋は2つ予約しました。", "部屋は２つ予約しました。", &[]).as_deref(),
            Ok("部屋は2つ予約しました。")
        );
    }

    #[test]
    fn invented_urls_and_summaries_fail() {
        assert!(
            validate("申し込みは案内のページからどうぞ", "申し込みは https://other.example からどうぞ", &[]).is_err()
        );
        let long = "午前中は書類の整理をして、午後から取引先を二件回り、夕方に戻って報告書を書く予定です。";
        assert_eq!(validate(long, "報告書を書きます。", &[]), Err(IntegrityFailure::LengthChanged));
    }
}
