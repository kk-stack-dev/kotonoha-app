//! Rewrites known misrecognitions to the dictionary spelling ("クロード" -> "Claude").
//! Deterministic, so unlike an LLM it cannot invent content.

use super::chars::{is_ascii_letter, is_katakana};

/// A known misrecognition and the spelling it should become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub variant: String,
    pub word: String,
}

/// Applies `replacements` in order. Callers pass them longest variant first.
pub fn normalize(text: &str, replacements: &[Replacement]) -> String {
    replacements.iter().fold(text.to_owned(), |acc, r| replace_whole_word(&acc, &r.variant, &r.word))
}

/// Replaces `variant` only where it is not part of a longer Latin or katakana word,
/// so "Claud" never matches inside "Claude" and "クロード" never inside "クロードモネ".
pub fn replace_whole_word(text: &str, variant: &str, word: &str) -> String {
    let (Some(first), Some(last)) = (variant.chars().next(), variant.chars().last()) else {
        return text.to_owned();
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (start, matched) in text.match_indices(variant) {
        let end = start + matched.len();
        let before = text[..start].chars().last();
        let after = text[end..].chars().next();
        out.push_str(&text[rest..start]);
        if boundary_ok(first, before) && boundary_ok(last, after) {
            out.push_str(word);
        } else {
            out.push_str(matched);
        }
        rest = end;
    }
    out.push_str(&text[rest..]);
    out
}

fn boundary_ok(edge: char, neighbour: Option<char>) -> bool {
    match neighbour {
        None => true,
        Some(n) => !(is_ascii_letter(edge) && is_ascii_letter(n) || is_katakana(edge) && is_katakana(n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(variant: &str, word: &str) -> Replacement {
        Replacement { variant: variant.into(), word: word.into() }
    }

    #[test]
    fn whole_words_only() {
        let reps = [r("Github", "GitHub"), r("スラック", "Slack")];
        assert_eq!(normalize("GitHubとGithubに置きました", &reps), "GitHubとGitHubに置きました");
        assert_eq!(normalize("スラックスとスラック", &reps), "スラックスとSlack");
    }
}
