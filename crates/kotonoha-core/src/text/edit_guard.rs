//! Decides whether an LLM rewrite may be used. The model is only allowed to delete: fillers, the words a
//! speaker took back, and punctuation it moves. Any output that adds text, rewords a phrase or loses a
//! word the speaker meant is rejected, and the rule-based text is used instead.

use std::sync::LazyLock;

use unicode_normalization::UnicodeNormalization;

use super::chars::{is_content, is_punctuation};
use super::spoken::{longest_first, CORRECTION_CUES, HESITATIONS, WORD_LIKE_FILLERS};

/// Everything the model may delete outright, longest spelling first.
static DELETABLE: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| longest_first(HESITATIONS.iter().chain(WORD_LIKE_FILLERS).copied()));
/// Symbols ignored when comparing, on top of `is_punctuation`.
const EXTRA_IGNORED: &str = "、。，．・「」『』（）()!?！？〜~";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Share of output characters that may be new (a few changed characters are fine).
    pub max_added_ratio: f64,
    /// Share of input characters that must survive.
    pub min_kept_ratio: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_added_ratio: 0.08, min_kept_ratio: 0.5 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    EmptyOutput,
    Paraphrased,
    TooMuchDeleted,
    ContentWordDropped,
    WordsDropped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub added_ratio: f64,
    pub kept_ratio: f64,
    pub rejection: Option<Rejection>,
}

impl Verdict {
    pub fn accepted(&self) -> bool {
        self.rejection.is_none()
    }
}

pub fn check(input: &str, output: &str) -> Verdict {
    check_with(input, output, Limits::default())
}

pub fn check_with(input: &str, output: &str, limits: Limits) -> Verdict {
    let a: Vec<char> = normalize(input).chars().collect();
    let b: Vec<char> = normalize(output).chars().collect();
    let verdict = |added, kept, rejection| Verdict { added_ratio: added, kept_ratio: kept, rejection };

    if a.is_empty() {
        let rejection = (!b.is_empty()).then_some(Rejection::Paraphrased);
        return verdict(if b.is_empty() { 0.0 } else { 1.0 }, 1.0, rejection);
    }
    if b.is_empty() {
        return verdict(0.0, 0.0, Some(Rejection::EmptyOutput));
    }

    let kept_mask = lcs_mask(&a, &b);
    let common = kept_mask.iter().filter(|&&k| k).count();
    let added_chars = b.len() - common;
    let added = added_chars as f64 / b.len() as f64;
    let kept = common as f64 / a.len() as f64;

    if added > limits.max_added_ratio && added_chars > 3 {
        return verdict(added, kept, Some(Rejection::Paraphrased));
    }
    // Taking a phrase back can remove much of a short input
    let has_correction = CORRECTION_CUES.iter().any(|m| input.contains(m));
    let min_kept = if has_correction { limits.min_kept_ratio.min(0.3) } else { limits.min_kept_ratio };
    if kept < min_kept {
        return verdict(added, kept, Some(Rejection::TooMuchDeleted));
    }

    // Look at each run of deleted characters. Near a correction cue anything may go. Elsewhere a deleted
    // kanji, katakana or Latin word must still appear later in the output, and once the fillers are
    // taken out of the run, a particle or two is all that may be left.
    let output_text: String = b.iter().collect();
    let mut i = 0;
    while i < a.len() {
        if kept_mask[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < a.len() && !kept_mask[j] {
            j += 1;
        }
        let span: String = a[i..j].iter().collect();
        let context: String = a[i.saturating_sub(4)..(j + 6).min(a.len())].iter().collect();
        if !CORRECTION_CUES.iter().any(|m| context.contains(m)) {
            if content_words(&span).iter().any(|w| !output_text.contains(w.as_str())) {
                return verdict(added, kept, Some(Rejection::ContentWordDropped));
            }
            let rest = DELETABLE.iter().fold(span, |s, f| s.replace(f, ""));
            if rest.chars().count() > 2 {
                return verdict(added, kept, Some(Rejection::WordsDropped));
            }
        }
        i = j;
    }
    verdict(added, kept, None)
}

/// The text reduced to its words: NFKC, with spaces, punctuation and brackets taken out.
fn normalize(s: &str) -> String {
    s.nfkc().filter(|&c| !c.is_whitespace() && !is_punctuation(c) && !EXTRA_IGNORED.contains(c)).collect()
}

/// Words that carry meaning: two or more kanji, katakana or ASCII letters and digits in a row.
fn content_words(s: &str) -> Vec<String> {
    s.split(|c: char| !is_content(c)).filter(|w| w.chars().count() >= 2).map(str::to_owned).collect()
}

/// For each character of `a`, whether it survives in `b` (by a longest common subsequence).
fn lcs_mask(a: &[char], b: &[char]) -> Vec<bool> {
    let (n, m) = (a.len(), b.len());
    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if a[i] == b[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }
    let mut mask = vec![false; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            mask[i] = true;
            i += 1;
            j += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_fillers_and_taken_back_words_is_fine() {
        assert!(check("会費は、えっと、一人2000円です", "会費は一人2000円です。").accepted());
        assert!(check("集合は北口、いや南口の前です", "集合は南口の前です").accepted());
        assert!(check(
            "荷物は明日届くと聞いています。じゃなくて、明後日届くと聞いています",
            "荷物は明後日届くと聞いています。"
        )
        .accepted());
    }

    #[test]
    fn rewording_adding_or_dropping_words_is_rejected() {
        let reworded =
            check("図書館の本は来週返すつもりです。", "図書館の本につきましては、来週返却する予定でございます。");
        assert_eq!(reworded.rejection, Some(Rejection::Paraphrased));
        assert!(!check("了解です", "了解しました。確認のうえ、改めてご連絡いたします。").accepted());
        assert!(!check(
            "新しいSlackのチャンネルなんだけど、通知を切ってあります",
            "新しいSlackのチャンネルの通知を切ってあります。"
        )
        .accepted());
        assert!(!check("図書館の本は来週返すつもりです。", "図書館の本は返すつもりです。").accepted());
    }

    #[test]
    fn empty_cases() {
        assert!(check("", "").accepted());
        assert_eq!(check("何か", "").rejection, Some(Rejection::EmptyOutput));
    }
}
