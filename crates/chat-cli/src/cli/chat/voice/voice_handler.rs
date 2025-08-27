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

        println!("🔄 AWS Transcribe streaming mode - real-time transcription");
        println!();
        println!("🗣️  Speak into your microphone now!");
        println!("📝 Transcription will appear below:");
        println!();

        // Simple status line that updates in place (like Whisper)
        print!("⏱️  0.0s | 🎙️  [░░░░░░░░░░░░░░░░░░░░] | 💬 ");
        io::stdout().flush().ok();
        
        // Use new streaming transcription infrastructure
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(1000);
        let _stream = self.audio_capture.start_capture(audio_tx)?;
        
        // Convert audio to streaming format
        let (blob_tx, blob_rx) = mpsc::channel(1000);
        
        // Convert raw audio to AudioBlob format with voice activity level detection
        let (vad_tx, mut vad_rx) = mpsc::channel::<u8>(100);
        tokio::spawn(async move {
            let mut audio_rx = audio_rx;
            let mut chunk_count = 0;
            let mut current_level = 0u8;
            
            while let Some(audio_chunk) = audio_rx.recv().await {
                chunk_count += 1;
                
                // Voice Activity Level Detection (0-8 scale like Whisper)
                if audio_chunk.len() >= 2 {
                    let samples: Vec<i16> = audio_chunk
                        .chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    
                    let rms = if !samples.is_empty() {
                        (samples.iter()
                            .map(|&s| (s as f64).powi(2))
                            .sum::<f64>() / samples.len() as f64)
                            .sqrt()
                    } else { 0.0 };
                    
                    let norm = rms / 32767.0_f64;
                    let db = if norm > 0.0 { 20.0 * norm.log10() } else { -100.0 };
                    
                    // Convert dB to activity level (0-8 scale like Whisper)
                    if db > -30.0 {
                        current_level = 8; // Very loud
                    } else if db > -40.0 {
                        current_level = 6; // Loud
                    } else if db > -48.0 {
                        current_level = 4; // Normal speech
                    } else if db > -60.0 {
                        current_level = 2; // Quiet
                    } else {
                        // Gradual decay for silence
                        current_level = current_level.saturating_sub(1);
                    }
                } else {
                    // No audio data, decay level
                    current_level = current_level.saturating_sub(1);
                }
                
                // Send voice activity level
                let _ = vad_tx.send(current_level).await;
                
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
        
        // Timer for regular display updates
        let mut display_timer = tokio::time::interval(Duration::from_millis(100));
        display_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        
        // Current voice activity level (0-8 scale)
        let mut current_voice_level = 0u8;
        
        loop {
            tokio::select! {
                // Handle transcription events
                transcript_result = transcript_receiver.recv() => {
                    match transcript_result {
                        Some(transcript_event) => {
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
                                } else {
                                    // Partial result - update display text but don't accumulate yet
                                    // Display will be updated by the timer tick
                                }
                            }
                        }
                        None => {
                            println!("🔚 Transcript receiver channel closed");
                            break; // Channel closed
                        }
                    }
                }
                
                // Handle voice activity detection updates
                voice_level = vad_rx.recv() => {
                    match voice_level {
                        Some(level) => {
                            current_voice_level = level;
                            if level > 0 {
                                last_activity_time = Instant::now();
                            }
                        }
                        None => {
                            // VAD channel closed
                            current_voice_level = 0;
                        }
                    }
                }
                
                // Regular display updates with real-time voice activity
                _ = display_timer.tick() => {
                    if last_update.elapsed() >= Duration::from_millis(100) {
                        let elapsed = recording_start.elapsed().as_secs_f32();
                        Self::update_single_line(&current_transcript, elapsed, current_voice_level);
                        last_update = Instant::now();
                        voice_activity_counter += 1;
                    }
                    
                    // Check silence timeout conditions
                    let speech_silence = last_speech_time.elapsed();
                    let activity_silence = last_activity_time.elapsed();
                    let session_time = recording_start.elapsed();
                    
                    // End if no speech for silence_timeout duration
                    if speech_silence >= silence_timeout {
                        println!();
                        println!("🔇 No voice activity detected - ending voice input");
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
        
        // Move to new line after single-line status updates
        println!();

        if current_transcript.trim().is_empty() {
            return Ok(None);
        }

        // Use the same boxed UI + options as Whisper/Parakeet
        return self.present_transcript_for_editing(current_transcript).await;
    }

    async fn handle_transcript_options(&self, transcript: &str) -> Result<Option<String>> {
        use std::io::{stdin, stdout, Write};
        
        loop {
            print!("> ");
            stdout().flush().ok();
            
            let mut input = String::new();
            
            match stdin().read_line(&mut input) {
                Ok(0) => {
                    // EOF (Ctrl+C/Ctrl+D)
                    println!("❌ Cancelled");
                    return Ok(None);
                },
                Ok(_) => {
                    let input = input.trim().to_lowercase();
                    debug!("Received input: '{}'", input);
                    
                    match input.as_str() {
                        "" => {
                            // Submit as-is (just Enter)
                            return Ok(Some(transcript.to_string()));
                        },
                        "e" => {
                            // Edit mode
                            println!("✏️  Opening external editor...");
                            return self.launch_interactive_editor(transcript.to_string()).await;
                        },
                        // Allow slash-commands to escape to REPL
                        s if s.starts_with('/') => {
                            println!("↪️  Exiting voice editor...");
                            return Ok(Some(input.to_string()));
                        },
                        // Quick cancels
                        "c" | "q" | "quit" | "cancel" => {
                            println!("❌ Cancelled");
                            return Ok(None);
                        },
                        _ => {
                            // Invalid input
                            println!("❌ Invalid option '{}'. Please press Enter, e, or Ctrl+C", input);
                            println!("🎯 Options:");
                            println!("   • Press [Enter] to submit as-is");
                            println!("   • Press [e] + [Enter] to edit");
                            println!("   • Press [Ctrl+C] to cancel");
                            continue;
                        }
                    }
                },
                Err(_) => {
                    // Read error (likely Ctrl+C)
                    println!("❌ Cancelled");
                    return Ok(None);
                }
            }
        }
    }

    async fn handle_transcript_options_with_cleanup(&self, transcript: &str) -> Result<Option<String>> {
        use std::io::{stdin, stdout, Write};
        
        // For batch mode, first drain any buffered stdin from the recording phase
        println!("Press any key to continue...");
        tokio::task::spawn_blocking(|| {
            use std::io::{stdin, BufRead, BufReader};
            
            let stdin = stdin();
            let mut reader = BufReader::new(stdin);
            let mut dummy = String::new();
            
            // Read and discard one line (this consumes the buffered Enter from recording)
            let _ = reader.read_line(&mut dummy);
        }).await.ok();
        
        // Now proceed with normal options handling
        self.handle_transcript_options(transcript).await
    }

    pub async fn listen_for_speech(&self) -> Result<Option<String>> {

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

        // Use raw key events to avoid buffered newlines
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};
        use crossterm::terminal::{enable_raw_mode, disable_raw_mode};

        enable_raw_mode().ok();

        let input_handle = {
            let input_sender = input_tx.clone();
            tokio::spawn(async move {
                loop {
                    if event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
                        if let Ok(Event::Key(k)) = event::read() {
                            if k.code == KeyCode::Enter {
                                let _ = input_sender.send(InputEvent::Enter).await;
                                break;
                            }
                            if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                                let _ = input_sender.send(InputEvent::CtrlC).await;
                                break;
                            }
                        }
                    }
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
                                
                                let norm = rms / 32767.0_f64;
                                let db = if norm > 0.0 { 20.0 * norm.log10() } else { -100.0 };
                                
                                last_activity_time = Instant::now();
                                if db > -48.0 {
                                    last_voice_time = Instant::now();
                                    voice_activity_level = 8;
                                } else if db > -60.0 {
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
                                println!();
                                println!("🔇 No voice activity detected - ending recording");
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
        disable_raw_mode().ok();
        
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
                    // Silent handling for no speech - clean like Whisper
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
        let (blob_tx, blob_rx) = mpsc::channel(1);
        let _ = blob_tx.send(super::streaming::AudioBlob { data: audio_data.to_vec() }).await;
        drop(blob_tx);

        let options = super::provider::TranscriptionOptions::default();
        let mut rx = self.provider.stream_transcribe(blob_rx, &options).await
            .map_err(|e| eyre::eyre!("Stream transcribe failed: {}", e))?;

        let deadline = Instant::now() + Duration::from_secs(60);

        // Keep the last partial we've seen and also capture final text when present.
        // (Some providers only populate partial_text, some only on the final event.)
        let mut last_partial = String::new();
        let mut final_text: Option<String> = None;

        while Instant::now() < deadline {
            match timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(evt)) => {
                    let p = evt.partial_text.trim();
                    if !p.is_empty() {
                        last_partial = p.to_string();
                    }
                    if evt.is_final {
                        final_text = if !p.is_empty() {
                            Some(p.to_string())
                        } else if !last_partial.is_empty() {
                            Some(last_partial.clone())
                        } else {
                            None
                        };
                        break;
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }

        Ok(final_text.unwrap_or_else(|| last_partial))
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

        // Check for auto-submit environment variable
        let autosubmit = std::env::var("Q_VOICE_AUTOSUBMIT").as_deref() == Ok("1");
        if autosubmit {
            // Skip menu if explicitly requested via environment variable
            return Ok(Some(transcript));
        }

        println!("🎯 Options:");
        println!("   • Press [Enter] to submit as-is");
        println!("   • Press [e] + [Enter] to edit");
        println!("   • Press [Ctrl+C] to cancel");
        print!("\n> ");
        io::stdout().flush().ok();

        // For batch mode, we need to drain buffered stdin from the recording phase
        self.handle_transcript_options_with_cleanup(&transcript).await
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
