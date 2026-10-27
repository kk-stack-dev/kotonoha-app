//! Model comparisons on fixed Japanese cases, using the app's own dictionary and text pipeline.
//!
//! - `bench asr`: macOS `say` voices read sentences with product names; each Whisper model transcribes
//!   them with the dictionary prompt and spelling fixes. Reports CER, exact product names and time.
//! - `bench llm`: transcripts with fillers and self-corrections go through each LLM. Reports how close
//!   the model's own output is to the expected text, how often the edit guard accepts it, and time.
//!
//! Peak memory is measured with `/usr/bin/time -l` on macOS. Nothing leaves the machine.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use kotonoha_core::dictionary::{Dictionary, DEFAULT_PROMPT_BUDGET};
use kotonoha_core::pipeline;
use kotonoha_core::ports::{CompletionRequest, ConfigSource, LanguageModel, SpeechRecognizer, TranscriptionRequest};
use kotonoha_core::rewrite::{clean_output, drop_inserted_spaces, max_tokens_for, prompt_for, PromptStyle};
use kotonoha_core::text::{edit_guard, transcript, vocabulary};
use kotonoha_platform::llama::LlamaCli;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::tools::{self, Tool};
use kotonoha_platform::whisper::WhisperCli;
use unicode_normalization::UnicodeNormalization;

/// (read aloud, expected text, product names that must be exact).
const ASR_CASES: &[(&str, &str, &[&str])] = &[
    (
        "クロードコードにリファクタリングを頼んだら、30分で終わりました。",
        "Claude Codeにリファクタリングを頼んだら、30分で終わりました。",
        &["Claude Code"],
    ),
    (
        "チャットジーピーティーとジェミニに同じ質問をして、答えを比べました。",
        "ChatGPTとGeminiに同じ質問をして、答えを比べました。",
        &["ChatGPT", "Gemini"],
    ),
    (
        "ノートブックエルエムに会議の録音を入れて、要点をまとめました。",
        "NotebookLMに会議の録音を入れて、要点をまとめました。",
        &["NotebookLM"],
    ),
    (
        "コーデックスが作ったプルリクエストを、ギットハブで確認してください。",
        "Codexが作ったプルリクエストを、GitHubで確認してください。",
        &["Codex", "GitHub"],
    ),
    (
        "グーグルワークスペースのカレンダーに、来月の予定を入れました。",
        "Google Workspaceのカレンダーに、来月の予定を入れました。",
        &["Google Workspace"],
    ),
    (
        "スラックの通知が多いので、クロードに整理の仕方を聞きました。",
        "Slackの通知が多いので、Claudeに整理の仕方を聞きました。",
        &["Slack", "Claude"],
    ),
    (
        "請求書は12月5日までに、経理の佐藤さんへ送ってください。",
        "請求書は12月5日までに、経理の佐藤さんへ送ってください。",
        &[],
    ),
];

/// (transcript as Whisper writes it, expected clean text).
const LLM_CASES: &[(&str, &str)] = &[
    (
        "えーっと、明日の朝礼なんですけど、あの、8時半からに早まりました",
        "明日の朝礼なんですけど、8時半からに早まりました。",
    ),
    (
        "Codexは上限に達したので、残りの作業はClaudeで進める予定です",
        "Codexは上限に達したので、残りの作業はClaudeで進める予定です。",
    ),
    ("請求書は水曜日、いや木曜日までに鈴木さんに渡してください", "請求書は木曜日までに鈴木さんに渡してください。"),
    (
        "まあ、なんか、Claude Codeで直したら、えー、動くようになりました",
        "Claude Codeで直したら、動くようになりました。",
    ),
    ("料金は年額2万4000円で、えっと、4月1日から切り替わります", "料金は年額2万4000円で、4月1日から切り替わります。"),
    (
        "この前ですね、あの、新しいキーボードを買ってみたんですけど、打ちやすくなった気がします",
        "この前ですね、新しいキーボードを買ってみたんですけど、打ちやすくなった気がします。",
    ),
    ("それでは、うーん、来週の打ち合わせで詳しく説明します", "それでは、来週の打ち合わせで詳しく説明します。"),
    ("GeminiとNotebookLMで、あの、議事録をまとめておきました", "GeminiとNotebookLMで議事録をまとめておきました。"),
    ("その件は、えー、山田さんじゃなくて佐藤さんに確認してください", "その件は佐藤さんに確認してください。"),
    (
        "なんか、思ったより、あの、時間がかかりそうなので、明日もう一度連絡します",
        "思ったより時間がかかりそうなので、明日もう一度連絡します。",
    ),
    (
        "えっと、ファイル名はreport_2026.xlsxで、共有フォルダに置いてあります",
        "ファイル名はreport_2026.xlsxで、共有フォルダに置いてあります。",
    ),
    ("来月の発表は、あー、15日じゃなくて22日の午後2時からです", "来月の発表は22日の午後2時からです。"),
    // Below: fillers and corrections without commas, which the rules cannot catch
    ("えーと来週の会議ですけどあの資料は山田さんが用意します", "来週の会議ですけど、資料は山田さんが用意します。"),
    ("あのーその件についてはえっと明日回答します", "その件については明日回答します。"),
    ("なんかうまく動かないのでちょっと見てもらえますか", "うまく動かないので、ちょっと見てもらえますか。"),
    ("えっと明日の朝9時にえー東京駅で待ち合わせしましょう", "明日の朝9時に東京駅で待ち合わせしましょう。"),
    ("請求書は今月末いや来月の5日までに送ります", "請求書は来月の5日までに送ります。"),
    ("うーんそうですねこの案で進めましょう", "そうですね、この案で進めましょう。"),
    (
        "あのGoogle Workspaceの管理画面からえーユーザーを追加してください",
        "Google Workspaceの管理画面からユーザーを追加してください。",
    ),
    ("まあ正直なところ予算は50万円くらいを考えています", "正直なところ、予算は50万円くらいを考えています。"),
];

pub async fn asr(store: &ConfigStore, models: &[PathBuf], voices: &str, show: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("bench asr synthesises speech with the macOS `say` command");
    }
    let dictionary = store.dictionary();
    let prompt = dictionary.whisper_prompt(DEFAULT_PROMPT_BUDGET);
    let replacements = dictionary.replacements();
    let whisper = WhisperCli::new(store.settings().tools.whisper, Some(store.dirs().tools_dir()));
    let dir = tempfile::tempdir()?;

    let mut clips = Vec::new();
    for (vi, voice) in voices.split(',').filter(|v| !v.is_empty()).enumerate() {
        for (ci, (spoken, _, _)) in ASR_CASES.iter().enumerate() {
            let wav = dir.path().join(format!("v{vi}c{ci}.wav"));
            let status = Command::new("say")
                .args(["-v", voice, "--file-format=WAVE", "--data-format=LEI16@16000", "-o"])
                .arg(&wav)
                .arg(spoken)
                .status()?;
            if !status.success() {
                bail!("say failed for voice {voice}");
            }
            clips.push((ci, crate::run::read_wav_16k(&wav)?, wav));
        }
    }
    println!("{} clips ({} voices x {} sentences)\n", clips.len(), clips.len() / ASR_CASES.len(), ASR_CASES.len());
    println!("{:<40} {:>7} {:>9} {:>9} {:>9}", "model", "CER", "names", "avg time", "peak RAM");

    let tool = tools::locate(Tool::Whisper, store.settings().tools.whisper.as_deref(), Some(&store.dirs().tools_dir()))
        .context("whisper-cli not found")?;
    for model in models {
        let (mut errors, mut chars, mut names_ok, mut names, mut seconds) = (0.0, 0.0, 0, 0, 0.0);
        for (ci, samples, _) in &clips {
            let (_, expected, terms) = ASR_CASES[*ci];
            let started = Instant::now();
            let raw =
                whisper.transcribe(TranscriptionRequest { samples, model, language: "ja", prompt: &prompt }).await?;
            seconds += started.elapsed().as_secs_f64();
            let text = vocabulary::normalize(&transcript::clean_whisper_output(&raw), &replacements);
            let reference = comparable(expected);
            errors += edit_distance(&reference, &comparable(&text)) as f64;
            chars += reference.chars().count() as f64;
            names += terms.len();
            names_ok += terms.iter().filter(|t| text.contains(*t)).count();
            if show {
                println!("  {text}");
            }
        }
        let peak = peak_memory_mb(&tool, &whisper_args(model, &clips[0].2, &prompt));
        println!(
            "{:<40} {:>6.1}% {:>5}/{:<3} {:>8.2}s {:>9}",
            file_name(model),
            errors / chars * 100.0,
            names_ok,
            names,
            seconds / clips.len() as f64,
            peak.map_or("-".to_owned(), |mb| format!("{mb} MB"))
        );
    }
    Ok(())
}

pub async fn llm(store: &ConfigStore, models: &[PathBuf], show: bool) -> Result<()> {
    let dictionary: Dictionary = store.dictionary();
    let llama = LlamaCli::new(store.settings().tools.llama, Some(store.dirs().tools_dir()));
    let tool = tools::locate(Tool::Llama, store.settings().tools.llama.as_deref(), Some(&store.dirs().tools_dir()))
        .context("llama-completion not found")?;

    // Baseline: the rule-based text the app uses when the LLM is off or rejected
    let (mut base_err, mut base_chars) = (0.0, 0.0);
    for (input, expected) in LLM_CASES {
        let rules = pipeline::apply_rules(input, true);
        base_err += edit_distance(&comparable(expected), &comparable(&rules)) as f64;
        base_chars += comparable(expected).chars().count() as f64;
    }
    println!("{} cases. Rules only (no LLM): CER {:.1}%\n", LLM_CASES.len(), base_err / base_chars * 100.0);
    println!(
        "{:<36} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "model", "LLM CER", "accepted", "final CER", "avg time", "peak RAM"
    );

    for model in models {
        let style = PromptStyle::for_model(model);
        let (mut llm_err, mut final_err, mut chars, mut accepted, mut seconds) = (0.0, 0.0, 0.0, 0, 0.0);
        for (input, expected) in LLM_CASES {
            let rules = pipeline::apply_rules(input, true);
            let prompt = prompt_for(style, &rules);
            let started = Instant::now();
            let raw = llama
                .complete(CompletionRequest { model, prompt: &prompt, max_tokens: max_tokens_for(&rules) })
                .await?;
            seconds += started.elapsed().as_secs_f64();
            let output =
                vocabulary::normalize(&drop_inserted_spaces(&rules, &clean_output(&raw)), &dictionary.replacements());
            let ok = edit_guard::check(&rules, &output).accepted();
            let final_text = if ok { output.clone() } else { rules.clone() };
            let reference = comparable(expected);
            llm_err += edit_distance(&reference, &comparable(&output)) as f64;
            final_err += edit_distance(&reference, &comparable(&final_text)) as f64;
            chars += reference.chars().count() as f64;
            accepted += usize::from(ok);
            if show {
                println!("  [{}] {output}", if ok { "ok" } else { "NG" });
            }
        }
        let rules = pipeline::apply_rules(LLM_CASES[0].0, true);
        let prompt_file = tempfile::NamedTempFile::new()?;
        std::fs::write(prompt_file.path(), prompt_for(style, &rules))?;
        let peak = peak_memory_mb(&tool, &llama_args(model, prompt_file.path()));
        println!(
            "{:<36} {:>8.1}% {:>5}/{:<3} {:>8.1}% {:>8.2}s {:>9}",
            file_name(model),
            llm_err / chars * 100.0,
            accepted,
            LLM_CASES.len(),
            final_err / chars * 100.0,
            seconds / LLM_CASES.len() as f64,
            peak.map_or("-".to_owned(), |mb| format!("{mb} MB"))
        );
    }
    Ok(())
}

/// NFKC, lower case, without spaces and punctuation: only the words count.
fn comparable(s: &str) -> String {
    s.nfkc().flat_map(char::to_lowercase).filter(|c| !c.is_whitespace() && !"、。,.!?！？「」・".contains(*c)).collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + usize::from(ca != cb));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn whisper_args(model: &Path, wav: &Path, prompt: &str) -> Vec<String> {
    vec![
        "-m".into(),
        path(model),
        "-f".into(),
        path(wav),
        "-l".into(),
        "ja".into(),
        "-nt".into(),
        "--prompt".into(),
        prompt.into(),
    ]
}

fn llama_args(model: &Path, prompt_file: &Path) -> Vec<String> {
    vec![
        "-m".into(),
        path(model),
        "-f".into(),
        path(prompt_file),
        // Same context as the app; the model default (up to 256K tokens) would inflate memory
        "-c".into(),
        "4096".into(),
        "-n".into(),
        "128".into(),
        "--temp".into(),
        "0".into(),
        "-no-cnv".into(),
        "--no-display-prompt".into(),
        "--no-warmup".into(),
    ]
}

/// Peak resident memory of one run of `program`, via `/usr/bin/time -l` (macOS only).
fn peak_memory_mb(program: &Path, args: &[String]) -> Option<u64> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = Command::new("/usr/bin/time")
        .arg("-l")
        .arg(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr.lines().find(|l| l.contains("maximum resident set size"))?;
    let bytes: u64 = line.split_whitespace().next()?.parse().ok()?;
    Some(bytes / (1024 * 1024))
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| path(p), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_and_normalisation() {
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("", "ab"), 2);
        assert_eq!(comparable("Claude、です。 ＡＢ"), "claudeですab");
    }
}
