use std::io::{
    self,
    Write,
};
use std::time::{
    Duration,
    Instant,
};

use aws_config::SdkConfig;
use crossterm::style::Stylize;
use eyre::Result;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{
    debug,
    error,
    info,
    warn,
};

use super::transcriber::send_audio_to_transcribe;
use super::{
    AudioCapture,
    VoiceError,
    VoiceTranscriber,
};

#[derive(Debug)]
enum InputEvent {
    Enter,
    CtrlC,
    Error,
}

pub struct VoiceHandler {
    transcriber: VoiceTranscriber,
    audio_capture: AudioCapture,
}

impl VoiceHandler {
    pub async fn new(aws_config: &SdkConfig, language: &str) -> Result<Self> {
        let transcriber = VoiceTranscriber::new(aws_config, language).await?;
        let audio_capture = AudioCapture::new()?;

        Ok(Self {
            transcriber,
            audio_capture,
        })
    }

    pub async fn listen_for_speech(&self) -> Result<Option<String>> {
        println!("🎤 Voice mode activated. Speak now...");
        println!("   (Press Ctrl+C to cancel or Enter to stop recording)");
        println!();

        // Start real AWS Transcribe streaming session
        let transcription_result = self.transcriber.start_transcription().await?;

        // Create audio processing channels
        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<u8>>(1000);

        // Start real audio capture
        let _stream = self.audio_capture.start_capture(audio_tx)?;

        // Get channels for real AWS communication
        let transcribe_sender = transcription_result.audio_sender.clone();
        let mut transcript_receiver = transcription_result.transcript_receiver;

        // Spawn task to forward real audio data to AWS Transcribe
        let audio_forward_handle =
            tokio::spawn(async move { send_audio_to_transcribe(&mut audio_rx, &transcribe_sender).await });

        // Recording UI with simple, reliable display
        let mut current_transcript = String::new();
        let mut last_speech_time = Instant::now();
        let silence_timeout = Duration::from_secs(5);
        let recording_start = Instant::now();
        let mut voice_activity_level = 0u8;

        println!("🔴 Recording, press ENTER when done or Ctrl+C to cancel...");
        println!();
        println!("🗣️  Speak into your microphone now!");
        println!("📝 Transcription will appear below:");
        println!();

        // Simple status line that updates in place
        print!("⏱️  Recording: 0.0s | 🎙️  [░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] | 💬 ");
        io::stdout().flush().ok();

        // Create channels for user input handling using rustyline
        let (input_tx, mut input_rx) = mpsc::channel::<InputEvent>(1);

        // Spawn task to handle Enter key input using rustyline
        let input_handle = {
            let input_sender = input_tx.clone();
            tokio::spawn(async move {
                let input_future = tokio::task::spawn_blocking(move || -> InputEvent {
                    // Create a minimal rustyline editor for voice mode input
                    let mut rl = match Editor::<(), FileHistory>::new() {
                        Ok(editor) => editor,
                        Err(_) => return InputEvent::Error,
                    };

                    // Read input with rustyline - this will be consistent with main chat loop
                    match rl.readline("") {
                        Ok(_line) => {
                            // Any input (empty or not) is treated as Enter to stop recording
                            InputEvent::Enter
                        },
                        Err(ReadlineError::Interrupted | ReadlineError::Eof) => InputEvent::CtrlC,
                        Err(_) => InputEvent::Error,
                    }
                });

                match input_future.await {
                    Ok(event) => {
                        let _ = input_sender.send(event).await;
                    },
                    Err(_) => {
                        let _ = input_sender.send(InputEvent::Error).await;
                    },
                }
            })
        };

        // Process real AWS Transcribe events with simple single-line updates
        loop {
            tokio::select! {
                // Check for user input (Enter or Ctrl+C)
                input_event = input_rx.recv() => {
                    match input_event {
                        Some(InputEvent::Enter) => {
                            debug!("Enter key pressed, ending transcription");
                            break;
                        }
                        Some(InputEvent::CtrlC) => {
                            debug!("Ctrl+C pressed, cancelling transcription");
                            // Clean up tasks
                            audio_forward_handle.abort();
                            input_handle.abort();

                            // Move to new line and show cancellation message
                            println!();
                            println!();
                            println!("❌ Voice input cancelled");
                            return Ok(None);
                        }
                        Some(InputEvent::Error) | None => {
                            debug!("Input error or channel closed");
                            break;
                        }
                    }
                }

                // Check for transcript events
                transcript_result = timeout(Duration::from_millis(500), transcript_receiver.recv()) => {
                    match transcript_result {
                        Ok(Some(transcript_event)) => {
                            if transcript_event.is_partial {
                                // Show voice activity and partial results
                                voice_activity_level = 8; // High activity during speech

                                let elapsed = recording_start.elapsed().as_secs_f32();

                                Self::update_single_line(
                                    &transcript_event.transcript,
                                    elapsed,
                                    voice_activity_level,
                                );

                                // Reset silence timer on speech
                                last_speech_time = Instant::now();
                            } else {
                                // Final result - add to the continuous transcript
                                if !transcript_event.transcript.trim().is_empty() {
                                    if !current_transcript.is_empty() {
                                        current_transcript.push(' ');
                                    }
                                    current_transcript.push_str(&transcript_event.transcript);

                                    // Update display with complete transcript
                                    voice_activity_level = 3; // Medium activity for final results

                                    let elapsed = recording_start.elapsed().as_secs_f32();

                                    Self::update_single_line(
                                        &current_transcript,
                                        elapsed,
                                        voice_activity_level,
                                    );
                                }

                                // Reset silence timer
                                last_speech_time = Instant::now();
                            }
                        }
                        Ok(None) => {
                            // Transcript channel closed
                            debug!("Transcript channel closed");
                            break;
                        }
                        Err(_) => {
                            // Timeout - update display with current state
                            voice_activity_level = voice_activity_level.saturating_sub(1); // Decay activity

                            let elapsed = recording_start.elapsed().as_secs_f32();

                            Self::update_single_line(
                                &current_transcript,
                                elapsed,
                                voice_activity_level,
                            );

                            if last_speech_time.elapsed() > silence_timeout && !current_transcript.trim().is_empty() {
                                debug!("Silence timeout reached, ending transcription");
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Clean up both tasks
        audio_forward_handle.abort();
        input_handle.abort();

        // Move to new line after recording
        println!();
        println!();

        let final_transcript = current_transcript.trim().to_string();
        if final_transcript.is_empty() {
            println!("🔇 No speech detected");
            Ok(None)
        } else {
            // Present the transcript for editing/confirmation
            self.present_transcript_for_editing(final_transcript).await
        }
    }

    fn update_single_line(transcript: &str, elapsed: f32, activity_level: u8) {
        // Simple carriage return to beginning of line
        print!("\r");

        // Build the complete status line
        let bar_width = 20; // Shorter bar to fit everything on one line
        let filled = (activity_level as usize * bar_width / 10).min(bar_width);

        let mut bar = String::new();
        for i in 0..bar_width {
            if i < filled {
                bar.push('█');
            } else {
                bar.push('░');
            }
        }

        // Truncate transcript to fit on line
        let max_transcript_width = 40;
        let display_transcript = if transcript.len() <= max_transcript_width {
            transcript.to_string()
        } else {
            let start = transcript.len().saturating_sub(max_transcript_width - 3);
            format!("...{}", &transcript[start..])
        };

        // Print complete status line
        print!("⏱️  {:.1}s | 🎙️  [{}] | 💬 {}", elapsed, bar, display_transcript);

        // Clear any remaining characters from previous longer lines
        print!("\x1B[K");

        io::stdout().flush().ok();
    }

    async fn present_transcript_for_editing(&self, transcript: String) -> Result<Option<String>> {
        println!();
        println!("✅ Transcription complete!");
        println!("📝 Your transcribed text:");

        // Create properly sized box with text wrapping
        let box_width = 79;
        let wrapped_lines = Self::wrap_text_to_lines(&transcript, box_width - 4);

        // Top border
        println!("┌{}┐", "─".repeat(box_width - 2));

        // Content with proper padding
        for line in wrapped_lines {
            let padding = (box_width - 4).saturating_sub(line.len());
            println!("│ {}{} │", line, " ".repeat(padding));
        }

        // Bottom border
        println!("└{}┘", "─".repeat(box_width - 2));

        println!();
        println!("🎯 Options:");
        println!("   • Press [Enter] to submit as-is");
        println!("   • Press [e] + [Enter] to edit in external editor ($EDITOR)");
        println!("   • Press [Ctrl+C] to cancel");
        println!();
        io::stdout().flush().ok();

        // Use rustyline for consistent input handling
        let (input_tx, mut input_rx) = mpsc::channel::<Option<String>>(1);

        let input_handle = tokio::spawn(async move {
            let read_future = tokio::task::spawn_blocking(|| {
                // Create a minimal rustyline editor for transcript editing
                let mut rl = Editor::<(), FileHistory>::new().ok()?;

                match rl.readline("> ".yellow().to_string().as_str()) {
                    Ok(choice) => Some(choice.trim().to_lowercase()),
                    Err(ReadlineError::Interrupted | ReadlineError::Eof) => None,
                    Err(_) => None,
                }
            });

            match read_future.await {
                Ok(choice) => {
                    let _ = input_tx.send(choice).await;
                },
                Err(_) => {
                    let _ = input_tx.send(None).await;
                },
            }
        });

        // Wait for user input or Ctrl+C
        let choice = match input_rx.recv().await {
            Some(Some(choice)) => choice,
            Some(None) | None => {
                // Ctrl+C was pressed or input failed
                input_handle.abort();
                println!();
                println!("❌ Transcript editing cancelled");
                return Ok(None);
            },
        };

        input_handle.abort();

        if choice.is_empty() {
            // User pressed Enter without input - use original transcript
            println!("📤 Submitting original transcription...");
            Ok(Some(transcript))
        } else if choice == "e" || choice == "edit" || choice == "editor" {
            // User wants to edit in external editor
            self.launch_interactive_editor(transcript).await
        } else {
            // User typed something else - treat as replacement text
            println!("📤 Submitting your input as replacement...");
            Ok(Some(choice))
        }
    }

    async fn launch_interactive_editor(&self, transcript: String) -> Result<Option<String>> {
        use std::fs;
        use std::process::Command;

        println!();
        println!("🖊️  Opening interactive editor...");

        // Create a temporary file with the transcript
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("q_voice_edit_{}.txt", std::process::id()));

        // Write transcript to temp file
        if let Err(e) = fs::write(&temp_file, &transcript) {
            error!("Failed to create temp file: {}", e);
            println!("❌ Could not create temporary file for editing");
            println!("📤 Using original transcription...");
            return Ok(Some(transcript));
        }

        // Determine editor to use
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| {
                // Default editors by platform
                if cfg!(target_os = "windows") {
                    "notepad".to_string()
                } else {
                    "nano".to_string() // More user-friendly than vi for voice editing
                }
            });

        println!("📝 Opening {} to edit your transcription...", editor);
        println!("💡 Save and exit the editor when you're done editing");
        println!();

        // Launch editor
        let editor_result = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", &format!("{} {}", editor, temp_file.display())])
                .status()
        } else {
            Command::new("sh")
                .args(["-c", &format!("{} {}", editor, temp_file.display())])
                .status()
        };

        match editor_result {
            Ok(status) => {
                if status.success() {
                    // Read the edited content
                    match fs::read_to_string(&temp_file) {
                        Ok(edited_content) => {
                            let edited_content = edited_content.trim().to_string();

                            // Clean up temp file
                            let _ = fs::remove_file(&temp_file);

                            if edited_content.is_empty() {
                                println!("⚠️  Editor returned empty content");
                                println!("📤 Using original transcription...");
                                Ok(Some(transcript))
                            } else if edited_content == transcript {
                                println!("📝 No changes made");
                                println!("📤 Submitting original transcription...");
                                Ok(Some(transcript))
                            } else {
                                println!("✅ Edits saved successfully!");
                                println!("📤 Submitting edited version...");

                                // Show what changed (first 100 chars for brevity)
                                let preview = if edited_content.len() > 100 {
                                    format!("{}...", &edited_content[..100])
                                } else {
                                    edited_content.clone()
                                };
                                println!("📋 Edited text: {}", preview);

                                Ok(Some(edited_content))
                            }
                        },
                        Err(e) => {
                            error!("Failed to read edited file: {}", e);
                            println!("❌ Could not read edited content");
                            println!("📤 Using original transcription...");
                            let _ = fs::remove_file(&temp_file);
                            Ok(Some(transcript))
                        },
                    }
                } else {
                    println!("⚠️  Editor exited with error or was cancelled");
                    println!("📤 Using original transcription...");
                    let _ = fs::remove_file(&temp_file);
                    Ok(Some(transcript))
                }
            },
            Err(e) => {
                error!("Failed to launch editor '{}': {}", editor, e);
                println!("❌ Could not launch editor '{}'", editor);
                println!("💡 Try setting the EDITOR environment variable to your preferred editor");
                println!("📤 Using original transcription...");
                let _ = fs::remove_file(&temp_file);
                Ok(Some(transcript))
            },
        }
    }

    fn wrap_text_to_lines(text: &str, max_width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in text.split_whitespace() {
            if current_line.len() + word.len() < max_width {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            } else {
                if !current_line.is_empty() {
                    lines.push(current_line);
                }

                if word.len() <= max_width {
                    current_line = word.to_string();
                } else {
                    // Handle very long words by breaking them
                    let mut remaining = word;
                    while remaining.len() > max_width {
                        lines.push(remaining[..max_width].to_string());
                        remaining = &remaining[max_width..];
                    }
                    current_line = remaining.to_string();
                }
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // Ensure at least one line
        if lines.is_empty() {
            lines.push(String::new());
        }

        lines
    }

    pub async fn check_setup(&self) -> Result<()> {
        info!("Checking voice setup...");

        // Check microphone permissions
        match super::audio_capture::request_microphone_permission() {
            Ok(_) => {
                info!("✅ Microphone permission check passed");
            },
            Err(e) => {
                error!("❌ Microphone permission check failed: {}", e);

                // Show diagnostic information
                println!("🔧 Audio troubleshooting information:");
                if let Err(diag_err) = super::audio_capture::diagnose_audio_devices() {
                    warn!("Failed to run audio diagnostics: {}", diag_err);
                }

                return Err(VoiceError::MicrophoneUnavailable.into());
            },
        }

        // Check real AWS Transcribe permissions
        self.transcriber.check_permissions().await?;

        info!("Voice setup check completed successfully");
        Ok(())
    }
}
