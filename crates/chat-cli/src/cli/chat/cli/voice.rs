use clap::Args;
use crossterm::execute;
use crossterm::style::{
    self,
    Attribute,
    Color,
};

use crate::aws_common::behavior_version;
use crate::cli::chat::voice::{
    VoiceHandler,
    show_voice_setup_help,
    TranscriptionBackend,
};
use crate::cli::chat::{
    ChatError,
    ChatSession,
    ChatState,
};
use crate::database::settings::Setting;
use crate::os::Os;

#[derive(Debug, PartialEq, Args)]
pub struct VoiceArgs {
    /// Voice input language (kept for UI display purposes only)
    #[arg(long, hide = true)]
    pub language: Option<String>,

    /// Set the default voice language (kept for UI display purposes only)
    #[arg(long, hide = true)]
    pub set_language: Option<String>,

    /// Transcription backend to use
    #[arg(long, value_enum, default_value = "aws-transcribe")]
    pub backend: TranscriptionBackendArg,
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

impl VoiceArgs {
    pub async fn execute(self, os: &mut Os, session: &mut ChatSession) -> Result<ChatState, ChatError> {
        // Handle setting the default language if requested
        if let Some(new_language) = &self.set_language {
            os.database
                .settings
                .set(Setting::VoiceLanguage, new_language.clone())
                .await
                .map_err(|e| ChatError::Custom(format!("Failed to save language setting: {}", e).into()))?;

            execute!(
                session.stderr,
                style::SetForegroundColor(Color::Green),
                style::Print(format!("✅ Default voice language set to: {}\n", new_language)),
                style::SetForegroundColor(Color::Reset)
            )?;

            // If only setting language, return to prompt
            if self.language.is_none() {
                return Ok(ChatState::PromptUser {
                    skip_printing_tools: true,
                });
            }
        }

        // Determine which language to use
        let language = self
            .language
            .or_else(|| os.database.settings.get_string(Setting::VoiceLanguage))
            .unwrap_or_else(|| "en-US".to_string());

        execute!(
            session.stderr,
            style::SetForegroundColor(Color::Cyan),
            style::Print(format!("🎤 Activating voice input mode (language: {})...\n", language)),
            style::SetForegroundColor(Color::Reset)
        )?;

        // Show voice setup help
        show_voice_setup_help();

        // Create AWS config for transcribe service
        let aws_config = aws_config::defaults(behavior_version()).load().await;

        // Using hardcoded English but still showing user's language preference in UI
        match VoiceHandler::new(&aws_config, self.backend.clone().into()).await {
            Ok(voice_handler) => {
                // Check voice setup
                if let Err(e) = voice_handler.check_setup().await {
                    execute!(
                        session.stderr,
                        style::SetForegroundColor(Color::Red),
                        style::Print(format!("❌ Voice setup failed: {}\n", e)),
                        style::SetForegroundColor(Color::Yellow),
                        style::Print("💡 Falling back to text input mode\n\n"),
                        style::SetForegroundColor(Color::Reset)
                    )?;

                    return Ok(ChatState::PromptUser {
                        skip_printing_tools: true,
                    });
                }

                // Listen for voice input
                match voice_handler.listen_for_speech().await {
                    Ok(Some(voice_input)) => {
                        execute!(
                            session.stderr,
                            style::SetForegroundColor(Color::Green),
                            style::Print("✅ Voice input captured. Submitting prompt...\n\n"),
                            style::SetForegroundColor(Color::Reset)
                        )?;

                        // Display the transcribed content as if the user typed it
                        execute!(
                            session.stderr,
                            style::SetAttribute(Attribute::Reset),
                            style::SetForegroundColor(Color::Magenta),
                            style::Print("> "),
                            style::SetAttribute(Attribute::Reset),
                            style::Print(&voice_input),
                            style::Print("\n")
                        )?;

                        // Process the voice input as user input
                        Ok(ChatState::HandleInput { input: voice_input })
                    },
                    Ok(None) => {
                        execute!(
                            session.stderr,
                            style::SetForegroundColor(Color::Yellow),
                            style::Print("🔇 No voice input detected\n\n"),
                            style::SetForegroundColor(Color::Reset)
                        )?;

                        Ok(ChatState::PromptUser {
                            skip_printing_tools: true,
                        })
                    },
                    Err(e) => {
                        execute!(
                            session.stderr,
                            style::SetForegroundColor(Color::Red),
                            style::Print(format!("❌ Voice input failed: {}\n", e)),
                            style::SetForegroundColor(Color::Yellow),
                            style::Print("💡 Falling back to text input mode\n\n"),
                            style::SetForegroundColor(Color::Reset)
                        )?;

                        Ok(ChatState::PromptUser {
                            skip_printing_tools: true,
                        })
                    },
                }
            },
            Err(e) => {
                execute!(
                    session.stderr,
                    style::SetForegroundColor(Color::Red),
                    style::Print(format!("❌ Failed to initialize voice handler: {}\n", e)),
                    style::SetForegroundColor(Color::Yellow),
                    style::Print("💡 Falling back to text input mode\n\n"),
                    style::SetForegroundColor(Color::Reset)
                )?;

                Ok(ChatState::PromptUser {
                    skip_printing_tools: true,
                })
            },
        }
    }
}
