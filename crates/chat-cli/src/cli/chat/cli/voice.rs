use clap::Args;
use crossterm::execute;
use crossterm::style::{
    self,
    Attribute,
    Color,
};

use crate::cli::chat::{
    ChatError,
    ChatSession,
    ChatState,
};
use crate::cli::chat::voice::{VoiceHandler, show_voice_setup_help};
use crate::aws_common::behavior_version;

#[derive(Debug, PartialEq, Args)]
pub struct VoiceArgs {
    /// Voice input language (default: en-US)
    #[arg(long, default_value = "en-US")]
    pub language: String,
}

impl VoiceArgs {
    pub async fn execute(self, session: &mut ChatSession) -> Result<ChatState, ChatError> {
        execute!(
            session.stderr,
            style::SetForegroundColor(Color::Cyan),
            style::Print("🎤 Activating voice input mode...\n"),
            style::SetForegroundColor(Color::Reset)
        )?;

        // Show voice setup help
        show_voice_setup_help();

        // Create AWS config for transcribe service
        let aws_config = aws_config::defaults(behavior_version())
            .load()
            .await;

        match VoiceHandler::new(&aws_config, &self.language).await {
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
                    }
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
                    }
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
                    }
                }
            }
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
            }
        }
    }
}
