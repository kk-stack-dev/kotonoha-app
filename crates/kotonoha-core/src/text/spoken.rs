//! Word lists for spoken Japanese, shared by the rules and the edit guard. Spellings are grouped by sound;
//! callers that match against text sort them with `longest_first`.

/// Hesitation sounds. The one- and two-character ones ("え", "ええ", "うん") are also replies, so the
/// rules leave them alone; the LLM may still drop them.
pub const HESITATIONS: &[&str] = &[
    // え
    "え",
    "えー",
    "ええ",
    "えっと",
    "えーと",
    "ええと",
    "えーっと",
    "ええっと",
    // あ, う
    "あ",
    "あー",
    "うん",
    "うーん",
    // Drawn-out demonstratives
    "あのー",
    "そのー",
];

/// Sounds that are never a reply or part of a word when they stand alone.
pub fn standalone_hesitations() -> impl Iterator<Item = &'static str> {
    HESITATIONS.iter().copied().filter(|h| h.chars().count() >= 2 && !matches!(*h, "ええ" | "うん"))
}

/// Fillers that are also ordinary words ("あの人", "その件", "まあまあ", "そうですね").
pub const WORD_LIKE_FILLERS: &[&str] = &["あの", "その", "まあ", "なんか", "ですね", "ね"];

/// Words that announce a self-correction: a negation of what was just said, or taking it back.
pub const CORRECTION_CUES: &[&str] =
    &["じゃなくて", "ではなく", "いや", "違う", "ちがう", "間違え", "やっぱり", "訂正", "失礼"];

/// The words sorted so that a longer spelling is tried before a shorter one it contains ("えーっと" before
/// "えー").
pub fn longest_first<'a>(words: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut words: Vec<&str> = words.into_iter().collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    words.dedup();
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_spellings_come_first() {
        let sorted = longest_first(HESITATIONS.iter().copied());
        let pos = |w: &str| sorted.iter().position(|s| *s == w).unwrap();
        assert!(pos("えーっと") < pos("えーと") && pos("えーと") < pos("えー") && pos("えー") < pos("え"));
        assert_eq!(sorted.len(), HESITATIONS.len());
    }

    #[test]
    fn replies_are_not_standalone_hesitations() {
        let standalone: Vec<&str> = standalone_hesitations().collect();
        assert!(standalone.contains(&"えっと") && standalone.contains(&"うーん"));
        assert!(!standalone.contains(&"ええ") && !standalone.contains(&"うん") && !standalone.contains(&"え"));
    }
}
