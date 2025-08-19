pub mod audio_capture;
pub mod common;
pub mod transcriber;
pub mod voice_handler;
pub mod transcription_provider;
pub mod aws_transcribe_provider;
pub mod parakeet_provider;
pub mod whisper_provider;

pub use audio_capture::AudioCapture;
use thiserror::Error;
pub use voice_handler::VoiceHandler;
pub use transcription_provider::TranscriptionBackend;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("Microphone not available or permission denied")]
    MicrophoneUnavailable,

    #[error("AWS Transcribe service unavailable: {0}")]
    TranscribeUnavailable(String),

    #[error("Audio format not supported")]
    UnsupportedAudioFormat,

    #[error("Network connectivity issues")]
    #[allow(dead_code)]
    NetworkError,

    #[error("Audio processing error: {0}")]
    AudioProcessingError(String),
}

pub fn show_voice_setup_help() {
    println!("🎤 Voice Mode Setup");
    println!("==================");
    println!();
    println!("Requirements:");
    println!("• Microphone access permission");
    println!("• AWS credentials with Transcribe permissions OR local models");
    println!("• Stable internet connection (for AWS Transcribe only)");
    println!();
    println!("Usage:");
    println!("• Speak clearly into your microphone in English");
    println!("• Pause briefly when finished speaking");
    println!("• Press Enter to stop recording or Ctrl+C to cancel");
    println!();
    println!("Backend Options:");
    println!("• AWS Transcribe: /voice or /voice --backend aws-transcribe (default)");
    println!("• Local Whisper: /voice --backend local-whisper");
    println!("• Local Parakeet: /voice --backend local-parakeet");
    println!();
}
