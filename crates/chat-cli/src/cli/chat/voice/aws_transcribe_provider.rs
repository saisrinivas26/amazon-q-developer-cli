use async_trait::async_trait;
use aws_config::SdkConfig;
use eyre::Result;

use super::transcriber::VoiceTranscriber;
use super::transcription_provider::{TranscriptionProvider, TranscriptionResult};

pub struct AwsTranscribeProvider {
    transcriber: VoiceTranscriber,
}

impl AwsTranscribeProvider {
    pub async fn new(aws_config: &SdkConfig, language: &str) -> Result<Self> {
        let transcriber = VoiceTranscriber::new(aws_config, language).await?;
        Ok(Self { transcriber })
    }
}

#[async_trait]
impl TranscriptionProvider for AwsTranscribeProvider {
    async fn start_transcription(&self) -> Result<TranscriptionResult> {
        self.transcriber.start_transcription().await
    }
}
