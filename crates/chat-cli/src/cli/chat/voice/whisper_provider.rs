//! Whisper-based transcription provider using OpenAI Whisper models

use async_trait::async_trait;
use eyre::{eyre, Result};
use tokio::sync::mpsc;
use aws_sdk_transcribestreaming::types::AudioEvent;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use std::path::PathBuf;

use super::transcription_provider::{TranscriptionProvider, TranscriptionResult, TranscriptEvent};
use super::common::{
    detect_voice_activity, create_wav_file, detect_python_executable,
    check_python_dependencies, is_model_cached, check_metal_support
};

pub struct WhisperProvider {
    language: String,
    vad_threshold_db: f64,
    python_executable: PathBuf,
}

impl WhisperProvider {
    pub async fn new(language: &str) -> Result<Self> {
        // Detect Python executable
        let python_executable = detect_python_executable().await?;
        
        // Use configurable VAD threshold (default -40dB for better balance)
        let vad_threshold_db = -40.0;
        
        // Create instance
        let provider = Self {
            language: language.to_string(),
            vad_threshold_db,
            python_executable,
        };
        
        // Check dependencies and pre-load model
        provider.check_dependencies().await?;
        provider.preload_model().await?;
        
        Ok(provider)
    }

    async fn check_dependencies(&self) -> Result<()> {
        // Check required packages for Whisper
        let packages = ["torch", "transformers", "librosa", "soundfile"];
        check_python_dependencies(&self.python_executable, &packages).await?;
        
        // Check for Metal (Mac GPU) support
        check_metal_support(&self.python_executable).await;
        
        Ok(())
    }

    async fn preload_model(&self) -> Result<()> {
        println!("🔄 Pre-loading Whisper model...");
        
        let python_script = r#"
import torch
import sys
import logging
import os

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

os.environ['TRANSFORMERS_VERBOSITY'] = 'error'

# Force CPU-only mode for stability (avoid MPS issues on Mac)
device = "cpu"
logger.info("Using CPU for stable Whisper speech recognition")

model_loaded = False

# Try OpenAI Whisper library first
try:
    import whisper
    model = whisper.load_model("base", device="cpu")
    
    globals()['cached_whisper_model'] = model
    globals()['device'] = "cpu"
    globals()['model_type'] = 'whisper'
    model_loaded = True
    logger.info("OpenAI Whisper model loaded successfully on CPU")
    
except ImportError:
    logger.info("OpenAI Whisper not available, trying Transformers...")
except Exception as whisper_error:
    logger.warning(f"Whisper loading failed: {str(whisper_error)[:100]}...")
    logger.info("Trying Transformers-based approach...")

# Fallback to transformers if direct Whisper fails
if not model_loaded:
    try:
        from transformers import pipeline
        
        # Use CPU-optimized pipeline
        pipe = pipeline(
            "automatic-speech-recognition",
            model="openai/whisper-base",
            device=-1,  # Force CPU for stability
            torch_dtype=torch.float32,
            return_timestamps=False
        )
        globals()['cached_whisper_model'] = pipe
        globals()['device'] = "cpu"
        globals()['model_type'] = 'transformers'
        model_loaded = True
        logger.info("Transformers Whisper pipeline loaded successfully on CPU")
        
    except Exception as transformers_error:
        logger.error(f"Transformers loading failed: {str(transformers_error)[:100]}...")
        print(f"ERROR:Both Whisper approaches failed to load", file=sys.stderr)
        raise Exception("Both Whisper approaches failed to load")

if model_loaded:
    print("WHISPER_MODEL_READY")
else:
    raise Exception("Failed to load Whisper model")
"#;

        let result = timeout(
            Duration::from_secs(60), // Timeout for model loading
            Command::new(&self.python_executable)
                .args(&["-c", python_script])
                .output()
        ).await;

        match result {
            Ok(Ok(output)) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("WHISPER_MODEL_READY") => {
                println!("✅ Whisper model pre-loaded and ready");
                Ok(())
            }
            Ok(Ok(output)) => {
                let error = String::from_utf8_lossy(&output.stderr);
                Err(eyre!("Failed to pre-load Whisper model: {}", error))
            }
            Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
            Err(_) => Err(eyre!("Whisper model preloading timed out after 60 seconds")),
        }
    }

    async fn is_model_cached(&self) -> bool {
        let model_patterns = ["openai/whisper-base"];
        is_model_cached(&self.python_executable, &model_patterns).await
    }

    async fn transcribe_with_whisper(wav_path: &str, python_executable: &PathBuf, language: &str) -> Result<String> {
        // Convert language code from "en-US" to "en" for Whisper
        let whisper_language = language.split('-').next().unwrap_or("en");
        
        let python_script = format!(
            r#"
import sys
import logging
import os

# Enhanced error handling
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

os.environ['TRANSFORMERS_VERBOSITY'] = 'error'

# Force CPU-only to avoid MPS logits issues
device = "cpu"
language = "{1}"

try:
    # Try OpenAI Whisper first with CPU-only for stability
    import whisper
    model = whisper.load_model("base", device="cpu")
    
    # Transcribe with CPU-only and language constraint
    result = model.transcribe("{0}", language=language)
    transcript = result.get('text', '').strip()
    
    if transcript:
        print("TRANSCRIPT:" + transcript, flush=True)
    else:
        print("TRANSCRIPT:", flush=True)
        
except ImportError:
    # Fallback to transformers with CPU-only
    try:
        from transformers import pipeline
        pipe = pipeline(
            "automatic-speech-recognition",
            model="openai/whisper-base",
            device=-1,  # Force CPU for stability
            torch_dtype=torch.float32
        )
        # Note: transformers pipeline doesn't support language parameter the same way
        # but we can try to force generate_kwargs if available
        try:
            result = pipe("{0}", generate_kwargs={{"language": language}})
        except:
            # Fallback without language constraint for transformers
            result = pipe("{0}")
        
        transcript = result.get('text', '').strip()
        
        if transcript:
            print("TRANSCRIPT:" + transcript, flush=True)
        else:
            print("TRANSCRIPT:", flush=True)
            
    except Exception as e:
        logger.error(f"Transformers transcription failed: {{e}}")
        print(f"ERROR:{{str(e)}}", file=sys.stderr)
        raise
        
except Exception as e:
    logger.error(f"Whisper transcription failed: {{e}}")
    print(f"ERROR:{{str(e)}}", file=sys.stderr)
    raise
"#,
            wav_path, whisper_language
        );

        let result = timeout(
            Duration::from_secs(30), // Timeout for transcription
            Command::new(python_executable)
                .args(&["-c", &python_script])
                .output()
        ).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                
                // Extract transcript
                for line in stdout.lines() {
                    if line.starts_with("TRANSCRIPT:") {
                        let transcript = line.strip_prefix("TRANSCRIPT:").unwrap_or("").trim();
                        return Ok(transcript.to_string());
                    } else if line.starts_with("ERROR:") {
                        return Err(eyre!("Transcription error: {}", line.strip_prefix("ERROR:").unwrap_or("")));
                    }
                }
                
                Ok(String::new())
            }
            Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
            Err(_) => Err(eyre!("Whisper transcription timed out after 30 seconds")),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for WhisperProvider {
    async fn start_transcription(&self) -> Result<TranscriptionResult> {
        let (audio_sender, mut audio_receiver) = mpsc::channel::<AudioEvent>(1000);
        let (transcript_sender, transcript_receiver) = mpsc::channel::<TranscriptEvent>(100);

        // Capture instance fields needed in the async task
        let vad_threshold_db = self.vad_threshold_db;
        let python_executable = self.python_executable.clone();
        let language = self.language.clone();

        // Stream transcription with periodic processing
        tokio::spawn(async move {
            let mut audio_buffer = Vec::new();
            let mut processed_buffer_size = 0;
            let mut last_process_time = std::time::Instant::now();
            let mut last_activity_event = std::time::Instant::now();
            let session_start_time = std::time::Instant::now();
            let process_interval = std::time::Duration::from_millis(2000);
            let activity_event_interval = std::time::Duration::from_millis(300);
            let session_timeout = std::time::Duration::from_secs(5);
            let max_session_time = std::time::Duration::from_secs(30);
            let mut final_transcript = String::new();
            let mut first_chunk = true;
            let mut has_recent_audio = false;
            let mut last_voice_activity_time = std::time::Instant::now();

            let mut timer_interval = tokio::time::interval(std::time::Duration::from_millis(100));
            timer_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Handle audio events
                    audio_event = audio_receiver.recv() => {
                        match audio_event {
                            Some(audio_event) => {
                                if let Some(chunk) = audio_event.audio_chunk() {
                                    let chunk_bytes = chunk.as_ref();
                                    
                                    // Use Voice Activity Detection
                                    let is_voice_activity = detect_voice_activity(chunk_bytes, vad_threshold_db);
                                    
                                    // Always add to buffer for transcription
                                    audio_buffer.extend_from_slice(chunk_bytes);
                                    
                                    // Only set has_recent_audio if VAD detects voice activity
                                    if is_voice_activity {
                                        has_recent_audio = true;
                                        last_voice_activity_time = std::time::Instant::now();
                                    }
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    
                    // Timer ticks for activity events and processing
                    _ = timer_interval.tick() => {
                        // Send activity events when we have recent audio
                        if has_recent_audio && last_activity_event.elapsed() >= activity_event_interval {
                            let result = transcript_sender.send(TranscriptEvent {
                                transcript: final_transcript.clone(),
                                is_partial: true,
                            }).await;
                            if result.is_err() {
                                break;
                            }
                            last_activity_event = std::time::Instant::now();
                        }
                        
                        // Process audio chunks for transcription
                        let should_process = if first_chunk {
                            last_process_time.elapsed() > std::time::Duration::from_secs(1) && !audio_buffer.is_empty()
                        } else {
                            last_process_time.elapsed() > process_interval && !audio_buffer.is_empty()
                        };
                        
                        if should_process {
                            let new_audio_size = audio_buffer.len();
                            if new_audio_size > processed_buffer_size {
                                let new_data_threshold = 16000 * 2 * 2; // ~2 seconds of audio
                                if new_audio_size - processed_buffer_size >= new_data_threshold || first_chunk {
                                    
                                    let new_audio_chunk = if first_chunk {
                                        &audio_buffer[..]
                                    } else {
                                        &audio_buffer[processed_buffer_size..]
                                    };
                                    
                                    if !new_audio_chunk.is_empty() {
                                        match create_wav_file(new_audio_chunk, 16000).await {
                                            Ok(wav_path) => {
                                                match Self::transcribe_with_whisper(&wav_path, &python_executable, &language).await {
                                                    Ok(new_text) if !new_text.trim().is_empty() => {
                                                        let cleaned_text = new_text.trim().to_string();
                                                        
                                                        // Append new text to final transcript
                                                        if first_chunk {
                                                            final_transcript = cleaned_text;
                                                        } else {
                                                            if !final_transcript.is_empty() && 
                                                               !final_transcript.ends_with('.') && 
                                                               !final_transcript.ends_with('!') && 
                                                               !final_transcript.ends_with('?') {
                                                                final_transcript.push(' ');
                                                            } else if !final_transcript.is_empty() {
                                                                final_transcript.push(' ');
                                                            }
                                                            final_transcript.push_str(&cleaned_text);
                                                        }
                                                        
                                                        println!("📝 WHISPER TRANSCRIPTION: {}", final_transcript);
                                                        let _ = transcript_sender.send(TranscriptEvent {
                                                            transcript: final_transcript.clone(),
                                                            is_partial: false,
                                                        }).await;
                                                    }
                                                    Ok(_) => {
                                                        // Empty transcription - continue listening
                                                    }
                                                    Err(e) => {
                                                        eprintln!("Whisper transcription error: {}", e);
                                                    }
                                                }
                                                let _ = std::fs::remove_file(wav_path);
                                            }
                                            Err(e) => {
                                                eprintln!("WAV file creation failed: {}", e);
                                            }
                                        }
                                    }
                                    processed_buffer_size = new_audio_size;
                                }
                            }
                            last_process_time = std::time::Instant::now();
                            first_chunk = false;
                        }

                        // Reset audio activity flag if no recent voice activity
                        if last_voice_activity_time.elapsed() > std::time::Duration::from_millis(1000) {
                            has_recent_audio = false;
                        }

                        // Check for session timeout conditions
                        let silence_time = last_voice_activity_time.elapsed();
                        let total_session_time = session_start_time.elapsed();
                        
                        if silence_time >= session_timeout || total_session_time >= max_session_time {
                            if silence_time >= session_timeout {
                                println!("🔇 No voice activity for {}s - ending voice input", silence_time.as_secs());
                            } else {
                                println!("🔇 Maximum session time reached ({}s) - ending voice input", total_session_time.as_secs());
                            }
                            break;
                        }
                    }
                }
            }

            // Send final result when recording stops
            if !final_transcript.is_empty() {
                let _ = transcript_sender.send(TranscriptEvent {
                    transcript: final_transcript,
                    is_partial: false,
                }).await;
            } else if !audio_buffer.is_empty() {
                // Process remaining audio one final time
                match create_wav_file(&audio_buffer, 16000).await {
                    Ok(wav_path) => {
                        match Self::transcribe_with_whisper(&wav_path, &python_executable, &language).await {
                            Ok(transcript) if !transcript.trim().is_empty() => {
                                let _ = transcript_sender.send(TranscriptEvent {
                                    transcript: transcript.trim().to_string(),
                                    is_partial: false,
                                }).await;
                            }
                            Ok(_) => {
                                let _ = transcript_sender.send(TranscriptEvent {
                                    transcript: "No speech detected".to_string(),
                                    is_partial: false,
                                }).await;
                            }
                            Err(e) => {
                                eprintln!("Whisper transcription error: {}", e);
                                let _ = transcript_sender.send(TranscriptEvent {
                                    transcript: "Transcription failed".to_string(),
                                    is_partial: false,
                                }).await;
                            }
                        }
                        let _ = std::fs::remove_file(wav_path);
                    }
                    Err(e) => {
                        eprintln!("Failed to create WAV file: {}", e);
                        let _ = transcript_sender.send(TranscriptEvent {
                            transcript: "Audio processing failed".to_string(),
                            is_partial: false,
                        }).await;
                    }
                }
            } else {
                let _ = transcript_sender.send(TranscriptEvent {
                    transcript: "No audio recorded".to_string(),
                    is_partial: false,
                }).await;
            }
            
            // Close the channel to signal completion
            drop(transcript_sender);
        });

        Ok(TranscriptionResult {
            audio_sender,
            transcript_receiver,
        })
    }
}
