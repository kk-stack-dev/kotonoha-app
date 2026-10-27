//! Line breaks for dictated text. Speech comes out as one run of sentences, so each sentence goes on its
//! own line. A sentence end inside 「…」 does not break.

const SENTENCE_ENDS: &[char] = &['。', '！', '？', '!', '?'];
const OPEN_QUOTES: &[char] = &['「', '『', '（', '('];
const CLOSE_QUOTES: &[char] = &['」', '』', '）', ')'];

/// Puts each sentence of `text` on its own line. Existing line breaks are kept.
pub fn break_lines(text: &str) -> String {
    text.split('\n').map(break_line).collect::<Vec<_>>().join("\n")
}

/// Puts the line breaks of `copy`, a model's copy of `text`, into `text`. None when the copy changed any
/// character other than spaces. Breaks that do not follow a sentence end are dropped.
pub fn breaks_from_copy(text: &str, copy: &str) -> Option<String> {
    let visible = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if visible(text) != visible(copy) {
        return None;
    }
    // Visible characters before each break in the copy
    let mut breaks = Vec::new();
    let mut count = 0;
    for c in copy.chars() {
        if c == '\n' && count > 0 {
            breaks.push(count);
        } else if !c.is_whitespace() {
            count += 1;
        }
    }

    let mut out = String::new();
    let (mut count, mut depth) = (0, 0usize);
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c.is_whitespace() {
            continue;
        }
        count += 1;
        if OPEN_QUOTES.contains(&c) {
            depth += 1;
        } else if CLOSE_QUOTES.contains(&c) {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && SENTENCE_ENDS.contains(&c) && breaks.contains(&count) && chars.peek().is_some() {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            out.push('\n');
        }
    }
    Some(out)
}

/// Number of sentences, not counting sentence ends inside 「…」.
pub fn sentence_count(text: &str) -> usize {
    text.split('\n').map(|line| split_sentences(line).len()).sum()
}

fn break_line(line: &str) -> String {
    let sentences = split_sentences(line);
    if sentences.len() < 2 {
        return line.to_owned();
    }
    sentences.join("\n")
}

/// Sentences without surrounding spaces. A sentence end inside 「…」 does not split.
fn split_sentences(line: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for c in line.chars() {
        current.push(c);
        if OPEN_QUOTES.contains(&c) {
            depth += 1;
        } else if CLOSE_QUOTES.contains(&c) {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && SENTENCE_ENDS.contains(&c) {
            sentences.push(std::mem::take(&mut current));
        }
    }
    sentences.push(current);
    sentences.into_iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_sentence_goes_on_its_own_line() {
        assert_eq!(
            break_lines("今日の進捗です。画面の修正は終わりました！テストは通りましたか？"),
            "今日の進捗です。\n画面の修正は終わりました！\nテストは通りましたか？"
        );
        assert_eq!(break_lines("資料は月曜日までに送ります。"), "資料は月曜日までに送ります。");
        assert_eq!(break_lines("末尾に句点がない文です"), "末尾に句点がない文です");
        assert_eq!(break_lines(""), "");
    }

    #[test]
    fn quoted_sentences_do_not_split() {
        assert_eq!(
            break_lines("彼は「まず、やってみよう。次に、考えよう。」と言いました。それで始めました。"),
            "彼は「まず、やってみよう。次に、考えよう。」と言いました。\nそれで始めました。"
        );
    }

    #[test]
    fn breaks_come_from_an_exact_copy_at_sentence_ends_only() {
        let text = "旅行は楽しかったです。 写真も撮りました。帰りは混んでいました。";
        assert_eq!(
            breaks_from_copy(text, "旅行は楽しかったです。写真も\n撮りました。\n帰りは混んでいました。\n"),
            Some("旅行は楽しかったです。 写真も撮りました。\n帰りは混んでいました。".to_owned())
        );
        assert_eq!(breaks_from_copy(text, "旅行は楽しかった。\n写真も撮りました。帰りは混んでいました。"), None);
        assert_eq!(
            breaks_from_copy("「はい。」と言いました。", "「はい。\n」と言いました。"),
            Some("「はい。」と言いました。".into())
        );
        assert_eq!(sentence_count(text), 3);
    }

    #[test]
    fn spaces_between_sentences_are_dropped_and_existing_breaks_kept() {
        assert_eq!(
            break_lines("報告です。 Aは完了です。 Bは途中です。\n以上です。"),
            "報告です。\nAは完了です。\nBは途中です。\n以上です。"
        );
    }
}
