use clap::Args;
use crossterm::execute;
use crossterm::style::{self, Attribute, Color};
use std::path::PathBuf;

use crate::cli::chat::voice::{
    SimpleVoiceHandler, VoiceSettings, TranscriptionBackend,
    show_voice_setup_help,
};
use crate::cli::chat::{ChatError, ChatSession, ChatState};
use crate::database::settings::Setting;
use crate::os::Os;

#[derive(Debug, PartialEq, Args)]
pub struct VoiceEnhancedArgs {
    /// Transcription backend to use
    #[arg(long, value_enum, default_value = "aws-transcribe")]
    pub backend: TranscriptionBackendArg,

    /// Enable streaming transcription
    #[arg(long)]
    pub streaming: bool,

    /// Set voice language
    #[arg(long, default_value = "en")]
    pub language: String,

    /// Set transcription temperature (0.0-1.0)
    #[arg(long, default_value = "0.0")]
    pub temperature: f32,

    /// Show detailed setup information
    #[arg(long)]
    pub setup: bool,
}

#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
pub enum TranscriptionBackendArg {
    #[value(name = "aws-transcribe")]
    AwsTranscribe,
    #[value(name = "local-parakeet")]
    LocalParakeet,
    #[value(name = "local-whisper")]
    LocalWhisper,
}

impl From<TranscriptionBackendArg> for TranscriptionBackend {
    fn from(arg: TranscriptionBackendArg) -> Self {
        match arg {
            TranscriptionBackendArg::AwsTranscribe => TranscriptionBackend::AwsTranscribe,
            TranscriptionBackendArg::LocalParakeet => TranscriptionBackend::LocalParakeet,
            TranscriptionBackendArg::LocalWhisper => TranscriptionBackend::LocalWhisper,
        }
    }
}

impl VoiceEnhancedArgs {
    pub async fn execute(self, os: &mut Os, session: &mut ChatSession) -> Result<ChatState, ChatError> {
        if self.setup {
            show_voice_setup_help();
            return Ok(ChatState::PromptUser { skip_printing_tools: true });
        }

        // Load or create voice settings
        let mut settings = self.load_settings(os).await?;
        
        // Update settings from CLI args
        settings.selected_provider = self.backend.into();
        settings.language = self.language;
        settings.temperature = self.temperature;
        settings.streaming_enabled = self.streaming;

        // Save updated settings
        self.save_settings(os, &settings).await?;

        execute!(
            session.stderr,
            style::SetForegroundColor(Color::Cyan),
            style::Print(format!("🎤 Activating enhanced voice input mode ({})\n", 
                self.get_provider_name(&settings.selected_provider))),
            style::SetForegroundColor(Color::Reset)
        )?;

        // Create cache directory
        let cache_dir = self.get_cache_dir(os).await?;

        // Initialize voice handler
        let mut voice_handler = SimpleVoiceHandler::new(settings);
        
        match voice_handler.initialize().await {
            Ok(()) => {
                // Check setup
                if let Err(e) = voice_handler.check_setup().await {
                    self.display_error(session, &voice_handler.get_rich_error(e)).await?;
                    return Ok(ChatState::PromptUser { skip_printing_tools: true });
                }

                // Listen for voice input
                match voice_handler.listen_for_speech().await {
                    Ok(Some(voice_input)) => {
                        execute!(
                            session.stderr,
                            style::SetForegroundColor(Color::Green),
                            style::Print("✅ Enhanced voice input captured. Submitting prompt...\n\n"),
                            style::SetForegroundColor(Color::Reset)
                        )?;

                        // Display transcribed content
                        execute!(
                            session.stderr,
                            style::SetAttribute(Attribute::Reset),
                            style::SetForegroundColor(Color::Magenta),
                            style::Print("> "),
                            style::SetAttribute(Attribute::Reset),
                            style::Print(&voice_input),
                            style::Print("\n")
                        )?;

                        Ok(ChatState::HandleInput { input: voice_input })
                    }
                    Ok(None) => {
                        execute!(
                            session.stderr,
                            style::SetForegroundColor(Color::Yellow),
                            style::Print("🔇 No voice input detected\n\n"),
                            style::SetForegroundColor(Color::Reset)
                        )?;

                        Ok(ChatState::PromptUser { skip_printing_tools: true })
                    }
                    Err(e) => {
                        self.display_error(session, &voice_handler.get_rich_error(e)).await?;
                        Ok(ChatState::PromptUser { skip_printing_tools: true })
                    }
                }
            }
            Err(e) => {
                let rich_error = voice_handler.get_rich_error(e);
                self.display_error(session, &rich_error).await?;
                Ok(ChatState::PromptUser { skip_printing_tools: true })
            }
        }
    }

    async fn load_settings(&self, os: &Os) -> Result<VoiceSettings, ChatError> {
        let mut settings = VoiceSettings::default();
        
        if let Some(provider_str) = os.database.settings.get_string(Setting::VoiceProvider) {
            settings.selected_provider = match provider_str.as_str() {
                "aws-transcribe" => TranscriptionBackend::AwsTranscribe,
                "local-whisper" => TranscriptionBackend::LocalWhisper,
                "local-parakeet" => TranscriptionBackend::LocalParakeet,
                _ => TranscriptionBackend::AwsTranscribe,
            };
        }

        if let Some(language) = os.database.settings.get_string(Setting::VoiceLanguage) {
            settings.language = language;
        }

        if let Some(temp_str) = os.database.settings.get_string(Setting::VoiceTemperature) {
            if let Ok(temp) = temp_str.parse::<f32>() {
                settings.temperature = temp;
            }
        }

        if let Some(streaming_str) = os.database.settings.get_string(Setting::VoiceStreamingEnabled) {
            settings.streaming_enabled = streaming_str == "true";
        }
        
        Ok(settings)
    }

    async fn save_settings(&self, os: &mut Os, settings: &VoiceSettings) -> Result<(), ChatError> {
        let provider_str = match settings.selected_provider {
            TranscriptionBackend::AwsTranscribe => "aws-transcribe",
            TranscriptionBackend::LocalWhisper => "local-whisper",
            TranscriptionBackend::LocalParakeet => "local-parakeet",
        };

        os.database.settings
            .set(Setting::VoiceProvider, provider_str.to_string())
            .await
            .map_err(|e| ChatError::Custom(format!("Failed to save provider: {}", e).into()))?;

        os.database.settings
            .set(Setting::VoiceLanguage, settings.language.clone())
            .await
            .map_err(|e| ChatError::Custom(format!("Failed to save language: {}", e).into()))?;

        os.database.settings
            .set(Setting::VoiceTemperature, settings.temperature.to_string())
            .await
            .map_err(|e| ChatError::Custom(format!("Failed to save temperature: {}", e).into()))?;

        os.database.settings
            .set(Setting::VoiceStreamingEnabled, settings.streaming_enabled.to_string())
            .await
            .map_err(|e| ChatError::Custom(format!("Failed to save streaming setting: {}", e).into()))?;

        Ok(())
    }

    async fn get_cache_dir(&self, os: &Os) -> Result<PathBuf, ChatError> {
        let cache_dir = os.directories.cache_dir().join("voice_models");
        
        tokio::fs::create_dir_all(&cache_dir).await
            .map_err(|e| ChatError::Custom(format!("Failed to create cache dir: {}", e).into()))?;
        
        Ok(cache_dir)
    }

    fn get_provider_name(&self, provider: &TranscriptionBackend) -> &str {
        match provider {
            TranscriptionBackend::AwsTranscribe => "AWS Transcribe",
            TranscriptionBackend::LocalWhisper => "Local Whisper",
            TranscriptionBackend::LocalParakeet => "Local Parakeet",
        }
    }

    async fn display_error(&self, session: &mut ChatSession, error: &RichVoiceError) -> Result<(), ChatError> {
        let color = match error.severity {
            crate::cli::chat::voice::error::ErrorSeverity::Error => Color::Red,
            crate::cli::chat::voice::error::ErrorSeverity::Warning => Color::Yellow,
            crate::cli::chat::voice::error::ErrorSeverity::Info => Color::Cyan,
        };

        execute!(
            session.stderr,
            style::SetForegroundColor(color),
            style::Print(format!("{}\n", error.title)),
            style::SetForegroundColor(Color::White),
            style::Print(format!("{}\n", error.description)),
        )?;

        if let Some(action) = &error.action {
            match action {
                crate::cli::chat::voice::error::ErrorAction::Link { label, href } => {
                    execute!(
                        session.stderr,
                        style::SetForegroundColor(Color::Blue),
                        style::Print(format!("💡 {}: {}\n", label, href)),
                    )?;
                }
                crate::cli::chat::voice::error::ErrorAction::MoreDetails { error_msg } => {
                    execute!(
                        session.stderr,
                        style::SetForegroundColor(Color::DarkGrey),
                        style::Print(format!("Details: {}\n", error_msg)),
                    )?;
                }
                crate::cli::chat::voice::error::ErrorAction::Retry => {
                    execute!(
                        session.stderr,
                        style::SetForegroundColor(Color::Green),
                        style::Print("💡 Try running the command again\n"),
                    )?;
                }
                crate::cli::chat::voice::error::ErrorAction::ConfigureProvider => {
                    execute!(
                        session.stderr,
                        style::SetForegroundColor(Color::Green),
                        style::Print("💡 Run /voice-enhanced --setup for configuration help\n"),
                    )?;
                }
            }
        }

        execute!(
            session.stderr,
            style::SetForegroundColor(Color::Reset),
            style::Print("\n")
        )?;

        Ok(())
    }
}
