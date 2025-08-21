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
