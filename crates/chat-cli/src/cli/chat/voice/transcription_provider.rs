use async_trait::async_trait;
use eyre::Result;

// Re-export existing types from transcriber
pub use super::transcriber::{TranscriptEvent, TranscriptionResult};

#[async_trait]
pub trait TranscriptionProvider {
    async fn start_transcription(&self) -> Result<TranscriptionResult>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TranscriptionBackend {
    AwsTranscribe,
    LocalParakeet,
    LocalWhisper,
}

impl Default for TranscriptionBackend {
    fn default() -> Self {
        Self::AwsTranscribe
    }
}
