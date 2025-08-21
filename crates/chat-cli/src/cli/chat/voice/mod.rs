// Core modules
pub mod audio_capture;
pub mod common;
pub mod voice_display;

// Architecture modules
pub mod error;
pub mod streaming;
pub mod provider;
pub mod providers;

// Main handler
pub mod voice_handler;
pub mod transcription_provider;

// Re-exports
pub use audio_capture::AudioCapture;
pub use voice_handler::VoiceHandler;
pub use transcription_provider::TranscriptionBackend;
pub use error::VoiceError;

pub fn show_voice_setup_help() {
    println!("🎤 Voice Mode Setup");
    println!("==================================================================");
    println!();
    println!("Requirements:");
    println!("• Microphone access permission");
    println!("• AWS credentials with Transcribe permissions OR local models");
    println!("• Stable internet connection (for AWS Transcribe only)");
    println!();
    println!("Usage:");
    println!("• Speak clearly into your microphone");
    println!("• Real-time transcription with streaming providers");
    println!("• Press Enter to stop recording or Ctrl+C to cancel");
    println!();
    println!("Backend Options:");
    println!("• AWS Transcribe: /voice --backend aws-transcribe (streaming)");
    println!("• Local Whisper: /voice --backend local-whisper (batch)");
    println!("• Local Parakeet: /voice --backend local-parakeet (streaming)");
    println!();
    println!("Features:");
    println!("• 🔄 Real-time streaming transcription");
    println!("• 📁 Automatic model management");
    println!("• ⚙️ Configurable settings");
    println!("• 🎯 Multi-provider support");
    println!();
}
