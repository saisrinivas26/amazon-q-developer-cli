use std::io::{
    self,
    Write,
};
use std::time::{
    Duration,
    Instant,
};

use aws_config::SdkConfig;
use eyre::Result;

use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{
    debug,
    error,
    info,
    warn,
};

use super::{
    AudioCapture,
    VoiceError,
};
use super::transcription_provider::TranscriptionBackend;
use super::provider::TranscriptionProvider;
use super::providers::aws::AwsTranscribeProvider;
use super::providers::parakeet::ParakeetProvider;
use super::providers::whisper::WhisperProvider;
use super::voice_display::VoiceDisplay;

#[derive(Debug)]
enum InputEvent {
    Enter,
    CtrlC,
    Error,
}

pub struct VoiceHandler {
    provider: Box<dyn TranscriptionProvider + Send + Sync>,
    audio_capture: AudioCapture,
}

impl VoiceHandler {
    pub async fn new(aws_config: &SdkConfig, backend: TranscriptionBackend) -> Result<Self> {
        // Hardcoded to English ("en")
        let language = "en";
        
        let provider: Box<dyn TranscriptionProvider + Send + Sync> = match backend {
            TranscriptionBackend::AwsTranscribe => {
                Box::new(AwsTranscribeProvider::new_with_aws_config(aws_config, language).await?)
            }
            TranscriptionBackend::LocalParakeet => {
                let mut config = std::collections::HashMap::new();
                config.insert("language".to_string(), language.to_string());
                Box::new(ParakeetProvider::new(config).await?)
            }
            TranscriptionBackend::LocalWhisper => {
                let mut config = std::collections::HashMap::new();
                config.insert("language".to_string(), language.to_string());
                Box::new(WhisperProvider::new(config).await?)
            }
        };
        
        let audio_capture = AudioCapture::new()?;

        Ok(Self {
            provider,
            audio_capture,
        })
    }

    pub fn supports_streaming(&self) -> bool {
        // AWS Transcribe and Parakeet support streaming, Whisper does not
        self.provider.supports_streaming()
    }

    pub async fn listen_for_speech_streaming(&self) -> Result<Option<String>> {
        // AWS Transcribe always uses streaming - no fallback to batch mode
        if !self.supports_streaming() {
            return self.listen_for_speech().await;
        }

        // Initialize enhanced display
        let mut display = VoiceDisplay::new();
        display.start_display()?;
        
        // Use new streaming transcription infrastructure
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(1000);
        let _stream = self.audio_capture.start_capture(audio_tx)?;
        
        // Convert audio to streaming format
        let (blob_tx, blob_rx) = mpsc::channel(1000);
        
        // Convert raw audio to AudioBlob format
        tokio::spawn(async move {
            let mut audio_rx = audio_rx;
            let mut chunk_count = 0;
            while let Some(audio_chunk) = audio_rx.recv().await {
                chunk_count += 1;
                let audio_blob = super::streaming::AudioBlob {
                    data: audio_chunk,
                };
                if blob_tx.send(audio_blob).await.is_err() {
                    break;
                }
            }
            debug!("Audio forwarding ended. Total chunks: {}", chunk_count);
        });

        // Use new streaming transcription
        debug!("Starting transcription service...");
        let options = super::provider::TranscriptionOptions::default();
        let mut transcript_receiver = self.provider.stream_transcribe(blob_rx, &options).await?;
        debug!("Transcription service connected");

        let mut current_transcript = String::new();
        let mut last_update = Instant::now();
        let recording_start = Instant::now();
        let mut voice_activity_counter = 0u32;
        
        // Enhanced streaming display loop with proper silence timeout
        let mut last_speech_time = Instant::now();
        let mut last_activity_time = Instant::now();
        let silence_timeout = Duration::from_secs(5);
        let max_session_time = Duration::from_secs(30);
        
        loop {
            match timeout(Duration::from_millis(100), transcript_receiver.recv()).await {
                Ok(Some(transcript_event)) => {
                    let transcript_text = &transcript_event.partial_text;
                    debug!("Received transcription: '{}' (final: {})", transcript_text, transcript_event.is_final);
                    
                    // Update activity timers
                    last_activity_time = Instant::now();
                    if !transcript_text.trim().is_empty() {
                        last_speech_time = Instant::now();
                        
                        // Use the proven pattern from old working code
                        if transcript_event.is_final {
                            // Final result - add to the continuous transcript (like old code)
                            if !transcript_text.trim().is_empty() {
                                if !current_transcript.is_empty() {
                                    current_transcript.push(' ');
                                }
                                current_transcript.push_str(transcript_text.trim());
                                debug!("Final segment added. Complete transcript: '{}'", current_transcript);
                            }
                            
                            // Update display with complete accumulated transcript
                            let voice_active = true;
                            let confidence = 0.85 + (voice_activity_counter % 10) as f32 * 0.01;
                            
                            if last_update.elapsed() >= Duration::from_millis(100) {
                                display.update_streaming(&current_transcript, Some(confidence), voice_active)?;
                                last_update = Instant::now();
                                voice_activity_counter += 1;
                            }
                        } else {
                            // Partial result - show for real-time feedback but don't accumulate
                            let display_text = if current_transcript.is_empty() {
                                transcript_text.to_string()
                            } else {
                                format!("{} {}", current_transcript, transcript_text)
                            };
                            
                            let voice_active = true;
                            let confidence = 0.85 + (voice_activity_counter % 10) as f32 * 0.01;
                            
                            if last_update.elapsed() >= Duration::from_millis(100) {
                                display.update_streaming(&display_text, Some(confidence), voice_active)?;
                                last_update = Instant::now();
                                voice_activity_counter += 1;
                            }
                            debug!("Partial update shown: '{}'", display_text);
                        }
                    }
                },
                Ok(None) => {
                    println!("🔚 Transcript receiver channel closed");
                    break; // Channel closed
                },
                Err(_) => {
                    // Timeout - update display and check for silence timeout
                    let voice_active = false;
                    display.update_streaming(&current_transcript, Some(0.0), voice_active)?;
                    
                    // Check silence timeout conditions
                    let speech_silence = last_speech_time.elapsed();
                    let activity_silence = last_activity_time.elapsed();
                    let session_time = recording_start.elapsed();
                    
                    // End if no speech for silence_timeout duration
                    if speech_silence >= silence_timeout {
                        println!("\n🔇 No voice activity detected - ending voice input");
                        break;
                    }
                    
                    // End if no activity at all for extended period
                    if activity_silence >= silence_timeout * 2 {
                        println!("\n🔇 No audio activity detected - ending voice input");
                        break;
                    }
                    
                    // End if session exceeds maximum time
                    if session_time >= max_session_time {
                        println!("\n⏰ Maximum session time reached - ending voice input");
                        break;
                    }
                }
            }
        }

        // No audio forward handle to abort in streaming mode
        
        // Finalize display and show options
        display.finalize(&current_transcript)?;
        
        // Show complete transcription if we have content
        if !current_transcript.trim().is_empty() {
            println!();
            println!("✅ Transcription complete!");
            println!("📝 Complete transcribed text:");
            
            // Create properly sized box with text wrapping
            let box_width = 79;
            let wrapped_lines = Self::wrap_text_to_lines(&current_transcript, box_width - 4);

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
        }
        
        // Handle user input for edit options
        let final_result = self.handle_transcript_options(&current_transcript).await?;
        
        display.cleanup()?;
        
        Ok(final_result)
    }

    async fn handle_transcript_options(&self, transcript: &str) -> Result<Option<String>> {
        loop {
            // Use simple, reliable stdin reading with better error handling
            let choice = tokio::task::spawn_blocking(|| -> Result<String, String> {
                use std::io::{stdin, BufRead, BufReader};
                
                let stdin = stdin();
                let mut reader = BufReader::new(stdin);
                let mut line = String::new();
                
                match reader.read_line(&mut line) {
                    Ok(0) => Err("EOF".to_string()), // Ctrl+C or EOF
                    Ok(_) => {
                        let trimmed = line.trim().to_lowercase();
                        Ok(trimmed)
                    },
                    Err(e) => Err(format!("Read error: {}", e)),
                }
            }).await.unwrap_or_else(|_| Err("Task error".to_string()));

            match choice {
                Ok(ref input) => {
                    debug!("Received input: '{}'", input);
                    match input.as_str() {
                        "" => {
                            // Submit as-is (just Enter) - exit the loop immediately
                            return Ok(Some(transcript.to_string()));
                        },
                        "e" => {
                            // Edit mode - open external editor
                            println!("✏️  Opening external editor...");
                            return self.launch_interactive_editor(transcript.to_string()).await;
                        },
                        _ => {
                            // Invalid input - show error and continue loop
                            println!("❌ Invalid option '{}'. Please press Enter, e, or Ctrl+C", input);
                            println!("🎯 Options:");
                            println!("   • Press [Enter] to submit as-is");
                            println!("   • Press [e] + [Enter] to edit");
                            println!("   • Press [Ctrl+C] to cancel");
                            print!("\n> ");
                            io::stdout().flush().ok();
                            // Continue the loop for retry
                            continue;
                        }
                    }
                },
                Err(err) => {
                    debug!("Input error: {}", err);
                    // Ctrl+C, EOF, or error - exit the loop
                    println!("❌ Cancelled");
                    return Ok(None);
                }
            }
        }
    }

    pub async fn listen_for_speech(&self) -> Result<Option<String>> {
        println!("🎤 Voice mode activated. Speak now...");
        println!("   (Press Ctrl+C to cancel or Enter to stop recording)");
        println!();

        // Start audio capture
        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<u8>>(1000);
        let _stream = self.audio_capture.start_capture(audio_tx)?;

        // Recording UI with simple, reliable display
        let mut audio_buffer = Vec::new();
        let recording_start = Instant::now();
        let mut voice_activity_level = 0u8;

        println!("🔴 Recording, press ENTER when done or Ctrl+C to cancel...");
        println!();
        println!("🗣️  Speak into your microphone now!");
        println!("📝 Transcription will appear below:");
        println!();

        // Simple status line that updates in place
        print!("⏱️  0.0s | 🎙️  [░░░░░░░░░░░░░░░░░░░░] | 💬 ");
        io::stdout().flush().ok();

        // Create channels for user input handling
        let (input_tx, mut input_rx) = mpsc::channel::<InputEvent>(1);

        // Spawn task to handle Enter key input using simple stdin
        let input_handle = {
            let input_sender = input_tx.clone();
            tokio::spawn(async move {
                use std::io::{stdin, BufRead, BufReader};
                
                let input_future = tokio::task::spawn_blocking(move || -> InputEvent {
                    let stdin = stdin();
                    let mut reader = BufReader::new(stdin);
                    let mut line = String::new();
                    
                    match reader.read_line(&mut line) {
                        Ok(0) => InputEvent::CtrlC, // EOF indicates Ctrl+C or Ctrl+D
                        Ok(_) => InputEvent::Enter,
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

        // Batch mode silence timeout tracking
        let mut last_voice_time = Instant::now();
        let mut last_activity_time = Instant::now();
        let silence_timeout = Duration::from_secs(5); // Stop recording after 5 seconds of silence
        let max_session_time = Duration::from_secs(30); // Maximum recording time

        // Main recording loop - collect audio until Enter or timeout
        loop {
            tokio::select! {
                // Check for user input (Enter or Ctrl+C)
                input_event = input_rx.recv() => {
                    match input_event {
                        Some(InputEvent::Enter) => {
                            debug!("Enter key pressed, ending recording");
                            break;
                        }
                        Some(InputEvent::CtrlC) => {
                            debug!("Ctrl+C pressed, cancelling recording");
                            // Clean up input task
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

                // Collect audio data with silence detection
                audio_chunk = audio_rx.recv() => {
                    match audio_chunk {
                        Some(chunk) => {
                            // Add to buffer for batch processing
                            audio_buffer.extend_from_slice(&chunk);
                            
                            // Voice activity detection for display and timeout
                            if chunk.len() >= 2 {
                                let samples: Vec<i16> = chunk
                                    .chunks_exact(2)
                                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                                    .collect();
                                
                                let rms = if !samples.is_empty() {
                                    (samples.iter()
                                        .map(|&s| (s as f64).powi(2))
                                        .sum::<f64>() / samples.len() as f64)
                                        .sqrt()
                                } else { 0.0 };
                                
                                // Update activity timers based on audio energy
                                last_activity_time = Instant::now();
                                if rms > 800.0 { // Voice threshold
                                    last_voice_time = Instant::now();
                                    voice_activity_level = 8;
                                } else if rms > 400.0 {
                                    voice_activity_level = 4;
                                } else {
                                    voice_activity_level = voice_activity_level.saturating_sub(1);
                                }
                            }

                            // Update display
                            let elapsed = recording_start.elapsed().as_secs_f32();
                            Self::update_single_line("", elapsed, voice_activity_level);
                            
                            // Check for silence timeout
                            let voice_silence = last_voice_time.elapsed();
                            let activity_silence = last_activity_time.elapsed();
                            let session_time = recording_start.elapsed();
                            
                            // Auto-stop conditions for batch mode
                            if voice_silence >= silence_timeout && !audio_buffer.is_empty() {
                                debug!("Voice silence timeout reached, ending recording");
                                println!("\n🔇 No voice activity detected - ending recording");
                                break;
                            }
                            
                            if activity_silence >= silence_timeout * 2 {
                                debug!("Complete silence timeout reached, ending recording");
                                println!("\n🔇 No audio activity detected - ending recording");
                                break;
                            }
                            
                            if session_time >= max_session_time {
                                debug!("Maximum session time reached, ending recording");
                                println!("\n⏰ Maximum recording time reached - ending recording");
                                break;
                            }
                        }
                        None => {
                            debug!("Audio channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Clean up input task properly
        input_handle.abort();
        
        // Give a brief moment for task cleanup without aggressive stdin manipulation
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Move to new line after recording
        println!();

        if audio_buffer.is_empty() {
            println!("🔇 No audio recorded");
            Ok(None)
        } else {
            // Process the recorded audio with batch transcription
            println!("🔄 Processing audio...");
            
            match self.process_batch_audio(&audio_buffer).await {
                Ok(transcript) if !transcript.trim().is_empty() => {
                    // Present the transcript for editing/confirmation
                    self.present_transcript_for_editing(transcript).await
                }
                Ok(_) => {
                    println!("🔇 No speech detected");
                    Ok(None)
                }
                Err(e) => {
                    error!("Batch transcription failed: {}", e);
                    println!("❌ Transcription failed: {}", e);
                    Ok(None)
                }
            }
        }
    }

    async fn process_batch_audio(&self, audio_data: &[u8]) -> Result<String> {
        // Use the existing streaming interface for all providers
        let (blob_tx, blob_rx) = mpsc::channel(1);
        
        // Send the audio as a single blob for batch processing
        let audio_blob = super::streaming::AudioBlob {
            data: audio_data.to_vec(),
        };
        
        let _ = blob_tx.send(audio_blob).await;
        drop(blob_tx); // Close the channel to signal end of audio
        
        // Get transcription result using the standard interface
        let options = super::provider::TranscriptionOptions::default();
        let mut transcript_receiver = self.provider.stream_transcribe(blob_rx, &options).await
            .map_err(|e| eyre::eyre!("Stream transcribe failed: {}", e))?;
        
        // Wait for the final result
        match timeout(Duration::from_secs(30), transcript_receiver.recv()).await {
            Ok(Some(result)) => Ok(result.partial_text),
            Ok(None) => Ok(String::new()),
            Err(_) => Err(eyre::eyre!("Transcription timed out")),
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
        println!("   • Press [e] + [Enter] to edit");
        println!("   • Press [Ctrl+C] to cancel");
        print!("\n> ");
        io::stdout().flush().ok();

        // Handle user input for edit options
        self.handle_transcript_options(&transcript).await
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
        // TODO: Add provider-specific permission checks

        info!("Voice setup check completed successfully");
        Ok(())
    }
}
