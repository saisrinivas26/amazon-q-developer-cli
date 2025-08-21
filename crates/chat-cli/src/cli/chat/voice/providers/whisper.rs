//! Whisper-based transcription provider using OpenAI Whisper models

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::process::Command;
use tokio::time::{timeout, Duration, Instant};

use crate::cli::chat::voice::{
    error::{VoiceResult, VoiceError},
    provider::{TranscriptionProvider, TranscriptionOptions},
    streaming::{AudioStream, StreamingTranscription, TranscriptionStream},
};

pub struct WhisperProvider {
    language: String,
    python_executable: PathBuf,
    vad_threshold_db: f64,
}

impl WhisperProvider {
    pub async fn new(config: HashMap<String, String>) -> VoiceResult<Self> {
        let language = config.get("language")
            .unwrap_or(&"en".to_string())
            .clone();

        // Detect Python executable
        let python_executable = Self::detect_python_executable().await?;

        // Use configurable VAD threshold (default -45dB for better sensitivity)
        let vad_threshold_db = config.get("vad_threshold")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(-45.0);

        let provider = Self {
            language,
            python_executable,
            vad_threshold_db,
        };

        // Check dependencies and preload model
        provider.check_dependencies().await?;
        provider.preload_model().await?;

        Ok(provider)
    }

    async fn detect_python_executable() -> VoiceResult<PathBuf> {
        // Try common Python executables
        for python_cmd in &["python3", "python", "python3.11", "python3.10", "python3.9"] {
            if let Ok(output) = Command::new(python_cmd)
                .arg("--version")
                .output()
                .await
            {
                if output.status.success() {
                    return Ok(PathBuf::from(python_cmd));
                }
            }
        }

        Err(VoiceError::ProviderInitFailed(
            "Python not found. Please install Python 3.8+ for Whisper support".to_string()
        ))
    }

    async fn check_dependencies(&self) -> VoiceResult<()> {
        // Check required packages for Whisper
        let packages = ["torch", "transformers", "librosa", "soundfile"];
        
        for package in packages {
            let output = Command::new(&self.python_executable)
                .args(&["-c", &format!("import {}", package)])
                .output()
                .await
                .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

            if !output.status.success() {
                return Err(VoiceError::ProviderInitFailed(
                    format!("Missing Python package: {}. Install with: pip install {}", package, package)
                ));
            }
        }

        // Check for Metal (Mac GPU) support
        self.check_metal_support().await;

        Ok(())
    }

    async fn check_metal_support(&self) {
        let metal_check = r#"
try:
    import torch
    if hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        print("✅ Metal Performance Shaders (MPS) available for GPU acceleration")
    else:
        print("ℹ️  Using CPU for Whisper (MPS not available)")
except:
    print("ℹ️  Using CPU for Whisper")
"#;

        if let Ok(output) = Command::new(&self.python_executable)
            .args(&["-c", metal_check])
            .output()
            .await
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    println!("{}", stdout.trim());
                }
            }
        }
    }

    async fn preload_model(&self) -> VoiceResult<()> {
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
                Err(VoiceError::ProviderInitFailed(format!("Failed to pre-load Whisper model: {}", error)))
            }
            Ok(Err(e)) => Err(VoiceError::ProviderInitFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::ProviderInitFailed("Whisper model preloading timed out after 60 seconds".to_string())),
        }
    }

    // Voice Activity Detection
    fn detect_voice_activity(&self, audio_data: &[u8], threshold_db: f64) -> bool {
        if audio_data.len() < 2 {
            return false;
        }

        // Convert bytes to i16 samples
        let samples: Vec<i16> = audio_data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        if samples.is_empty() {
            return false;
        }

        // Calculate RMS (Root Mean Square) 
        let rms = (samples.iter()
            .map(|&sample| (sample as f64).powi(2))
            .sum::<f64>() / samples.len() as f64)
            .sqrt();

        // Convert to dB (avoiding log(0))
        let db = if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            -100.0 // Very quiet
        };

        db > threshold_db
    }

    async fn create_wav_file(&self, audio_data: &[u8], sample_rate: u32) -> VoiceResult<String> {
        let temp_dir = std::env::temp_dir();
        let wav_path = temp_dir.join(format!("whisper_{}.wav", std::process::id()));

        // Create WAV file with proper header
        let mut wav_data = Vec::new();
        
        // WAV header
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&(36 + audio_data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav_data.extend_from_slice(&sample_rate.to_le_bytes());
        wav_data.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav_data.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&(audio_data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(audio_data);

        tokio::fs::write(&wav_path, wav_data).await
            .map_err(|e| VoiceError::AudioProcessingError(format!("Failed to create WAV file: {}", e)))?;

        Ok(wav_path.display().to_string())
    }

    async fn transcribe_with_whisper(&self, wav_path: &str) -> VoiceResult<String> {
        // Convert language code from "en-US" to "en" for Whisper
        let whisper_language = self.language.split('-').next().unwrap_or("en");
        
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
            Command::new(&self.python_executable)
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
                        return Err(VoiceError::TranscriptionFailed(
                            format!("Whisper transcription error: {}", 
                                line.strip_prefix("ERROR:").unwrap_or(""))
                        ));
                    }
                }
                
                Ok(String::new())
            }
            Ok(Err(e)) => Err(VoiceError::TranscriptionFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::TranscriptionFailed("Whisper transcription timed out after 30 seconds".to_string())),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for WhisperProvider {
    async fn stream_transcribe(&self, mut audio_stream: AudioStream, _options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream> {
        let (tx, rx) = mpsc::channel(100);

        // Capture instance fields needed in the async task
        let vad_threshold_db = self.vad_threshold_db;
        let python_executable = self.python_executable.clone();
        let provider_self = Self {
            language: self.language.clone(),
            python_executable: python_executable.clone(),
            vad_threshold_db,
        };

        // Enhanced streaming transcription with better UI feedback
        tokio::spawn(async move {
            let mut audio_buffer = Vec::new();
            let session_start_time = Instant::now();
            let session_timeout = Duration::from_secs(5);
            let max_session_time = Duration::from_secs(120);
            let mut last_voice_activity_time = Instant::now();
            let mut last_any_audio_time = Instant::now();

            let mut timer_interval = tokio::time::interval(Duration::from_millis(100));
            timer_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // Handle audio events
                    audio_event = audio_stream.recv() => {
                        match audio_event {
                            Some(audio_blob) => {
                                let chunk_bytes = &audio_blob.data;
                                
                                // Use Voice Activity Detection
                                let is_voice_activity = provider_self.detect_voice_activity(chunk_bytes, vad_threshold_db);
                                
                                // Always add to buffer for transcription
                                audio_buffer.extend_from_slice(chunk_bytes);
                                
                                // Update any audio activity time (fallback timeout)
                                last_any_audio_time = Instant::now();
                                
                                // Update voice activity time if VAD detects activity
                                if is_voice_activity {
                                    last_voice_activity_time = Instant::now();
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    
                    // Timer ticks for timeout checks only - no UI output
                    _ = timer_interval.tick() => {
                        // Check for session timeout conditions
                        let voice_silence_time = last_voice_activity_time.elapsed();
                        let any_audio_silence_time = last_any_audio_time.elapsed();
                        let total_session_time = session_start_time.elapsed();
                        
                        let effective_silence_time = voice_silence_time.min(any_audio_silence_time + Duration::from_secs(10));
                        
                        if effective_silence_time >= session_timeout || total_session_time >= max_session_time {
                            break;
                        }
                    }
                }
            }

            // Process the audio buffer
            if !audio_buffer.is_empty() {
                match provider_self.create_wav_file(&audio_buffer, 16000).await {
                    Ok(wav_path) => {
                        match provider_self.transcribe_with_whisper(&wav_path).await {
                            Ok(transcript) if !transcript.trim().is_empty() => {
                                // Send clean transcription result without UI interference
                                let _ = tx.send(StreamingTranscription::final_result(
                                    transcript.trim().to_string(),
                                    0.95,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                            Ok(_) => {
                                let _ = tx.send(StreamingTranscription::final_result(
                                    "No speech detected".to_string(),
                                    0.5,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                            Err(_e) => {
                                let _ = tx.send(StreamingTranscription::final_result(
                                    "Transcription failed".to_string(),
                                    0.0,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                        }
                        let _ = std::fs::remove_file(wav_path);
                    }
                    Err(_e) => {
                        let _ = tx.send(StreamingTranscription::final_result(
                            "Audio processing failed".to_string(),
                            0.0,
                            session_start_time.elapsed(),
                        )).await;
                    }
                }
            } else {
                let _ = tx.send(StreamingTranscription::final_result(
                    "No audio recorded".to_string(),
                    0.0,
                    session_start_time.elapsed(),
                )).await;
            }
        });

        Ok(rx)
    }

    fn supports_streaming(&self) -> bool {
        false // Whisper is batch-only, does not support real-time streaming
    }
}
