pub mod audio_capture;
pub mod transcriber;
pub mod voice_handler;

pub use audio_capture::AudioCapture;
pub use transcriber::VoiceTranscriber;
pub use voice_handler::VoiceHandler;

use thiserror::Error;

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
}

pub fn show_voice_setup_help() {
    println!("🎤 Voice Mode Setup");
    println!("==================");
    println!();
    println!("Requirements:");
    println!("• Microphone access permission");
    println!("• AWS credentials with Transcribe permissions");
    println!("• Stable internet connection");
    println!();
    println!("Usage:");
    println!("• Speak clearly into your microphone");
    println!("• Pause briefly when finished speaking");
    println!("• Press Ctrl+C to exit voice mode");
    println!();
}
