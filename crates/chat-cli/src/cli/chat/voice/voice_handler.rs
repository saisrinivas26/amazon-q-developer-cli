use std::io::{self, Write};
use std::time::{Duration, Instant};

use aws_config::SdkConfig;
use eyre::Result;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use super::{AudioCapture, VoiceTranscriber, VoiceError};
use super::transcriber::send_audio_to_transcribe;

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
        println!("   (Press Ctrl+C to stop recording)");
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
        let audio_forward_handle = tokio::spawn(async move {
            send_audio_to_transcribe(&mut audio_rx, &transcribe_sender).await
        });
        
        // Recording UI with simple, reliable display
        let mut current_transcript = String::new();
        let mut last_speech_time = Instant::now();
        let silence_timeout = Duration::from_secs(5);
        let recording_start = Instant::now();
        let mut voice_activity_level = 0u8;
        
        println!("🔴 Recording, press ENTER when done...");
        println!();
        println!("🗣️  Speak into your microphone now!");
        println!("📝 Transcription will appear below:");
        println!();
        
        // Simple status line that updates in place
        print!("⏱️  Recording: 0.0s | 🎙️  [░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] | 💬 ");
        io::stdout().flush().ok();
        
        // Process real AWS Transcribe events with simple single-line updates
        loop {
            match timeout(Duration::from_millis(500), transcript_receiver.recv()).await {
                Ok(Some(transcript_event)) => {
                    if transcript_event.is_partial {
                        // Show voice activity and partial results
                        voice_activity_level = 8; // High activity during speech
                        
                        let elapsed = recording_start.elapsed().as_secs_f32();
                        
                        self.update_single_line(
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
                            
                            self.update_single_line(
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
                    
                    self.update_single_line(
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
        
        // Clean up
        audio_forward_handle.abort();
        
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

    fn update_single_line(&self, transcript: &str, elapsed: f32, activity_level: u8) {
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
        let wrapped_lines = self.wrap_text_to_lines(&transcript, box_width - 4);
        
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
        println!("   • Type your edits and press [Enter] to submit modified version");
        println!("   • Press [Ctrl+C] to cancel");
        println!();
        print!("✏️  Edit (or press Enter to submit): ");
        io::stdout().flush().ok();
        
        // Read user input for editing
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => {
                let edited_input = input.trim();
                if edited_input.is_empty() {
                    // User pressed Enter without editing - use original transcript
                    println!("📤 Submitting original transcription...");
                    Ok(Some(transcript))
                } else {
                    // User provided edits - use the edited version
                    println!("📤 Submitting edited version...");
                    Ok(Some(edited_input.to_string()))
                }
            }
            Err(e) => {
                error!("Failed to read user input: {}", e);
                println!("❌ Input error, using original transcription");
                Ok(Some(transcript))
            }
        }
    }

    fn wrap_text_to_lines(&self, text: &str, max_width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();
        
        for word in text.split_whitespace() {
            if current_line.len() + word.len() + 1 <= max_width {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            } else {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = String::new();
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
            }
            Err(e) => {
                error!("❌ Microphone permission check failed: {}", e);
                
                // Show diagnostic information
                println!("🔧 Audio troubleshooting information:");
                if let Err(diag_err) = super::audio_capture::diagnose_audio_devices() {
                    warn!("Failed to run audio diagnostics: {}", diag_err);
                }
                
                return Err(VoiceError::MicrophoneUnavailable.into());
            }
        }
        
        // Check real AWS Transcribe permissions
        self.transcriber.check_permissions().await?;
        
        info!("Voice setup check completed successfully");
        Ok(())
    }
}

pub async fn handle_voice_input_with_fallback(
    handler: &VoiceHandler,
    max_retries: usize,
) -> Result<Option<String>> {
    let mut attempts = 0;
    
    while attempts < max_retries {
        match handler.listen_for_speech().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempts += 1;
                warn!("Voice input attempt {} failed: {}", attempts, e);
                
                if attempts < max_retries {
                    println!("🔄 Retrying voice input... (attempt {}/{})", attempts + 1, max_retries);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    
    Ok(None)
}
