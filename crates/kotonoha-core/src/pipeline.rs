//! Turns chunk transcripts into the final text:
//! merge overlaps -> dictionary spellings -> fillers -> self-corrections -> punctuation
//! -> optional LLM clean-up (edit guard per block) -> integrity check against the transcript
//! -> optional line breaks (rules, or the LLM for long text). If the LLM fails or changes something it must
//! not, the rule-based text is used.

use std::path::Path;

use crate::dictionary::Dictionary;
use crate::linebreak;
use crate::ports::{EngineError, LanguageModel};
use crate::rewrite::rewrite_guarded;
use crate::text::integrity::{self, IntegrityFailure};
use crate::text::{correction, filler, overlap, punctuation, vocabulary};

/// Whether and how to run the LLM step.
pub enum LlmPlan<'a> {
    Use { llm: &'a dyn LanguageModel, model: &'a Path },
    Skip(SkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    LowMemory,
    NoModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmOutcome {
    Skipped(SkipReason),
    /// The LLM text is used. Rejected blocks kept their rule-based text.
    Applied {
        blocks: usize,
        rejected_blocks: usize,
    },
    Failed(EngineError),
    RejectedByIntegrity(IntegrityFailure),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Processed {
    pub final_text: String,
    /// Merged transcript with dictionary spellings, before any other rule.
    pub transcript: String,
    pub rule_text: String,
    pub llm: LlmOutcome,
}

pub struct PipelineOptions<'a> {
    pub dictionary: &'a Dictionary,
    pub remove_fillers: bool,
    /// Start a new line after each sentence, or per topic in long text.
    pub line_breaks: bool,
}

pub async fn process(transcripts: &[String], options: PipelineOptions<'_>, plan: LlmPlan<'_>) -> Processed {
    let replacements = options.dictionary.replacements();
    let transcript = vocabulary::normalize(&overlap::merge_all(transcripts), &replacements).trim().to_owned();
    if transcript.is_empty() {
        return Processed {
            final_text: String::new(),
            transcript,
            rule_text: String::new(),
            llm: LlmOutcome::Skipped(SkipReason::Disabled),
        };
    }

    let rule_text = apply_rules(&transcript, options.remove_fillers);
    let (final_text, llm, break_llm) = match plan {
        LlmPlan::Skip(reason) => (rule_text.clone(), LlmOutcome::Skipped(reason), None),
        LlmPlan::Use { llm, model } => {
            let (text, outcome) = rewrite(llm, model, &transcript, &rule_text, &options).await;
            // A model that failed to run is not tried again for line breaks
            let usable = !matches!(outcome, LlmOutcome::Failed(_));
            (text, outcome, usable.then_some((llm, model)))
        }
    };
    let final_text =
        if options.line_breaks { linebreak::break_lines(break_llm, &final_text).await } else { final_text };
    Processed { final_text, transcript, rule_text, llm }
}

async fn rewrite(
    llm: &dyn LanguageModel,
    model: &Path,
    transcript: &str,
    rule_text: &str,
    options: &PipelineOptions<'_>,
) -> (String, LlmOutcome) {
    let rewritten = match rewrite_guarded(llm, model, rule_text).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "LLM rewrite failed; using rule-based text");
            return (rule_text.to_owned(), LlmOutcome::Failed(e));
        }
    };
    let candidate = vocabulary::normalize(&rewritten.text, &options.dictionary.replacements());

    match integrity::validate(transcript, &candidate, &options.dictionary.protected_words()) {
        Ok(repaired) => (
            punctuation::finish_sentence(&repaired),
            LlmOutcome::Applied { blocks: rewritten.blocks, rejected_blocks: rewritten.rejections.len() },
        ),
        Err(failure) => {
            tracing::warn!(?failure, "integrity check rejected LLM output; using rule-based text");
            (rule_text.to_owned(), LlmOutcome::RejectedByIntegrity(failure))
        }
    }
}

pub fn apply_rules(text: &str, remove_fillers: bool) -> String {
    let text = if remove_fillers { filler::remove_fillers(text) } else { text.to_owned() };
    punctuation::finish_sentence(&correction::apply_corrections(&text))
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::ports::CompletionRequest;

    struct FixedLlm(Result<String, EngineError>);

    #[async_trait]
    impl LanguageModel for FixedLlm {
        async fn complete(&self, _: CompletionRequest<'_>) -> Result<String, EngineError> {
            self.0.clone()
        }
    }

    fn options(dict: &Dictionary) -> PipelineOptions<'_> {
        PipelineOptions { dictionary: dict, remove_fillers: true, line_breaks: false }
    }

    #[tokio::test]
    async fn rule_based_when_llm_is_skipped() {
        let dict = Dictionary::builtin();
        let out = process(
            &["うーん、駐輪場は、えっと、建物の裏です".into()],
            options(&dict),
            LlmPlan::Skip(SkipReason::LowMemory),
        )
        .await;
        assert_eq!(out.final_text, "駐輪場は、建物の裏です。");
        assert_eq!(out.llm, LlmOutcome::Skipped(SkipReason::LowMemory));
    }

    #[tokio::test]
    async fn uses_llm_text_that_passes_the_guards() {
        let dict = Dictionary::builtin();
        let llm = FixedLlm(Ok("入場料は大人800円です。[end of text]".into()));
        let out = process(
            &["入場料は、まあ、大人800円です".into()],
            options(&dict),
            LlmPlan::Use { llm: &llm, model: Path::new("m") },
        )
        .await;
        assert_eq!(out.final_text, "入場料は大人800円です。");
        assert_eq!(out.llm, LlmOutcome::Applied { blocks: 1, rejected_blocks: 0 });
    }

    #[tokio::test]
    async fn falls_back_when_llm_fails_or_changes_numbers() {
        let dict = Dictionary::builtin();
        let failing = FixedLlm(Err(EngineError::TimedOut));
        let out = process(
            &["送料は1200円です".into()],
            options(&dict),
            LlmPlan::Use { llm: &failing, model: Path::new("m") },
        )
        .await;
        assert_eq!(out.final_text, "送料は1200円です。");
        assert_eq!(out.llm, LlmOutcome::Failed(EngineError::TimedOut));

        // Passes the edit guard (only 2 characters differ) but changes an amount
        let tampering = FixedLlm(Ok("送料は1300円です。".into()));
        let out = process(
            &["送料は1200円です".into()],
            options(&dict),
            LlmPlan::Use { llm: &tampering, model: Path::new("m") },
        )
        .await;
        assert_eq!(out.final_text, "送料は1200円です。");
        assert!(matches!(out.llm, LlmOutcome::RejectedByIntegrity(_)));
    }

    #[tokio::test]
    async fn dictionary_spellings_apply_to_transcript() {
        let dict = Dictionary::builtin();
        let out = process(&["スラックで送ります".into()], options(&dict), LlmPlan::Skip(SkipReason::Disabled)).await;
        assert_eq!(out.final_text, "Slackで送ります。");
    }

    #[tokio::test]
    async fn line_breaks_apply_with_and_without_llm() {
        let dict = Dictionary::default();
        let transcript = vec!["報告です。まず、Aは完了です。えー、次に、Bは途中です".to_owned()];
        let expected = "報告です。\nまず、Aは完了です。\n次に、Bは途中です。";
        let with_breaks = PipelineOptions { line_breaks: true, ..options(&dict) };
        let out = process(&transcript, with_breaks, LlmPlan::Skip(SkipReason::Disabled)).await;
        assert_eq!(out.final_text, expected);
        assert!(!out.rule_text.contains('\n'));

        let llm = FixedLlm(Ok("報告です。まず、Aは完了です。次に、Bは途中です。".into()));
        let with_breaks = PipelineOptions { line_breaks: true, ..options(&dict) };
        let out = process(&transcript, with_breaks, LlmPlan::Use { llm: &llm, model: Path::new("m") }).await;
        assert_eq!(out.final_text, expected);

        let out = process(&transcript, options(&dict), LlmPlan::Skip(SkipReason::Disabled)).await;
        assert!(!out.final_text.contains('\n'));
    }

    #[tokio::test]
    async fn empty_transcript() {
        let dict = Dictionary::default();
        let out = process(&[String::new(), "  ".into()], options(&dict), LlmPlan::Skip(SkipReason::Disabled)).await;
        assert!(out.final_text.is_empty());
    }
}
