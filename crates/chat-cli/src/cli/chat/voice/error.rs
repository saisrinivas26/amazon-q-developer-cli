use thiserror::Error;
use eyre::Report as ErrReport;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("Microphone not available or permission denied")]
    MicrophoneUnavailable,

    #[error("AWS Transcribe service unavailable: {0}")]
    TranscribeUnavailable(String),

    #[error("Audio format not supported")]
    UnsupportedAudioFormat,

    #[error("Network connectivity issues")]
    NetworkError,

    #[error("Audio processing error: {0}")]
    AudioProcessingError(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Provider initialization failed: {0}")]
    ProviderInitFailed(String),

    #[error("Transcription failed: {0}")]
    TranscriptionFailed(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone)]
pub struct RichVoiceError {
    pub title: String,
    pub description: String,
    pub action: Option<ErrorAction>,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone)]
pub enum ErrorAction {
    Link { label: String, href: String },
    MoreDetails { error_msg: String },
    Retry,
    ConfigureProvider,
}

#[derive(Debug, Clone)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

impl From<VoiceError> for RichVoiceError {
    fn from(error: VoiceError) -> Self {
        match error {
            VoiceError::MicrophoneUnavailable => RichVoiceError {
                title: "🎤 Microphone Access Required".to_string(),
                description: "Please grant microphone permissions to use voice input.".to_string(),
                action: Some(ErrorAction::ConfigureProvider),
                severity: ErrorSeverity::Error,
            },
            VoiceError::TranscribeUnavailable(msg) => RichVoiceError {
                title: "☁️ AWS Transcribe Unavailable".to_string(),
                description: format!("AWS Transcribe service error: {}", msg),
                action: Some(ErrorAction::Link {
                    label: "Check AWS Status".to_string(),
                    href: "https://status.aws.amazon.com/".to_string(),
                }),
                severity: ErrorSeverity::Error,
            },
            VoiceError::ModelNotFound(model) => RichVoiceError {
                title: "📁 Model Not Found".to_string(),
                description: format!("Required model '{}' is not available.", model),
                action: Some(ErrorAction::Retry),
                severity: ErrorSeverity::Error,
            },
            VoiceError::ConfigError(msg) => RichVoiceError {
                title: "⚙️ Configuration Error".to_string(),
                description: msg,
                action: Some(ErrorAction::ConfigureProvider),
                severity: ErrorSeverity::Warning,
            },
            _ => RichVoiceError {
                title: "❌ Voice Error".to_string(),
                description: error.to_string(),
                action: Some(ErrorAction::MoreDetails {
                    error_msg: error.to_string(),
                }),
                severity: ErrorSeverity::Error,
            },
        }
    }
}

impl From<ErrReport> for VoiceError {
    fn from(err: ErrReport) -> Self {
        VoiceError::AudioProcessingError(err.to_string())
    }
}

pub type VoiceResult<T> = Result<T, VoiceError>;
