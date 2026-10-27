//! `kotonoha`: diagnostics, model management and end-to-end runs of the dictation pipeline
//! without the desktop app.

mod bench;
mod run;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use kotonoha_core::models::{self, MemoryMode, ModelTier, CATALOG};
use kotonoha_core::ports::{ConfigSource, ModelLibrary, SystemProbe};
use kotonoha_core::shortcut::Platform;
use kotonoha_platform::library::ModelDirectory;
use kotonoha_platform::paths::AppDirs;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::system::MemoryProbe;
use kotonoha_platform::tools::{self, Tool};
use kotonoha_platform::{download, output};

#[derive(Parser)]
#[command(name = "kotonoha", version, about = "Local voice input: diagnostics and tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what was found: tools, models, memory mode, permissions.
    Doctor,
    /// List, verify or download models.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Transcribe a WAV file through the full pipeline and print the result.
    Transcribe {
        file: PathBuf,
        /// Skip the LLM clean-up.
        #[arg(long)]
        no_llm: bool,
    },
    /// Record from the default microphone and print the result (checks capture end to end).
    Record {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    /// Manage the dictionary.
    Dictionary {
        #[command(subcommand)]
        action: DictionaryAction,
    },
    /// Compare models on fixed Japanese cases.
    Bench {
        #[command(subcommand)]
        target: BenchTarget,
    },
    /// Use model files from an existing folder instead of downloading.
    UseModelsFrom { dir: PathBuf },
}

#[derive(Subcommand)]
enum BenchTarget {
    /// Whisper models: CER and product names on synthesised speech (macOS).
    Asr {
        /// ggml model files to compare.
        #[arg(required = true)]
        models: Vec<PathBuf>,
        #[arg(long, default_value = "Kyoko,Eddy (日本語（日本）),Flo (日本語（日本）),Reed (日本語（日本）)")]
        voices: String,
        #[arg(long)]
        show: bool,
    },
    /// LLM models: clean-up quality, edit-guard acceptance and speed.
    Llm {
        /// GGUF model files to compare.
        #[arg(required = true)]
        models: Vec<PathBuf>,
        #[arg(long)]
        show: bool,
    },
}

#[derive(Subcommand)]
enum ModelsAction {
    List,
    /// Check installed catalog models against their checksums.
    Verify,
    /// Download the standard models (or the light ones too with --all).
    Download {
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum DictionaryAction {
    List,
    /// Import a personal dictionary in JSON ([{"word": ..., "reading": ...}]).
    Import {
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,kotonoha_core=info,kotonoha_platform=info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();

    let dirs = AppDirs::discover().context("no home directory")?;
    let store = Arc::new(ConfigStore::open(dirs));
    match Cli::parse().command {
        Command::Doctor => doctor(&store),
        Command::Models { action } => models_command(&store, action),
        Command::Transcribe { file, no_llm } => run::transcribe_file(store, &file, no_llm).await,
        Command::Record { seconds } => run::record(store, seconds).await,
        Command::Dictionary { action } => dictionary_command(&store, action),
        Command::Bench { target: BenchTarget::Asr { models, voices, show } } => {
            bench::asr(&store, &models, &voices, show).await
        }
        Command::Bench { target: BenchTarget::Llm { models, show } } => bench::llm(&store, &models, show).await,
        Command::UseModelsFrom { dir } => {
            let dir = dir.canonicalize().with_context(|| format!("{} not found", dir.display()))?;
            if kotonoha_platform::library::scan(&dir).is_empty() {
                bail!("no model files (ggml-*.bin, *.gguf) in {}", dir.display());
            }
            let mut settings = store.settings();
            settings.models.directory = Some(dir.clone());
            store.save_settings(settings)?;
            println!("Models directory set to {}", dir.display());
            Ok(())
        }
    }
}

fn doctor(store: &ConfigStore) -> Result<()> {
    let settings = store.settings();
    let dirs = store.dirs();
    println!("Config:   {}", dirs.config.display());
    println!("Logs:     {}", dirs.logs.display());
    println!("Shortcut: {}", settings.shortcut.label(Platform::current()));
    println!();
    for (tool, configured) in [(Tool::Whisper, &settings.tools.whisper), (Tool::Llama, &settings.tools.llama)] {
        match tools::locate(tool, configured.as_deref(), Some(&dirs.tools_dir())) {
            Some(path) => println!("{:<17} {}", tool.display_name(), path.display()),
            None => println!("{:<17} NOT FOUND", tool.display_name()),
        }
    }
    let available = MemoryProbe::default().available_memory_mb();
    let mode = MemoryMode::from_available_mb(available);
    println!("\nFree memory: {available} MB -> mode {mode:?}");
    let models_dir = store.models_dir();
    let library = ModelDirectory::new(move || models_dir.clone()).models();
    println!("Models in {}:", store.models_dir().display());
    for m in &library {
        println!("  {:<40} {:>6} MB", m.name, m.bytes / (1024 * 1024));
    }
    let pick = |m: Option<&models::ModelFile>| m.map_or("none".to_owned(), |m| m.name.clone());
    println!(
        "Would use: whisper={} llm={}",
        pick(models::choose_whisper(&library, mode)),
        pick(models::choose_llm(&library, mode))
    );
    println!("\nPaste permission: {}", if output::has_input_permission() { "granted" } else { "missing" });
    Ok(())
}

fn models_command(store: &ConfigStore, action: ModelsAction) -> Result<()> {
    let dir = store.models_dir();
    match action {
        ModelsAction::List => {
            for m in CATALOG {
                let state = if dir.join(m.file).exists() { "present" } else { "-" };
                let set = if m.tier == ModelTier::Standard { "standard" } else { "light" };
                println!("{:<40} {:>6} MB  {:<12} {}", m.file, m.bytes / (1024 * 1024), set, state);
            }
        }
        ModelsAction::Verify => {
            for m in CATALOG.iter().filter(|m| dir.join(m.file).exists()) {
                let ok = download::is_installed(m, &dir);
                println!("{:<40} {}", m.file, if ok { "ok" } else { "CHECKSUM MISMATCH" });
            }
        }
        ModelsAction::Download { all } => {
            for m in CATALOG.iter().filter(|m| all || m.tier == ModelTier::Standard) {
                if download::is_installed(m, &dir) {
                    println!("{} already installed", m.file);
                    continue;
                }
                println!("Downloading {} ({} MB)...", m.file, m.bytes / (1024 * 1024));
                let mut last = 0;
                download::download(m, &dir, |done, total| {
                    let pct = done * 100 / total.max(1);
                    if pct >= last + 10 {
                        last = pct;
                        println!("  {pct}%");
                    }
                })?;
            }
            println!("Models are in {}", dir.display());
        }
    }
    Ok(())
}

fn dictionary_command(store: &ConfigStore, action: DictionaryAction) -> Result<()> {
    match action {
        DictionaryAction::List => {
            for e in store.dictionary().entries {
                println!("{:<24} {}", e.word, e.variants.join(", "));
            }
        }
        DictionaryAction::Import { file } => {
            let n = store.import_word_list(&file)?;
            println!("Imported {n} entries from {}", file.display());
        }
    }
    Ok(())
}
