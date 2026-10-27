//! Line breaks for the final text. Short dictation gets a line per sentence (`text::lines`). Long dictation
//! asks the local LLM to copy the text with breaks where the topic changes, so sentences on one topic stay
//! together; only the break positions are taken, and only from an exact copy, so the words cannot change.
//! A failed call, a changed copy or a copy without any break falls back to a line per sentence.

use std::path::Path;

use crate::ports::{CompletionRequest, LanguageModel};
use crate::rewrite::{clean_output, max_tokens_for, split_blocks, PromptStyle, BLOCK_CHARS, SPLIT_ABOVE_CHARS};
use crate::text::chars::char_len;
use crate::text::lines;

/// Text shorter than this gets a line per sentence: it is usually one message, and the LLM call would
/// add a few seconds (Qwen2.5-1.5B: about 4 s for 150 characters).
pub const LLM_ABOVE_CHARS: usize = 200;

const SYSTEM: &str = "あなたは文章を読みやすく改行する編集者です。与えられた文章を一字も変えずにそのまま書き写し、話題や用件が変わるところだけで改行してください。同じ話題が続くあいだは改行しません。";

const EXAMPLES: &[(&str, &str)] = &[
    (
        "おはようございます。本日の打ち合わせの資料を共有フォルダに入れました。前回いただいたご意見を反映して、3ページ目の図を差し替えています。別件ですが、来月の展示会の申し込みは今週の金曜日が締め切りです。参加される方は、私までご連絡ください。どうぞよろしくお願いします。",
        "おはようございます。\n本日の打ち合わせの資料を共有フォルダに入れました。前回いただいたご意見を反映して、3ページ目の図を差し替えています。\n別件ですが、来月の展示会の申し込みは今週の金曜日が締め切りです。参加される方は、私までご連絡ください。\nどうぞよろしくお願いします。",
    ),
    (
        "町内会の掃除は今月の第2日曜日に決まりました。集合は朝8時に公民館の前です。軍手とゴミ袋はこちらで用意します。話は変わりますが、夏祭りの手伝いをしてくれる人を探しています。屋台の準備と片付けをお願いする予定です。興味がある方は声をかけてください。",
        "町内会の掃除は今月の第2日曜日に決まりました。集合は朝8時に公民館の前です。軍手とゴミ袋はこちらで用意します。\n話は変わりますが、夏祭りの手伝いをしてくれる人を探しています。屋台の準備と片付けをお願いする予定です。興味がある方は声をかけてください。",
    ),
    (
        "新しいスマートフォンに替えました。画面が大きくて文字が読みやすいです。電池の持ちも前より良くなりました。",
        "新しいスマートフォンに替えました。画面が大きくて文字が読みやすいです。電池の持ちも前より良くなりました。",
    ),
];

/// Breaks `text` into lines, with the LLM when one is given and the text is long.
pub async fn break_lines(llm: Option<(&dyn LanguageModel, &Path)>, text: &str) -> String {
    // On 10 multi-sentence cases (18 wanted breaks, measured with a 100-character threshold) the rules found
    // 4 with none wrong, Qwen3.5-0.8B 7 with none wrong, and Qwen2.5-1.5B 11 with 5 wrong. More breaks were
    // preferred over none, so every model is used
    let Some((llm, model)) = llm.filter(|_| char_len(text) >= LLM_ABOVE_CHARS) else {
        return lines::break_lines(text);
    };
    let blocks =
        if char_len(text) > SPLIT_ABOVE_CHARS { split_blocks(text, BLOCK_CHARS) } else { vec![text.to_owned()] };
    let mut out = Vec::with_capacity(blocks.len());
    for block in &blocks {
        let broken = match break_block(llm, model, block).await {
            Some(broken) => broken,
            None => lines::break_lines(block),
        };
        out.push(broken.trim().to_owned());
    }
    // Blocks end at sentence ends
    out.join("\n")
}

async fn break_block(llm: &dyn LanguageModel, model: &Path, block: &str) -> Option<String> {
    if lines::sentence_count(block) < 3 {
        return None;
    }
    let prompt = prompt_for(PromptStyle::for_model(model), block);
    // The copy needs room for every character plus the breaks
    let request = CompletionRequest { model, prompt: &prompt, max_tokens: max_tokens_for(block) };
    let raw = match llm.complete(request).await {
        Ok(raw) => raw,
        Err(e) => {
            tracing::info!(error = %e, "LLM line breaks failed; using a line per sentence");
            return None;
        }
    };
    let Some(broken) = lines::breaks_from_copy(block, &clean_output(&raw)) else {
        tracing::info!(chars = char_len(block), "LLM copy changed the text; using a line per sentence");
        return None;
    };
    // Small models often copy long text without any break; one unbroken run is what we are avoiding
    if !broken.contains('\n') {
        tracing::info!(chars = char_len(block), "LLM placed no line break; using a line per sentence");
        return None;
    }
    Some(broken)
}

/// ChatML prompt, like the clean-up prompt.
pub fn prompt_for(style: PromptStyle, text: &str) -> String {
    let mut prompt = format!("<|im_start|>system\n{SYSTEM}<|im_end|>\n");
    for (input, output) in EXAMPLES {
        prompt.push_str(&format!("<|im_start|>user\n{input}<|im_end|>\n<|im_start|>assistant\n{output}<|im_end|>\n"));
    }
    prompt.push_str(&format!("<|im_start|>user\n{text}<|im_end|>\n<|im_start|>assistant\n"));
    if style == PromptStyle::ChatMlNoThinking {
        prompt.push_str("<think>\n\n</think>\n\n");
    }
    prompt
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::ports::EngineError;

    struct FixedLlm(Result<String, EngineError>);

    #[async_trait]
    impl LanguageModel for FixedLlm {
        async fn complete(&self, _: CompletionRequest<'_>) -> Result<String, EngineError> {
            self.0.clone()
        }
    }

    const QWEN3: &str = "Qwen3.5-0.8B-Q4_K_M.gguf";
    const LONG: &str = "今日は朝から雨でした。駅まで歩くあいだに靴がぬれてしまいました。会社に着いてから乾かしました。まず、午後の会議の資料を作りました。";

    fn long_text() -> String {
        LONG.repeat(4)
    }

    #[tokio::test]
    async fn long_text_takes_breaks_from_an_exact_copy() {
        let text = long_text();
        let copy = text.replace("でした。駅", "でした。\n駅");
        let llm = FixedLlm(Ok(format!("{copy}[end of text]")));
        let out = break_lines(Some((&llm, Path::new(QWEN3))), &text).await;
        assert_eq!(out, copy);
    }

    #[tokio::test]
    async fn changed_copy_or_failure_falls_back_to_rules() {
        let text = long_text();
        let rules = lines::break_lines(&text);
        assert_eq!(rules.lines().count(), lines::sentence_count(&text));

        let changed = FixedLlm(Ok(text.replace("ぬれて", "濡れて")));
        assert_eq!(break_lines(Some((&changed, Path::new(QWEN3))), &text).await, rules);
        let unbroken = FixedLlm(Ok(text.clone()));
        assert_eq!(break_lines(Some((&unbroken, Path::new(QWEN3))), &text).await, rules);
        let failing = FixedLlm(Err(EngineError::TimedOut));
        assert_eq!(break_lines(Some((&failing, Path::new(QWEN3))), &text).await, rules);
    }

    struct Unused;

    #[async_trait]
    impl LanguageModel for Unused {
        async fn complete(&self, _: CompletionRequest<'_>) -> Result<String, EngineError> {
            panic!("the LLM must not be called")
        }
    }

    #[tokio::test]
    async fn short_text_uses_rules_without_the_llm() {
        let text = "報告です。まず、Aは完了です。次に、Bは途中です。";
        assert_eq!(break_lines(Some((&Unused, Path::new(QWEN3))), text).await, lines::break_lines(text));
        assert_eq!(break_lines(None, &long_text()).await, lines::break_lines(&long_text()));
    }

    #[tokio::test]
    async fn qwen2_models_place_breaks_too() {
        let text = long_text();
        let copy = text.replace("でした。駅", "でした。\n駅");
        let llm = FixedLlm(Ok(copy.clone()));
        let model = Path::new("qwen2.5-1.5b-instruct-q4_k_m.gguf");
        assert_eq!(break_lines(Some((&llm, model)), &text).await, copy);
    }
}
