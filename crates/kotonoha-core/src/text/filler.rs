//! Rule-based removal of hesitations ("えっと", "うーん"). A filler goes only when it is set off on both
//! sides (text edge, punctuation or a space), so "あの人" or "まあまあ" stay as they are. Fillers inside a
//! phrase are left to the LLM step.

use std::sync::LazyLock;

use regex::Regex;

use super::spoken::{longest_first, standalone_hesitations};

/// One rule per hesitation, longest spelling first.
static HESITATION_RULES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    longest_first(standalone_hesitations())
        .into_iter()
        .map(|h| Regex::new(&format!(r"(^|[、。\n\s]){}([、\s]+|$)", regex::escape(h))).unwrap())
        .collect()
});
/// "あの" and "なんか" count as fillers only when a comma follows: "あの、" and "なんか、思ったより" go,
/// "あの店" and "なんか変" stay.
static BEFORE_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[、。\n\s])(?:あの|なんか)、").unwrap());
/// "まあ、" goes the same way, unless the text says "まあまあ".
static MAA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[、。\n\s])まあ、").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());

pub fn remove_fillers(text: &str) -> String {
    // An utterance that is nothing but a hesitation leaves nothing to insert
    let bare = text.trim().trim_end_matches(['。', '、']);
    if standalone_hesitations().any(|h| h == bare) {
        return String::new();
    }
    let mut out = text.to_owned();
    for rule in HESITATION_RULES.iter() {
        out = rule.replace_all(&out, "$1").into_owned();
    }
    out = BEFORE_COMMA.replace_all(&out, "$1").into_owned();
    if !out.contains("まあまあ") {
        out = MAA.replace_all(&out, "$1").into_owned();
    }
    tidy(&out)
}

/// Removing a filler can leave "、、", a leading comma or doubled spaces behind.
fn tidy(text: &str) -> String {
    let mut out = text.to_owned();
    while out.contains("、、") {
        out = out.replace("、、", "、");
    }
    let out = out.trim_start_matches(['、', '。']);
    SPACES.replace_all(out, " ").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hesitations_set_off_by_commas_go() {
        assert_eq!(remove_fillers("うーん、図書館は、えっと、月曜日が休みです。"), "図書館は、月曜日が休みです。");
        assert_eq!(remove_fillers("ええっと 鍵は玄関の棚にあります"), "鍵は玄関の棚にあります");
    }

    #[test]
    fn ordinary_words_stay() {
        let text = "あの店のパンは、まあまあおいしかったです。";
        assert_eq!(remove_fillers(text), text);
        assert_eq!(remove_fillers("ええ、そう思います"), "ええ、そう思います");
    }

    #[test]
    fn a_lone_hesitation_becomes_empty() {
        assert_eq!(remove_fillers(" うーん。 "), "");
        assert_eq!(remove_fillers("えーと"), "");
    }

    #[test]
    fn nanka_only_before_a_comma() {
        assert_eq!(remove_fillers("なんか、雨が強くなってきました"), "雨が強くなってきました");
        assert_eq!(remove_fillers("なんか焦げたにおいがします"), "なんか焦げたにおいがします");
    }

    #[test]
    fn ano_and_maa_before_a_comma() {
        assert_eq!(remove_fillers("まあ、あの、大丈夫だと思います"), "大丈夫だと思います");
    }
}
