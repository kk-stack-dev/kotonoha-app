//! Joins the transcripts of consecutive chunks. Chunks share a short stretch of audio, so the end of one
//! transcript usually repeats at the start of the next; that repeat is kept once.

use super::chars::{char_len, prefix, suffix};

/// Longest repeat looked for, in characters: the shared audio is short, and Japanese speech runs at
/// roughly 6-8 characters a second.
const MAX_OVERLAP_CHARS: usize = 60;
const MIN_OVERLAP_CHARS: usize = 3;
const MAX_OVERLAP_WORDS: usize = 8;

pub fn merge_all<S: AsRef<str>>(transcripts: &[S]) -> String {
    let mut iter = transcripts.iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    iter.fold(first.as_ref().to_owned(), |acc, next| merge(&acc, next.as_ref()))
}

/// Merges `next` onto `previous`, dropping the text duplicated by the audio overlap.
pub fn merge(previous: &str, next: &str) -> String {
    let prev = previous.trim();
    let next = next.trim();
    if prev.is_empty() {
        return next.to_owned();
    }
    if next.is_empty() {
        return prev.to_owned();
    }
    if next.starts_with(prev) {
        return next.to_owned();
    }
    if prev.ends_with(next) {
        return prev.to_owned();
    }

    if let Some(len) = char_overlap(prev, next) {
        let rest = next[prefix(next, len).len()..].trim();
        if rest.is_empty() {
            return prev.to_owned();
        }
        return join(prev, rest);
    }

    if let Some(rest) = word_overlap(prev, next) {
        return format!("{prev} {rest}");
    }

    join(prev, next)
}

fn char_overlap(prev: &str, next: &str) -> Option<usize> {
    let max = char_len(prev).min(char_len(next)).min(MAX_OVERLAP_CHARS);
    (MIN_OVERLAP_CHARS..=max).rev().find(|&len| {
        let tail = suffix(prev, len);
        let head = prefix(next, len);
        if tail == head {
            return true;
        }
        let tail = strip_for_comparison(tail);
        char_len(&tail) >= MIN_OVERLAP_CHARS && tail == strip_for_comparison(head)
    })
}

fn word_overlap(prev: &str, next: &str) -> Option<String> {
    let prev_words: Vec<&str> = prev.split(' ').filter(|w| !w.is_empty()).collect();
    let next_words: Vec<&str> = next.split(' ').filter(|w| !w.is_empty()).collect();
    if prev_words.len() < 2 || next_words.len() < 2 {
        return None;
    }
    let max = prev_words.len().min(next_words.len()).min(MAX_OVERLAP_WORDS);
    (1..=max).rev().find(|&n| prev_words[prev_words.len() - n..] == next_words[..n]).map(|n| next_words[n..].join(" "))
}

fn strip_for_comparison(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '、' | '。' | ' ' | '\n' | '\t' | ',' | '.' | '・')).collect()
}

/// Latin words need a space between them; Japanese text never does.
fn join(prev: &str, next: &str) -> String {
    let wordy = |c: char| c.is_ascii_alphanumeric();
    match (prev.chars().last(), next.chars().next()) {
        (Some(a), Some(b)) if wordy(a) && wordy(b) => format!("{prev} {next}"),
        _ => format!("{prev}{next}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repeated_stretch_is_kept_once() {
        assert_eq!(
            merge("会議室のプロジェクターを予約して", "プロジェクターを予約してから資料を印刷します"),
            "会議室のプロジェクターを予約してから資料を印刷します"
        );
    }

    #[test]
    fn several_chunks_join_without_repeats() {
        let merged = merge_all(&[
            "朝は七時に起きます",
            "七時に起きます。それから犬の散歩に行きます",
            "犬の散歩に行きます。帰ったら朝ごはんです",
        ]);
        assert_eq!(merged, "朝は七時に起きます。それから犬の散歩に行きます。帰ったら朝ごはんです");
    }

    #[test]
    fn empty_and_unrelated_chunks() {
        assert_eq!(merge_all::<&str>(&[]), "");
        assert_eq!(merge("", "あと"), "あと");
        assert_eq!(merge("まえ", ""), "まえ");
        assert_eq!(merge("good", "morning"), "good morning");
        assert_eq!(merge("晴れのち", "くもり"), "晴れのちくもり");
    }

    #[test]
    fn latin_words_overlap_by_word() {
        assert_eq!(merge("please send the", "the invoice tomorrow"), "please send the invoice tomorrow");
    }
}
