//! Character classes shared by the text rules.
//!
//! All length thresholds in this crate count Unicode scalar values (`char`s), never bytes.

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{309F}' | 'ー')
}

pub fn is_katakana(c: char) -> bool {
    matches!(c, '\u{30A0}'..='\u{30FF}')
}

pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}')
}

pub fn is_ascii_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Unicode general category P* (what Foundation calls punctuation characters).
pub fn is_punctuation(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Punctuation
}

/// Kanji, katakana and ASCII letters / digits: the characters that carry meaning in a Japanese sentence.
pub fn is_content(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric()
    } else {
        is_kanji(c) || is_katakana(c)
    }
}

/// Last `n` characters of `s`.
pub fn suffix(s: &str, n: usize) -> &str {
    let len = char_len(s);
    if n >= len {
        return s;
    }
    let start = s.char_indices().nth(len - n).map_or(s.len(), |(i, _)| i);
    &s[start..]
}

/// First `n` characters of `s`.
pub fn prefix(s: &str, n: usize) -> &str {
    let end = s.char_indices().nth(n).map_or(s.len(), |(i, _)| i);
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_and_suffix_count_chars() {
        assert_eq!(prefix("火曜日です", 2), "火曜");
        assert_eq!(suffix("火曜日です", 2), "です");
        assert_eq!(suffix("abc", 10), "abc");
        assert_eq!(prefix("abc", 10), "abc");
    }

    #[test]
    fn classes() {
        assert!(is_punctuation('、') && is_punctuation('。') && is_punctuation('!'));
        assert!(!is_punctuation('々'));
        assert!(is_content('漢') && is_content('カ') && is_content('A') && is_content('7'));
        assert!(!is_content('あ') && !is_content('、'));
        assert!(is_hiragana('ー'));
    }
}
