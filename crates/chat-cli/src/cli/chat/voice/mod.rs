// Core modules
pub mod audio_capture;
pub mod common;
pub mod transcriber;
pub mod voice_display;

// New architecture modules
pub mod settings;
pub mod error;
pub mod streaming;
pub mod model_manager;
pub mod provider;
pub mod simple_handler;
pub mod providers;

// Legacy modules (for compatibility)
pub mod voice_handler;
pub mod transcription_provider;
pub mod aws_transcribe_provider;
pub mod parakeet_provider;
pub mod whisper_provider;

// Re-exports
pub use audio_capture::AudioCapture;
pub use voice_handler::VoiceHandler; // Legacy
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
