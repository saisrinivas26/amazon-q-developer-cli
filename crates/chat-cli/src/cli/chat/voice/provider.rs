use async_trait::async_trait;
use std::collections::HashMap;

use super::error::VoiceResult;
use super::streaming::{AudioBlob, AudioStream, TranscriptionStream};
use super::model_manager::ModelInfo;

#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    pub language: String,
    pub temperature: f32,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub streaming: bool,
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            temperature: 0.0,
            prompt: None,
            model: None,
            streaming: true,
        }
    }
}

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// One-shot transcription of audio blob
    async fn transcribe(&self, audio: AudioBlob, options: &TranscriptionOptions) -> VoiceResult<String>;
    
    /// Streaming transcription with real-time updates
    async fn stream_transcribe(&self, audio_stream: AudioStream, options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream>;
    
    /// Check if provider supports streaming
    fn supports_streaming(&self) -> bool;
    
    /// Get available models for this provider
    fn get_models(&self) -> Vec<ModelInfo>;
    
    /// Get provider name
    fn name(&self) -> &str;
    
    /// Validate provider configuration
    async fn validate_config(&self) -> VoiceResult<()>;
    
    /// Get provider-specific settings
    fn get_settings(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

/// Factory for creating transcription providers
pub struct ProviderFactory;

impl ProviderFactory {
    pub async fn create_provider(
        provider_type: &str,
        config: HashMap<String, String>,
    ) -> VoiceResult<Box<dyn TranscriptionProvider>> {
        match provider_type {
            "aws-transcribe" => {
                let provider = super::providers::aws::AwsTranscribeProvider::new(config).await?;
                Ok(Box::new(provider))
            }
            "local-whisper" => {
                let provider = super::providers::whisper::WhisperProvider::new(config).await?;
                Ok(Box::new(provider))
            }
            "local-parakeet" => {
                let provider = super::providers::parakeet::ParakeetProvider::new(config).await?;
                Ok(Box::new(provider))
            }
            _ => Err(super::error::VoiceError::ProviderInitFailed(
                format!("Unknown provider: {}", provider_type)
            )),
        }
    }
    
    pub fn list_available_providers() -> Vec<&'static str> {
        vec!["aws-transcribe", "local-whisper", "local-parakeet"]
    }
}
