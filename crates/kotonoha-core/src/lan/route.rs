//! Sends a session's work to the paired computer, falling back to this one when it cannot be reached.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use crate::ports::{
    CompletionRequest, EngineError, LanguageModel, RemoteEngine, SpeechRecognizer, TranscriptionRequest,
};

/// Speech recognition on the paired computer. Once it is unreachable, the rest of the session's chunks
/// go straight to `local` (when given), instead of waiting for the connection to time out each time.
pub(crate) struct RoutedRecognizer {
    pub remote: Arc<dyn RemoteEngine>,
    /// This computer's recognizer and model, when falling back is allowed and possible.
    pub local: Option<(Arc<dyn SpeechRecognizer>, PathBuf)>,
    /// Shared by the session's work: set when the paired computer could not be reached.
    pub unreachable: Arc<AtomicBool>,
}

#[async_trait]
impl SpeechRecognizer for RoutedRecognizer {
    async fn transcribe(&self, request: TranscriptionRequest<'_>) -> Result<String, EngineError> {
        if !self.unreachable.load(Ordering::SeqCst) {
            match self.remote.transcribe(request.samples, request.language, request.prompt).await {
                Err(EngineError::Unreachable(reason)) => {
                    self.unreachable.store(true, Ordering::SeqCst);
                    tracing::warn!(%reason, fallback = self.local.is_some(), "paired computer unreachable");
                    if self.local.is_none() {
                        return Err(EngineError::Unreachable(reason));
                    }
                }
                other => return other,
            }
        }
        match &self.local {
            Some((local, model)) => local.transcribe(TranscriptionRequest { model, ..request }).await,
            None => Err(EngineError::Unreachable("unreachable earlier in this session".into())),
        }
    }
}

/// The language model on the paired computer. The model path in the request only chose the prompt
/// format; the other computer runs its own model.
pub(crate) struct RemoteLlm(pub Arc<dyn RemoteEngine>);

#[async_trait]
impl LanguageModel for RemoteLlm {
    async fn complete(&self, request: CompletionRequest<'_>) -> Result<String, EngineError> {
        self.0.complete(request.prompt, request.max_tokens).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;

    struct Remote(Mutex<Vec<Result<String, EngineError>>>);

    #[async_trait]
    impl RemoteEngine for Remote {
        fn is_paired(&self) -> bool {
            true
        }
        async fn transcribe(&self, _: &[f32], _: &str, _: &str) -> Result<String, EngineError> {
            self.0.lock().unwrap().remove(0)
        }
        async fn complete(&self, prompt: &str, _: usize) -> Result<String, EngineError> {
            Ok(format!("remote:{prompt}"))
        }
        async fn llm_model(&self) -> Result<Option<String>, EngineError> {
            Ok(None)
        }
    }

    struct Local(Mutex<Vec<PathBuf>>);

    #[async_trait]
    impl SpeechRecognizer for Local {
        async fn transcribe(&self, request: TranscriptionRequest<'_>) -> Result<String, EngineError> {
            self.0.lock().unwrap().push(request.model.to_owned());
            Ok("local".into())
        }
    }

    fn request() -> TranscriptionRequest<'static> {
        TranscriptionRequest { samples: &[], model: Path::new(""), language: "ja", prompt: "" }
    }

    #[tokio::test]
    async fn falls_back_once_unreachable_and_stays_there() {
        let remote = Arc::new(Remote(Mutex::new(vec![
            Ok("remote".into()),
            Err(EngineError::Unreachable("refused".into())),
            Ok("never asked".into()),
        ])));
        let local = Arc::new(Local(Mutex::default()));
        let routed = RoutedRecognizer {
            remote,
            local: Some((local.clone(), PathBuf::from("/m/ggml-small.bin"))),
            unreachable: Arc::default(),
        };
        assert_eq!(routed.transcribe(request()).await.unwrap(), "remote");
        assert_eq!(routed.transcribe(request()).await.unwrap(), "local");
        assert_eq!(routed.transcribe(request()).await.unwrap(), "local");
        assert_eq!(
            local.0.lock().unwrap().as_slice(),
            [PathBuf::from("/m/ggml-small.bin"), "/m/ggml-small.bin".into()]
        );
    }

    #[tokio::test]
    async fn without_fallback_unreachable_is_reported_and_other_errors_pass_through() {
        let remote = Arc::new(Remote(Mutex::new(vec![
            Err(EngineError::TimedOut),
            Err(EngineError::Unreachable("refused".into())),
        ])));
        let routed = RoutedRecognizer { remote, local: None, unreachable: Arc::default() };
        assert_eq!(routed.transcribe(request()).await, Err(EngineError::TimedOut));
        assert!(matches!(routed.transcribe(request()).await, Err(EngineError::Unreachable(_))));
        assert!(matches!(routed.transcribe(request()).await, Err(EngineError::Unreachable(_))));
    }

    #[tokio::test]
    async fn a_failure_on_the_other_computer_does_not_switch_to_local() {
        let remote = Arc::new(Remote(Mutex::new(vec![Err(EngineError::Failed("model".into())), Ok("ok".into())])));
        let local = Arc::new(Local(Mutex::default()));
        let routed =
            RoutedRecognizer { remote, local: Some((local.clone(), PathBuf::from("m"))), unreachable: Arc::default() };
        assert!(matches!(routed.transcribe(request()).await, Err(EngineError::Failed(_))));
        assert_eq!(routed.transcribe(request()).await.unwrap(), "ok");
        assert!(local.0.lock().unwrap().is_empty());
    }
}
