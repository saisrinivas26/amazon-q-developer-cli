//! NVIDIA Parakeet-based transcription provider using NeMo models

use async_trait::async_trait;
use eyre::{eyre, Result};
use tokio::sync::mpsc;
use aws_sdk_transcribestreaming::types::AudioEvent;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use std::io::Write;
use std::path::PathBuf;

use super::transcription_provider::{TranscriptionProvider, TranscriptionResult, TranscriptEvent};
use super::common::{
    detect_voice_activity, create_wav_file, detect_python_executable,
    check_python_dependencies, is_model_cached, check_metal_support
};

pub struct ParakeetProvider {
    vad_threshold_db: f64,
    python_executable: PathBuf,
}

impl ParakeetProvider {
    pub async fn new(_language: &str) -> Result<Self> {
        // Detect Python executable
        let python_executable = detect_python_executable().await?;
        
        // Use configurable VAD threshold (default -40dB for better balance)
        let vad_threshold_db = -40.0;
        
        // Create instance
        let provider = Self {
            vad_threshold_db,
            python_executable,
        };
        
        // Check dependencies and setup model
        provider.check_dependencies().await?;
        provider.setup_model().await?;
        provider.preload_model().await?;
        
        Ok(provider)
    }

    async fn check_dependencies(&self) -> Result<()> {
        // Check required packages for NVIDIA NeMo Parakeet
        let packages = ["torch", "nemo_toolkit", "librosa", "soundfile"];
        check_python_dependencies(&self.python_executable, &packages).await.map_err(|_| {
            eyre!("NVIDIA Parakeet requires NeMo toolkit. Install with:\n  pip install nemo_toolkit[\"asr\"]")
        })?;
        
        // Check for Metal (Mac GPU) support
        check_metal_support(&self.python_executable).await;
        
        Ok(())
    }

    async fn setup_model(&self) -> Result<()> {
        // First check if model is already cached
        if self.is_model_cached().await {
            println!("✅ Using cached NVIDIA Parakeet model");
            return Ok(());
        }

        println!("🔄 Downloading NVIDIA Parakeet model (first time setup)...");
        
        // Use async progress reporting
        let (progress_tx, mut progress_rx) = mpsc::channel::<u32>(100);
        
        let progress_handle = tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                let filled = (progress * 20) / 100;
                let empty = 20 - filled;
                
                print!("\r🔄 Loading NVIDIA Parakeet model [{}{}] {}%", 
                    "█".repeat(filled as usize), 
                    "░".repeat(empty as usize), 
                    progress
                );
                std::io::stdout().flush().ok();
            }
        });

        let setup_script = r#"
import os
import sys
import logging
import warnings

# Suppress NeMo warnings during download
os.environ['NEMO_LOG_LEVEL'] = 'ERROR'
warnings.filterwarnings("ignore")

try:
    import nemo.collections.asr as nemo_asr
    from contextlib import redirect_stderr, redirect_stdout
    from io import StringIO

    print("Downloading NVIDIA Parakeet model...", file=sys.stderr)
    
    # Download and cache the model with reduced verbosity
    with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
        asr_model = nemo_asr.models.ASRModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")

    print("PARAKEET_MODEL_READY")
except Exception as e:
    print(f"ERROR:{str(e)}", file=sys.stderr)
    raise
"#;

        let result = timeout(
            Duration::from_secs(300), // 5 minutes for model download
            Command::new(&self.python_executable)
                .args(&["-c", setup_script])
                .output()
        ).await;

        let _ = progress_tx.send(100).await;
        progress_handle.await?;
        
        print!("\r");
        println!();

        match result {
            Ok(Ok(output)) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("PARAKEET_MODEL_READY") => {
                println!("✅ NVIDIA Parakeet model ready");
                Ok(())
            }
            Ok(Ok(output)) => {
                let error = String::from_utf8_lossy(&output.stderr);
                Err(eyre!("Failed to setup NVIDIA Parakeet model: {}\n\nInstall with: pip install nemo_toolkit[\"asr\"]", error))
            }
            Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
            Err(_) => Err(eyre!("NVIDIA Parakeet model setup timed out after 5 minutes")),
        }
    }

    async fn preload_model(&self) -> Result<()> {
        println!("🔄 Pre-loading NVIDIA Parakeet model...");
        
        let python_script = r#"
import torch
import sys
import logging
import os
import warnings

# Configure logging and suppress warnings
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

os.environ['NEMO_LOG_LEVEL'] = 'ERROR'
warnings.filterwarnings("ignore")

# Force CPU-only mode for stability on Mac (NeMo can have GPU issues)
device = "cpu"
logger.info("Using CPU for stable NVIDIA Parakeet speech recognition")

model_loaded = False

try:
    import nemo.collections.asr as nemo_asr
    from contextlib import redirect_stderr, redirect_stdout
    from io import StringIO
    
    # Load NVIDIA Parakeet model with CPU-only
    with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
        asr_model = nemo_asr.models.ASRModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")
        # Force CPU usage for the model
        asr_model = asr_model.to(device)
    
    globals()['cached_parakeet_model'] = asr_model
    globals()['device'] = device
    globals()['model_type'] = 'nemo_parakeet'
    model_loaded = True
    logger.info("NVIDIA Parakeet model loaded successfully on CPU")
    
except Exception as nemo_error:
    logger.error(f"NVIDIA Parakeet loading failed: {str(nemo_error)[:100]}...")
    print(f"ERROR:NVIDIA Parakeet model failed to load: {str(nemo_error)}", file=sys.stderr)
    raise Exception("NVIDIA Parakeet model failed to load")

if model_loaded:
    print("PARAKEET_MODEL_PRELOADED")
else:
    raise Exception("Failed to load NVIDIA Parakeet model")
"#;

        let result = timeout(
            Duration::from_secs(60), // Timeout for model loading
            Command::new(&self.python_executable)
                .args(&["-c", python_script])
                .output()
        ).await;

        match result {
            Ok(Ok(output)) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("PARAKEET_MODEL_PRELOADED") => {
                println!("✅ NVIDIA Parakeet model pre-loaded and ready");
                Ok(())
            }
            Ok(Ok(output)) => {
                let error = String::from_utf8_lossy(&output.stderr);
                Err(eyre!("Failed to pre-load NVIDIA Parakeet model: {}", error))
            }
            Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
            Err(_) => Err(eyre!("NVIDIA Parakeet model preloading timed out after 60 seconds")),
        }
    }

    async fn is_model_cached(&self) -> bool {
        let model_patterns = ["nvidia/parakeet-tdt-0.6b-v2"];
        is_model_cached(&self.python_executable, &model_patterns).await
    }

    async fn transcribe_with_parakeet(wav_path: &str, python_executable: &PathBuf) -> Result<String> {
        let python_script = format!(
            r#"
import sys
import logging
import os
import warnings

# Enhanced error handling
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

os.environ['NEMO_LOG_LEVEL'] = 'ERROR'
warnings.filterwarnings("ignore")

# Force CPU-only to avoid GPU memory issues
device = "cpu"

try:
    import nemo.collections.asr as nemo_asr
    from contextlib import redirect_stderr, redirect_stdout
    from io import StringIO
    
    # Load NVIDIA Parakeet model for transcription
    with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
        asr_model = nemo_asr.models.ASRModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")
        asr_model = asr_model.to(device)
    
    # Transcribe using NVIDIA Parakeet
    transcript_list = asr_model.transcribe(["{0}"])
    
    if transcript_list and len(transcript_list) > 0:
        transcript = transcript_list[0].strip()
        if transcript:
            print("TRANSCRIPT:" + transcript, flush=True)
        else:
            print("TRANSCRIPT:", flush=True)
    else:
        print("TRANSCRIPT:", flush=True)
        
except Exception as e:
    logger.error(f"NVIDIA Parakeet transcription failed: {{e}}")
    print(f"ERROR:{{str(e)}}", file=sys.stderr)
    raise
"#,
            wav_path
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
                        return Err(eyre!("NVIDIA Parakeet transcription error: {}", line.strip_prefix("ERROR:").unwrap_or("")));
                    }
                }
                
                Ok(String::new())
            }
            Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
            Err(_) => Err(eyre!("NVIDIA Parakeet transcription timed out after 30 seconds")),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ParakeetProvider {
    async fn start_transcription(&self) -> Result<TranscriptionResult> {
        let (audio_sender, mut audio_receiver) = mpsc::channel::<AudioEvent>(1000);
        let (transcript_sender, transcript_receiver) = mpsc::channel::<TranscriptEvent>(100);

        // Capture instance fields needed in the async task
        let vad_threshold_db = self.vad_threshold_db;
        let python_executable = self.python_executable.clone();

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
                                                match Self::transcribe_with_parakeet(&wav_path, &python_executable).await {
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
                                                        
                                                        println!("📝 NVIDIA PARAKEET TRANSCRIPTION: {}", final_transcript);
                                                        let _ = transcript_sender.send(TranscriptEvent {
                                                            transcript: final_transcript.clone(),
                                                            is_partial: false,
                                                        }).await;
                                                    }
                                                    Ok(_) => {
                                                        // Empty transcription - continue listening
                                                    }
                                                    Err(e) => {
                                                        eprintln!("NVIDIA Parakeet transcription error: {}", e);
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
                        match Self::transcribe_with_parakeet(&wav_path, &python_executable).await {
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
                                eprintln!("NVIDIA Parakeet transcription error: {}", e);
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

    fn supports_streaming(&self) -> bool {
        true // Parakeet TDT supports real-time streaming
    }
}
