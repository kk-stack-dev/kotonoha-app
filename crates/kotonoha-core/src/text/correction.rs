//! Spoken self-corrections. When the speaker takes a word back ("木曜、いや金曜", "AではなくてB"), only
//! the replacement is kept; a phrase said twice in a row ("駅前の駅前の") is kept once.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::chars::is_hiragana;

/// The word being taken back: up to 8 characters with no punctuation, space or particle in it, so the
/// words before it survive ("会議は木曜、いや金曜" keeps "会議は").
const TAKEN_BACK: &str = r"([^、。\n\sのはがをにでとも]{1,8})";

/// "いや" counts only after a comma or a space; "それはいやです" has neither.
static IYA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"{TAKEN_BACK}[、\s]+いや[、\s]*([^、。\n\s]{{1,8}})")).unwrap());
static JANAKUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"{TAKEN_BACK}[、\s]*(?:じゃなくて|ではなくて|ではなく)[、\s]*([^、。\n\s]{{1,15}})")).unwrap()
});
/// Right after "いや", these make it "unpleasant" ("いやな", "いやだ", "いやがる"), not "no, I mean".
const IYA_AS_DISLIKE: &[&str] = &["な", "だ", "がる"];

const STUTTER_MIN: usize = 3;
const STUTTER_MAX: usize = 8;

pub fn apply_corrections(text: &str) -> String {
    let out = IYA.replace_all(text, |c: &Captures| {
        let after_iya = c[0].split_once("いや").map_or("", |(_, rest)| rest);
        if IYA_AS_DISLIKE.iter().any(|w| after_iya.starts_with(w)) {
            return c[0].to_owned();
        }
        c[2].to_owned()
    });
    let out = JANAKUTE.replace_all(&out, |c: &Captures| {
        // Only hiragana before "じゃなくて" is most often the end of a longer phrase, not a word taken back
        if c[1].chars().all(is_hiragana) {
            c[0].to_owned()
        } else {
            c[2].to_owned()
        }
    });
    collapse_stutters(&out)
}

/// Keeps one copy of a run of 3 to 8 characters said twice in a row. Shorter doublings are left, since
/// many words are built that way ("ときどき", "ますます").
fn collapse_stutters(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: &char| !matches!(c, '、' | '。' | '\n') && !c.is_whitespace();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let repeat = (STUTTER_MIN..=STUTTER_MAX).rev().find(|&n| {
            i + 2 * n <= chars.len()
                && chars[i..i + n].iter().all(is_word)
                && chars[i..i + n] == chars[i + n..i + 2 * n]
        });
        match repeat {
            Some(n) => {
                out.extend(&chars[i..i + n]);
                i += 2 * n;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iya_keeps_the_replacement() {
        assert_eq!(apply_corrections("集合は駅の北口、いや南口にしましょう"), "集合は駅の南口にしましょう");
        assert_eq!(apply_corrections("会議は木曜、いや金曜の朝に移ります"), "会議は金曜の朝に移ります");
    }

    #[test]
    fn janakute_keeps_the_replacement() {
        assert_eq!(apply_corrections("飲み物はコーヒーじゃなくて紅茶をください"), "飲み物は紅茶をください");
        assert_eq!(apply_corrections("3階ではなく5階に来てください"), "5階に来てください");
    }

    #[test]
    fn non_corrections_stay() {
        assert_eq!(apply_corrections("雨の日はいやですね"), "雨の日はいやですね");
        assert_eq!(apply_corrections("この服、いやな色ではないです"), "この服、いやな色ではないです");
        let doubled = "ときどき散歩して、ますます元気になりました";
        assert_eq!(apply_corrections(doubled), doubled);
    }

    #[test]
    fn a_phrase_said_twice_is_kept_once() {
        assert_eq!(apply_corrections("駅前の駅前の本屋で買いました"), "駅前の本屋で買いました");
    }
}
