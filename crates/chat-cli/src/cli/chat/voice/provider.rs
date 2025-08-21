use async_trait::async_trait;

use super::error::VoiceResult;
use super::streaming::{AudioStream, TranscriptionStream};

#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    // Only keep fields that might be used in the future
    // Currently only used as a placeholder
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {}
    }
}

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Streaming transcription with real-time updates
    async fn stream_transcribe(&self, audio_stream: AudioStream, options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream>;
    
    /// Check if provider supports streaming
    fn supports_streaming(&self) -> bool;
}
