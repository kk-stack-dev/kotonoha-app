//! LLM clean-up of the rule-processed text, block by block, keeping only edits that pass the edit guard.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::ports::{CompletionRequest, EngineError, LanguageModel};
use crate::text::chars::char_len;
use crate::text::edit_guard::{self, Rejection};

/// The model is told it may only delete fillers, keep the second half of a self-correction and fix
/// punctuation; without that limit Qwen2.5-1.5B turned casual sentence endings into formal ones.
/// Each family has its own wording: Qwen3.5-0.8B kept more mid-sentence fillers with this one, and
/// Qwen2.5-1.5B did slightly worse with the other.
const SYSTEM_QWEN2: &str = "あなたは音声入力で書き起こした文章を整える編集者です。話し手の単語や文末、言い回しは変えないでください。行ってよいのは次の3つだけです。
1. 「えー」「えっと」「あの」「まあ」「なんか」のようなつなぎ言葉を取り除く
2. 言い直した部分は、言い直した後の言葉だけを残す(例:「木曜日、いや金曜日」→「金曜日」)
3. 句読点を正しく打つ
";

const SYSTEM_QWEN3: &str = "あなたは音声入力の書き起こしを清書する係です。話し手が使った語、文末、言い回しは残したまま、次の3つのことだけをします。
1. 「えー」「えっと」「あの」「まあ」「なんか」などのつなぎ言葉を削る
2. 言い直しは、言い直した後の表現だけにする(例:「木曜日、いや金曜日」→「金曜日」)
3. 句読点を直す
";

const RULES_LOOSE: &str = "つなぎ言葉は、読点で区切られていなくても、文の途中にあっても取り除きます。別の言葉への置き換え、要約、文末や敬語の変更、内容の付け足しは一切してはいけません。数字、日付、時刻、URL、メールアドレス、人名、製品名、会社名などの固有名詞は、一文字も変えずに残します。出力するのは整えた後の文章だけです。";

/// Lists the kinds of words that must stay. With `kotonoha bench llm` it lowered Qwen2.5-1.5B's error
/// rate, but made Qwen3.5 keep fillers it should drop, so only Qwen2.5 gets it.
const RULES_STRICT: &str = "取り除いてよいのはつなぎ言葉だけです。「それでは」「じゃあ」のような接続の言葉、「ですね」のような話し言葉、人名、製品名、会社名、曜日、日付、時刻は必ず残してください。別の言葉への置き換え、要約、文末や敬語の変更、内容の付け足しはしないでください。数字、URL、メールアドレス、英字は元の表記のまま残してください。出力は整えた文章だけにしてください。";

/// Example turns: the speaker's own words come back unchanged, minus the fillers.
const EXAMPLES_BASE: &[(&str, &str)] = &[
    (
        "えっと、駅前の、あー、パン屋さんなんですけど、まあ、予約は電話でお願いします",
        "駅前のパン屋さんなんですけど、予約は電話でお願いします。",
    ),
    ("打ち合わせは木曜日、いや金曜日にします", "打ち合わせは金曜日にします。"),
    (
        "このあいだはですね、なんか、気になるところが一つあって、なので先に聞いておきたいです",
        "このあいだはですね、気になるところが一つあって、なので先に聞いておきたいです。",
    ),
];

const EXAMPLE_KEEP_WORDS: (&str, &str) = (
    "じゃあ、えー、Slackで山田さんに、あの、金曜日の17時までに連絡しておきます",
    "じゃあ、Slackで山田さんに金曜日の17時までに連絡しておきます。",
);

/// Text longer than this is rewritten in blocks of `BLOCK_CHARS`.
pub(crate) const SPLIT_ABOVE_CHARS: usize = 1_200;
pub(crate) const BLOCK_CHARS: usize = 500;

/// Prompt format and wording for a model family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStyle {
    /// Qwen2.5 and other ChatML models: strict wording.
    ChatMl,
    /// Qwen3 / Qwen3.5: ChatML with an empty thinking block, which switches reasoning off.
    ChatMlNoThinking,
}

impl PromptStyle {
    /// Chosen from the model file name, e.g. "Qwen3.5-0.8B-Q4_K_M.gguf".
    pub fn for_model(path: &Path) -> Self {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name.contains("qwen3") {
            Self::ChatMlNoThinking
        } else {
            Self::ChatMl
        }
    }

    fn system(self) -> String {
        let (base, rules) = match self {
            Self::ChatMl => (SYSTEM_QWEN2, RULES_STRICT),
            Self::ChatMlNoThinking => (SYSTEM_QWEN3, RULES_LOOSE),
        };
        format!("{base}{rules}")
    }

    fn examples(self) -> Vec<(&'static str, &'static str)> {
        let mut examples = EXAMPLES_BASE.to_vec();
        if self == Self::ChatMl {
            examples.push(EXAMPLE_KEEP_WORDS);
        }
        examples
    }
}

/// ChatML prompt (Qwen), passed as a raw completion prompt.
pub fn prompt_for(style: PromptStyle, text: &str) -> String {
    let mut prompt = format!("<|im_start|>system\n{}<|im_end|>\n", style.system());
    for (input, output) in style.examples() {
        prompt.push_str(&format!("<|im_start|>user\n{input}<|im_end|>\n<|im_start|>assistant\n{output}<|im_end|>\n"));
    }
    prompt.push_str(&format!("<|im_start|>user\n{text}<|im_end|>\n<|im_start|>assistant\n"));
    if style == PromptStyle::ChatMlNoThinking {
        prompt.push_str("<think>\n\n</think>\n\n");
    }
    prompt
}

/// A Japanese character is roughly one token, and the answer adds punctuation, so twice the input length.
pub fn max_tokens_for(text: &str) -> usize {
    (char_len(text) * 2).clamp(128, 2_048)
}

/// Cuts text into blocks of at most `max_chars`. A block ends at a sentence end where possible; a sentence
/// longer than a block is cut at its last comma, and a run without any comma at `max_chars`.
pub fn split_blocks(text: &str, max_chars: usize) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if "。！？!?\n".contains(c) {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    let mut pieces = Vec::new();
    for sentence in sentences {
        let mut rest: Vec<char> = sentence.chars().collect();
        while rest.len() > max_chars {
            let cut = rest[..max_chars].iter().rposition(|&c| c == '、' || c == ',').map_or(max_chars, |i| i + 1);
            pieces.push(rest.drain(..cut).collect::<String>());
        }
        if !rest.is_empty() {
            pieces.push(rest.into_iter().collect());
        }
    }

    let mut blocks = Vec::new();
    let mut block = String::new();
    for piece in pieces {
        if !block.is_empty() && char_len(&block) + char_len(&piece) > max_chars {
            blocks.push(std::mem::take(&mut block));
        }
        block.push_str(&piece);
    }
    if !block.trim().is_empty() {
        blocks.push(block);
    }
    blocks
}

/// A label a small model sometimes puts before its answer: "出力：", or a short phrase naming the edited
/// text ("以下は整文後の文章です："). It must name the edit, so speech that starts "結果：" is kept.
static ANSWER_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(?:以下は)?(?:整文|清書|整形|修正|完成)[^\n：:]{0,10}|出力)[：:]\s*").unwrap());

/// Removes what is not the answer: end-of-text and ChatML tokens, reasoning, and an answer label.
pub fn clean_output(raw: &str) -> String {
    const SPECIAL_TOKENS: &[&str] = &["<|im_start|>", "<|im_end|>", "<|endoftext|>", "[end of text]"];
    let text = strip_thinking(raw);
    let text = SPECIAL_TOKENS.iter().fold(text, |t, m| t.replace(m, ""));
    ANSWER_LABEL.replace(text.trim(), "").trim().to_owned()
}

/// Removes spaces the model put between Japanese and Latin text ("14 時", "Claude で") when the speaker's
/// text had none there. Qwen3.5 adds them; Japanese writing normally does not use them.
pub fn drop_inserted_spaces(input: &str, output: &str) -> String {
    let chars: Vec<char> = output.chars().collect();
    let mut kept = String::with_capacity(output.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 && i + 1 < chars.len() {
            let (before, after) = (chars[i - 1], chars[i + 1]);
            let touches_japanese = !before.is_ascii() || !after.is_ascii();
            if touches_japanese && !input.contains(&format!("{before} {after}")) {
                continue;
            }
        }
        kept.push(c);
    }
    kept
}

/// Removes `<think>...</think>` reasoning that a model may emit despite the empty thinking block.
fn strip_thinking(raw: &str) -> String {
    match (raw.find("<think>"), raw.find("</think>")) {
        (Some(start), Some(end)) if start < end => format!("{}{}", &raw[..start], &raw[end + "</think>".len()..]),
        (None, Some(end)) => raw[end + "</think>".len()..].to_owned(),
        _ => raw.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardedRewrite {
    pub text: String,
    pub blocks: usize,
    /// Why each rejected block was kept as its input.
    pub rejections: Vec<Rejection>,
}

/// Rewrites `text` block by block. A block whose output paraphrases, invents or drops words keeps its input.
pub async fn rewrite_guarded(llm: &dyn LanguageModel, model: &Path, text: &str) -> Result<GuardedRewrite, EngineError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(GuardedRewrite { text: String::new(), blocks: 0, rejections: Vec::new() });
    }
    let blocks =
        if char_len(text) > SPLIT_ABOVE_CHARS { split_blocks(text, BLOCK_CHARS) } else { vec![text.to_owned()] };

    let mut results = Vec::with_capacity(blocks.len());
    let mut rejections = Vec::new();
    let style = PromptStyle::for_model(model);
    for block in &blocks {
        let prompt = prompt_for(style, block);
        let raw = llm.complete(CompletionRequest { model, prompt: &prompt, max_tokens: max_tokens_for(block) }).await?;
        let candidate = drop_inserted_spaces(block, &clean_output(&raw));
        if candidate.is_empty() {
            return Err(EngineError::Failed("empty LLM response".into()));
        }
        let verdict = edit_guard::check(block, &candidate);
        match verdict.rejection {
            None => results.push(candidate),
            Some(rejection) => {
                tracing::info!(
                    ?rejection,
                    added = verdict.added_ratio,
                    kept = verdict.kept_ratio,
                    "LLM block rejected"
                );
                rejections.push(rejection);
                results.push(block.clone());
            }
        }
    }
    // Blocks end at sentence ends, so they join without a separator; line breaks are a later step
    Ok(GuardedRewrite { text: results.concat(), blocks: blocks.len(), rejections })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_tokens_and_answer_labels() {
        assert_eq!(clean_output("駐車場は地下にあります。 [end of text]\n\n"), "駐車場は地下にあります。");
        assert_eq!(clean_output("清書した文章：傘を持ってきてください。<|im_end|>"), "傘を持ってきてください。");
        assert_eq!(clean_output("以下は整形後の文です: 了解です。"), "了解です。");
        assert_eq!(clean_output("出力：今日は晴れです。"), "今日は晴れです。");
        assert_eq!(clean_output("結果：3対1で勝ちました。"), "結果：3対1で勝ちました。");
    }

    #[test]
    fn splits_unpunctuated_text_into_bounded_blocks() {
        let text = "さしすせそたちつてと".repeat(120);
        let blocks = split_blocks(&text, 500);
        assert!(blocks.len() >= 3);
        assert!(blocks.iter().all(|b| char_len(b) <= 500));
        assert_eq!(blocks.concat(), text);
    }

    #[test]
    fn splits_at_sentence_ends() {
        let text = format!("{}。{}。", "う".repeat(320), "え".repeat(280));
        let blocks = split_blocks(&text, 500);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].ends_with('。'));
    }

    #[test]
    fn qwen3_prompts_switch_thinking_off_and_reasoning_is_stripped() {
        let style = PromptStyle::for_model(Path::new("/m/Qwen3.5-0.8B-Q4_K_M.gguf"));
        assert_eq!(style, PromptStyle::ChatMlNoThinking);
        assert!(prompt_for(style, "テスト").ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        assert_eq!(PromptStyle::for_model(Path::new("qwen2.5-1.5b-instruct-q4_k_m.gguf")), PromptStyle::ChatMl);
        assert_eq!(clean_output("<think>\n考え中\n</think>\n\nこんにちは。"), "こんにちは。");
        assert_eq!(clean_output("考え中</think>こんにちは。"), "こんにちは。");
    }

    #[test]
    fn spaces_added_around_latin_text_are_removed() {
        assert_eq!(drop_inserted_spaces("14時にClaudeで", "14 時に Claude で"), "14時にClaudeで");
        assert_eq!(drop_inserted_spaces("Claude Codeで", "Claude Code で"), "Claude Codeで");
        assert_eq!(drop_inserted_spaces("A と B", "A と B"), "A と B");
    }

    #[test]
    fn prompt_ends_with_assistant_turn() {
        let p = prompt_for(PromptStyle::ChatMl, "テスト");
        assert!(
            p.starts_with("<|im_start|>system\n")
                && p.ends_with("<|im_start|>user\nテスト<|im_end|>\n<|im_start|>assistant\n")
        );
    }
}
