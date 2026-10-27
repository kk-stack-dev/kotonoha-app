//! The user's vocabulary: correct spellings of names, the ways speech recognition gets them wrong,
//! and which words to hint to Whisper.

use serde::{Deserialize, Serialize};

use crate::text::vocabulary::Replacement;

/// Characters of dictionary words sent to Whisper as a hint. Whisper reads the hint as text that came before
/// the audio and keeps only about 224 tokens of it, and a long list of names pulls ordinary words toward
/// them, so the list is cut short.
pub const DEFAULT_PROMPT_BUDGET: usize = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EntrySource {
    /// Shipped with the app.
    #[default]
    Builtin,
    /// Added by the user.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Correct spelling, e.g. "Claude".
    pub word: String,
    /// Readings and known misrecognitions, each rewritten to `word` after transcription.
    #[serde(default)]
    pub variants: Vec<String>,
    /// Whether the word is included in the Whisper prompt.
    #[serde(default = "default_true")]
    pub prompt_hint: bool,
    #[serde(default)]
    pub source: EntrySource,
    /// Off means the entry is kept but not used. Shipped words cannot be removed (they come back on the
    /// next start), so turning them off is how the user drops one.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Entry {
    pub fn new(word: impl Into<String>, variants: &[&str], source: EntrySource) -> Self {
        Self {
            word: word.into(),
            variants: variants.iter().map(|v| (*v).to_owned()).collect(),
            prompt_hint: true,
            source,
            enabled: true,
        }
    }

    /// Splits user input like "クロード, Claud、クラウド" into variants.
    pub fn parse_variants(text: &str) -> Vec<String> {
        text.split([',', '、', '，']).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("empty word")]
    Empty,
    #[error("already registered")]
    Duplicate,
    #[error("not found")]
    NotFound,
    #[error("shipped words cannot be changed")]
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Dictionary {
    pub entries: Vec<Entry>,
}

impl Dictionary {
    /// Product names often said while working with AI tools. Variants are the katakana readings and the
    /// spellings Whisper large-v3-turbo produced for them from synthesised speech, with and without the
    /// dictionary prompt (`kotonoha bench asr` sentences and others).
    pub fn builtin() -> Self {
        let b = |word, variants| Entry::new(word, variants, EntrySource::Builtin);
        Self {
            entries: vec![
                b("Claude", &["クロード", "Claud", "Cloude"]),
                b("Claude Code", &["クロードコード", "クロード コード", "Claudeコード", "Cloude Code", "Claud Code"]),
                b("ChatGPT", &["チャットジーピーティー", "チャットGPT", "チャットGBT"]),
                b("Gemini", &["ジェミニ", "デミニ"]),
                b("Codex", &["コーデックス"]),
                b("NotebookLM", &["ノートブックエルエム", "ノートブックLM"]),
                b("Google Workspace", &["グーグルワークスペース", "Googleワークスペース"]),
                b("GitHub", &["ギットハブ", "Github"]),
                b("Slack", &["スラック"]),
            ],
        }
    }

    /// Adds missing words and missing variants of existing words. Never removes anything.
    pub fn merge(&mut self, extra: impl IntoIterator<Item = Entry>) {
        for add in extra {
            let word = add.word.trim();
            if word.is_empty() {
                continue;
            }
            match self.entries.iter_mut().find(|e| e.word == word) {
                Some(existing) => {
                    for v in add.variants {
                        if v != existing.word && !existing.variants.contains(&v) {
                            existing.variants.push(v);
                        }
                    }
                }
                None => self.entries.push(Entry { word: word.to_owned(), ..add }),
            }
        }
    }

    /// Adds a user word. Returns false when the word is empty or already present.
    pub fn add(&mut self, word: &str, variants: Vec<String>) -> bool {
        let word = word.trim();
        if word.is_empty() || self.entries.iter().any(|e| e.word == word) {
            return false;
        }
        self.entries.push(Entry::new(word, &[], EntrySource::User));
        self.entries.last_mut().unwrap().variants = variants;
        true
    }

    /// Removes a user word. Shipped words are only turned off (see [`Entry::enabled`]).
    pub fn remove(&mut self, word: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.word != word || e.source == EntrySource::Builtin);
        self.entries.len() != before
    }

    /// Changes a user word's spelling and variants in place, keeping its position in the list.
    pub fn edit(&mut self, word: &str, new_word: &str, variants: Vec<String>) -> Result<(), EditError> {
        let new_word = new_word.trim();
        if new_word.is_empty() {
            return Err(EditError::Empty);
        }
        if new_word != word && self.entries.iter().any(|e| e.word == new_word) {
            return Err(EditError::Duplicate);
        }
        let entry = self.entries.iter_mut().find(|e| e.word == word).ok_or(EditError::NotFound)?;
        if entry.source == EntrySource::Builtin {
            return Err(EditError::Builtin);
        }
        entry.word = new_word.to_owned();
        entry.variants = variants;
        Ok(())
    }

    /// Returns false when there is no such word.
    pub fn set_enabled(&mut self, word: &str, enabled: bool) -> bool {
        self.entries.iter_mut().find(|e| e.word == word).map(|e| e.enabled = enabled).is_some()
    }

    fn active(&self) -> impl Iterator<Item = &Entry> + Clone {
        self.entries.iter().filter(|e| e.enabled)
    }

    /// Hint words joined Japanese-style ("Claude、Codex。"), shipped words first, cut at `budget` characters.
    pub fn whisper_prompt(&self, budget: usize) -> String {
        let hints = self.active().filter(|e| e.prompt_hint);
        let ordered = hints
            .clone()
            .filter(|e| e.source == EntrySource::Builtin)
            .chain(hints.filter(|e| e.source == EntrySource::User));
        let mut prompt = String::new();
        for entry in ordered {
            let added = if prompt.is_empty() { 0 } else { 1 } + entry.word.chars().count();
            if prompt.chars().count() + added > budget {
                break;
            }
            if !prompt.is_empty() {
                prompt.push('、');
            }
            prompt.push_str(&entry.word);
        }
        if !prompt.is_empty() {
            prompt.push('。');
        }
        prompt
    }

    /// Longest variant first, so "クロードコード" is handled before "クロード".
    pub fn replacements(&self) -> Vec<Replacement> {
        let mut pairs: Vec<Replacement> = self
            .active()
            .flat_map(|e| {
                e.variants
                    .iter()
                    .filter(|v| !v.is_empty() && **v != e.word)
                    .map(|v| Replacement { variant: v.clone(), word: e.word.clone() })
            })
            .collect();
        pairs.sort_by_key(|p| std::cmp::Reverse(p.variant.chars().count()));
        pairs
    }

    pub fn protected_words(&self) -> Vec<String> {
        self.active().map(|e| e.word.clone()).collect()
    }
}

/// A word list to import: `[{"word": "...", "reading": "a, b"}]`, with the variants comma-separated.
pub fn parse_word_list_json(json: &str) -> Result<Vec<Entry>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Item {
        word: String,
        #[serde(default)]
        reading: String,
    }
    let items: Vec<Item> = serde_json::from_str(json)?;
    Ok(items
        .into_iter()
        .filter(|i| !i.word.trim().is_empty())
        .map(|i| Entry {
            word: i.word.trim().to_owned(),
            variants: Entry::parse_variants(&i.reading),
            prompt_hint: true,
            source: EntrySource::User,
            enabled: true,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::vocabulary::normalize;

    fn user(word: &str) -> Entry {
        Entry::new(word, &[], EntrySource::User)
    }

    #[test]
    fn builtin_variants_become_product_names() {
        let reps = Dictionary::builtin().replacements();
        assert_eq!(
            normalize("ギットハブの課題をクロードコードに渡して、結果はスラックとデミニで共有しました", &reps),
            "GitHubの課題をClaude Codeに渡して、結果はSlackとGeminiで共有しました"
        );
        // Whole words only: a longer katakana or Latin word is left alone
        assert_eq!(normalize("クロードレーヌとSlacker", &reps), "クロードレーヌとSlacker");
    }

    #[test]
    fn merge_adds_words_and_variants_only() {
        let mut dict = Dictionary {
            entries: vec![Entry::new("Claude", &["くろうど"], EntrySource::User), user("ひだまり工房")],
        };
        dict.merge(Dictionary::builtin().entries);
        let claude = dict.entries.iter().find(|e| e.word == "Claude").unwrap();
        assert!(claude.variants.contains(&"くろうど".to_owned()));
        assert!(claude.variants.contains(&"クロード".to_owned()));
        assert!(dict.entries.iter().any(|e| e.word == "ひだまり工房"));
        assert!(dict.entries.iter().any(|e| e.word == "GitHub"));
    }

    #[test]
    fn a_word_list_file_is_imported() {
        let entries = parse_word_list_json(r#"[{"word": "ひだまり工房", "reading": "ひだまりこうぼう, 日だまり工房"}, {"word": "Slack", "reading": "スラッグ"}, {"word": " "}]"#).unwrap();
        let mut dict = Dictionary::builtin();
        dict.merge(entries);
        let studio = dict.entries.iter().find(|e| e.word == "ひだまり工房").unwrap();
        assert!(studio.variants.contains(&"日だまり工房".to_owned()));
        let slack = dict.entries.iter().find(|e| e.word == "Slack").unwrap();
        assert!(slack.variants.contains(&"スラッグ".to_owned()) && slack.variants.contains(&"スラック".to_owned()));
        assert!(parse_word_list_json("not json").is_err());
    }

    #[test]
    fn prompt_puts_shipped_words_first_and_respects_budget() {
        let b = |w| Entry::new(w, &[], EntrySource::Builtin);
        let dict = Dictionary {
            entries: vec![user("Substack"), b("Claude"), user("Notion"), b("Codex"), b("Gemini"), user("Lancers")],
        };
        assert_eq!(dict.whisper_prompt(30), "Claude、Codex、Gemini、Substack。");
        assert_eq!(dict.whisper_prompt(150), "Claude、Codex、Gemini、Substack、Notion、Lancers。");
        assert_eq!(Dictionary::default().whisper_prompt(150), "");
        let builtin = Dictionary::builtin().whisper_prompt(DEFAULT_PROMPT_BUDGET);
        assert!(builtin.ends_with('。') && builtin.contains('、') && builtin.chars().count() <= 151);
    }

    #[test]
    fn add_and_remove() {
        let mut dict = Dictionary::default();
        assert!(dict.add(" Kotonoha ", Entry::parse_variants("ことのは、コトノハ")));
        assert!(!dict.add("Kotonoha", vec![]));
        assert_eq!(dict.entries[0].variants, vec!["ことのは", "コトノハ"]);
        assert!(dict.remove("Kotonoha"));
        assert!(!dict.remove("Kotonoha"));
    }

    #[test]
    fn shipped_words_are_turned_off_not_removed() {
        let mut dict = Dictionary::builtin();
        assert!(!dict.remove("Claude"));
        assert!(dict.set_enabled("Claude", false));
        assert!(!dict.set_enabled("Missing", false));
        assert!(!dict.protected_words().contains(&"Claude".to_owned()));
        assert!(!dict.replacements().iter().any(|r| r.word == "Claude"));
        assert!(!dict.whisper_prompt(DEFAULT_PROMPT_BUDGET).contains("Claude、"));

        // The store merges the shipped list in again at start; the off state must survive that
        dict.merge(Dictionary::builtin().entries);
        assert!(!dict.entries.iter().find(|e| e.word == "Claude").unwrap().enabled);
    }

    #[test]
    fn edit_changes_user_words_only() {
        let mut dict = Dictionary::builtin();
        dict.add("Notion", vec![]);
        dict.add("Lancers", vec![]);
        assert_eq!(dict.edit("Notion", " Notion AI ", vec!["ノーション".into()]), Ok(()));
        let edited = dict.entries.iter().find(|e| e.word == "Notion AI").unwrap();
        assert_eq!(edited.variants, ["ノーション"]);
        assert_eq!(dict.edit("Notion AI", "Lancers", vec![]), Err(EditError::Duplicate));
        assert_eq!(dict.edit("Notion AI", " ", vec![]), Err(EditError::Empty));
        assert_eq!(dict.edit("Missing", "x", vec![]), Err(EditError::NotFound));
        assert_eq!(dict.edit("Claude", "Claude 3", vec![]), Err(EditError::Builtin));
    }

    #[test]
    fn older_files_without_enabled_load_as_on() {
        let dict: Dictionary = serde_json::from_str(r#"{"entries":[{"word":"Claude","source":"builtin"}]}"#).unwrap();
        assert!(dict.entries[0].enabled);
    }
}
