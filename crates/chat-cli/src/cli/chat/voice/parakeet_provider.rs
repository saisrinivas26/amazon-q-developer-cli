use async_trait::async_trait;
use eyre::{eyre, Result};
use tokio::sync::mpsc;
use aws_sdk_transcribestreaming::types::AudioEvent;
use std::process::Command;
use std::io::Write;

use super::transcription_provider::{TranscriptionProvider, TranscriptionResult, TranscriptEvent};

pub struct ParakeetProvider {
    _language: String,
}

impl ParakeetProvider {
    pub async fn new(_language: &str) -> Result<Self> {
        // Check dependencies and pre-load model
        Self::check_dependencies().await?;
        Self::setup_model().await?;
        Self::preload_model().await?; // Pre-load the model now
        
        Ok(Self {
            _language: _language.to_string(),
        })
    }

    async fn check_dependencies() -> Result<()> {
        // Check if Python and required packages are available
        let python_check = Command::new("python3")
            .args(&["-c", "import torch, transformers, librosa; print('DEPS_OK')"])
            .output();

        match python_check {
            Ok(output) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("DEPS_OK") => {
                
                // Check for Mac Metal support
                if cfg!(target_os = "macos") {
                    let metal_check = Command::new("python3")
                        .args(&["-c", "import torch; print('METAL_AVAILABLE' if torch.backends.mps.is_available() else 'CPU_ONLY')"])
                        .output();
                    
                    match metal_check {
                        Ok(output) if String::from_utf8_lossy(&output.stdout).contains("METAL_AVAILABLE") => {
                            println!("✅ Mac GPU (Metal) acceleration available");
                        }
                        _ => {
                            println!("⚠️  Metal not available, using CPU");
                        }
                    }
                }
                Ok(())
            },
            _ => Err(eyre!(
                "Required packages not found. Install with:\n  pip install torch transformers librosa soundfile"
            )),
        }
    }

    async fn preload_model() -> Result<()> {
        println!("🔄 Pre-loading optimized ASR model...");
        
        let python_script = r#"
import torch
import transformers
import warnings
import os
import platform

# Suppress warnings
warnings.filterwarnings("ignore")
os.environ['TRANSFORMERS_VERBOSITY'] = 'error'

# Force CPU-only mode to avoid MPS logits issues
device = "cpu"
print("💻 Using CPU for stable speech recognition")

# Load model with CPU-only for reliability
model_loaded = False

# Try Whisper first with CPU-only
try:
    import whisper
    model = whisper.load_model("base", device="cpu")
    
    globals()['cached_model'] = model
    globals()['device'] = "cpu"
    globals()['model_type'] = 'whisper'
    model_loaded = True
    print("✅ Whisper model loaded successfully on CPU")
    
except Exception as whisper_error:
    print(f"⚠️  Whisper loading failed: {str(whisper_error)[:100]}...")
    print("🔄 Trying transformers-based approach...")

# Fallback to transformers if Whisper fails
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
        globals()['cached_model'] = pipe
        globals()['device'] = "cpu"
        globals()['model_type'] = 'transformers'
        model_loaded = True
        print("✅ Transformers pipeline loaded successfully on CPU")
        
    except Exception as transformers_error:
        print(f"❌ Transformers loading failed: {str(transformers_error)[:100]}...")
        raise Exception("Both Whisper and Transformers failed to load")

if model_loaded:
    print("MODEL_PRELOADED")
else:
    raise Exception("Failed to load any speech recognition model")
"#;

        let result = tokio::task::spawn_blocking(move || {
            Command::new("python3")
                .args(&["-c", python_script])
                .output()
        }).await??;

        if result.status.success() && String::from_utf8_lossy(&result.stdout).contains("MODEL_PRELOADED") {
            println!("✅ Optimized ASR model pre-loaded and ready");
            Ok(())
        } else {
            let error = String::from_utf8_lossy(&result.stderr);
            Err(eyre!("Failed to pre-load model: {}", error))
        }
    }

    async fn setup_model() -> Result<()> {
        // First check if model is already cached
        if Self::is_model_cached().await {
            println!("✅ Using cached Parakeet model");
            return Ok(());
        }

        println!("🔄 Downloading Parakeet model (first time setup)...");
        
        // Show progress bar only when downloading
        let (progress_tx, mut progress_rx) = mpsc::channel::<bool>(1);
        
        let progress_handle = tokio::spawn(async move {
            let mut progress = 0;
            
            loop {
                tokio::select! {
                    result = progress_rx.recv() => {
                        if result.is_some() {
                            let filled = 20;
                            print!("\r🔄 Loading model [{}] 100%", "█".repeat(filled));
                            std::io::stdout().flush().ok();
                            break;
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                        let filled = (progress * 20) / 100;
                        let empty = 20 - filled;
                        
                        print!("\r🔄 Loading model [{}{} {}%", 
                            "█".repeat(filled), 
                            "░".repeat(empty), 
                            progress
                        );
                        std::io::stdout().flush().ok();
                        
                        progress = (progress + 1).min(95);
                    }
                }
            }
        });

        let setup_script = r#"
import os
os.environ['NEMO_LOG_LEVEL'] = 'CRITICAL'
import nemo.collections.asr as nemo_asr
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO

with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
    asr_model = nemo_asr.models.ASRModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")

print("MODEL_READY")
"#;

        let result = tokio::task::spawn_blocking(move || {
            Command::new("python3")
                .args(&["-c", setup_script])
                .output()
        }).await?;

        let _ = progress_tx.send(true).await;
        progress_handle.await?;
        
        print!("\r");
        std::io::stdout().flush().ok();

        match result {
            Ok(output) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("MODEL_READY") => {
                println!("✅ Parakeet model ready");
                Ok(())
            }
            Ok(output) => {
                let error = String::from_utf8_lossy(&output.stderr);
                Err(eyre!("Failed to setup Parakeet model: {}\n\nInstall with: pip install -U nemo_toolkit[\"asr\"]", error))
            }
            Err(e) => Err(eyre!("Failed to run Python: {}", e))
        }
    }

    async fn is_model_cached() -> bool {
        let cache_check = r#"
import os
from pathlib import Path

# Check HuggingFace cache
cache_dir = Path.home() / ".cache" / "huggingface" / "hub"
model_dirs = list(cache_dir.glob("models--nvidia--parakeet-tdt-0.6b-v2*"))

if model_dirs:
    for model_dir in model_dirs:
        nemo_files = list(model_dir.rglob("*.nemo"))
        if nemo_files:
            print("CACHED")
            exit(0)

print("NOT_CACHED")
"#;

        let result = Command::new("python3")
            .args(&["-c", cache_check])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).contains("CACHED")
            }
            _ => false
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ParakeetProvider {
    async fn start_transcription(&self) -> Result<TranscriptionResult> {
        let (audio_sender, mut audio_receiver) = mpsc::channel::<AudioEvent>(1000);
        let (transcript_sender, transcript_receiver) = mpsc::channel::<TranscriptEvent>(100);

        // Stream transcription with periodic processing - mimics AWS Transcribe behavior
        tokio::spawn(async move {
            let mut audio_buffer = Vec::new();
            let mut processed_buffer_size = 0; // Track how much we've already processed
            let mut last_audio_time = std::time::Instant::now();
            let mut last_process_time = std::time::Instant::now();
            let mut last_activity_event = std::time::Instant::now();
            let session_start_time = std::time::Instant::now();
            let process_interval = std::time::Duration::from_millis(2000); // Longer interval for better chunks
            let activity_event_interval = std::time::Duration::from_millis(300); // Send activity events every 300ms
            let session_timeout = std::time::Duration::from_secs(5); // 5 second session timeout
            let max_session_time = std::time::Duration::from_secs(30); // Max 30 seconds total
            let mut final_transcript = String::new();
            let mut first_chunk = true;
            let mut has_recent_audio = false;
            let mut last_voice_activity_time = std::time::Instant::now();

            // Create a separate timer for activity events to ensure it runs consistently
            let mut timer_interval = tokio::time::interval(std::time::Duration::from_millis(100));
            timer_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    // High priority: Check for audio events
                    audio_event = audio_receiver.recv() => {
                        match audio_event {
                            Some(audio_event) => {
                                if let Some(chunk) = audio_event.audio_chunk() {
                                    let chunk_bytes = chunk.as_ref();
                                    
                                    // Use Voice Activity Detection to filter background noise
                                    let is_voice_activity = Self::detect_voice_activity(chunk_bytes);
                                    
                                    // Always add to buffer for transcription
                                    audio_buffer.extend_from_slice(chunk_bytes);
                                    last_audio_time = std::time::Instant::now();
                                    
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
                    
                    // Medium priority: Timer ticks for activity events and processing
                    _ = timer_interval.tick() => {
                        // Send activity events when we have recent audio and enough time has passed
                        if has_recent_audio && last_activity_event.elapsed() >= activity_event_interval {
                            // Send partial event to show voice activity (like AWS Transcribe does)
                            let result = transcript_sender.send(TranscriptEvent {
                                transcript: final_transcript.clone(),
                                is_partial: true,
                            }).await;
                            if result.is_err() {
                                break; // Exit if channel is closed
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
                            // Process only NEW audio data to avoid duplicates
                            let new_audio_size = audio_buffer.len();
                            if new_audio_size > processed_buffer_size {
                                let new_data_threshold = 16000 * 2 * 2; // ~2 seconds of audio
                                if new_audio_size - processed_buffer_size >= new_data_threshold || first_chunk {
                                    
                                    // Extract ONLY the new audio chunk since last processing
                                    let new_audio_chunk = if first_chunk {
                                        &audio_buffer[..] // First chunk: use all audio
                                    } else {
                                        &audio_buffer[processed_buffer_size..] // Subsequent: only new data
                                    };
                                    
                                    if !new_audio_chunk.is_empty() {
                                        match Self::create_wav_file(new_audio_chunk, 16000).await {
                                            Ok(wav_path) => {
                                                match Self::transcribe_with_parakeet(&wav_path, &transcript_sender).await {
                                                    Ok(new_text) if !new_text.trim().is_empty() => {
                                                        let cleaned_text = new_text.trim().to_string();
                                                        
                                                        // Append new text to final transcript (no deduplication needed)
                                                        if first_chunk {
                                                            final_transcript = cleaned_text;
                                                        } else {
                                                            // Add space if final_transcript doesn't end with punctuation
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
                                                        
                                                        println!("📝 TRANSCRIPTION: {}", final_transcript);
                                                        let _ = transcript_sender.send(TranscriptEvent {
                                                            transcript: final_transcript.clone(),
                                                            is_partial: false,
                                                        }).await;
                                                    }
                                                    Ok(_) => {
                                                        // Empty transcription - continue listening
                                                    }
                                                    Err(e) => {
                                                        eprintln!("Transcription error: {}", e);
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

                        // Check for session timeout conditions:
                        // 1. No voice activity for 5 seconds, OR
                        // 2. Total session time exceeds 30 seconds
                        let silence_time = last_voice_activity_time.elapsed();
                        let total_session_time = session_start_time.elapsed();
                        
                        // More aggressive timeout: trigger after 5 seconds of silence regardless of has_recent_audio
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

            // Send final result when recording stops (this triggers completion)
            if !final_transcript.is_empty() {
                let _ = transcript_sender.send(TranscriptEvent {
                    transcript: final_transcript,
                    is_partial: false, // Final result to complete transcription
                }).await;
            } else if !audio_buffer.is_empty() {
                // Process remaining audio one final time
                match Self::create_wav_file(&audio_buffer, 16000).await {
                    Ok(wav_path) => {
                        match Self::transcribe_with_parakeet(&wav_path, &transcript_sender).await {
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
                                eprintln!("Parakeet transcription error: {}", e);
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

impl ParakeetProvider {
    /// Voice Activity Detection using RMS (Root Mean Square) analysis
    fn detect_voice_activity(audio_chunk: &[u8]) -> bool {
        if audio_chunk.len() < 2 {
            return false;
        }

        // Convert bytes to i16 samples (little-endian)
        let samples: Vec<i16> = audio_chunk
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        if samples.is_empty() {
            return false;
        }

        // Calculate RMS (Root Mean Square) for volume level
        let sum_of_squares: f64 = samples
            .iter()
            .map(|&sample| (sample as f64).powi(2))
            .sum();
        
        let rms = (sum_of_squares / samples.len() as f64).sqrt();
        
        // Convert to decibels (avoid log of 0)
        let db = if rms > 0.0 {
            20.0 * (rms / i16::MAX as f64).log10()
        } else {
            -100.0 // Very quiet
        };

        // Voice activity threshold: -50dB (more sensitive to detect speech)
        // This should detect normal speech while filtering very quiet background noise
        // Adjustable based on environment
        const VOICE_THRESHOLD_DB: f64 = -50.0;
        
        db > VOICE_THRESHOLD_DB
    }

    async fn create_wav_file(audio_data: &[u8], sample_rate: u32) -> Result<String> {
        use std::fs::File;
        
        let temp_path = format!("/tmp/parakeet_audio_{}.wav", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        
        let mut file = File::create(&temp_path)?;
        
        // Write WAV header
        let num_channels = 1u16;
        let bits_per_sample = 16u16;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = audio_data.len() as u32;
        let file_size = 36 + data_size;
        
        // RIFF header
        file.write_all(b"RIFF")?;
        file.write_all(&file_size.to_le_bytes())?;
        file.write_all(b"WAVE")?;
        
        // fmt chunk
        file.write_all(b"fmt ")?;
        file.write_all(&16u32.to_le_bytes())?; // chunk size
        file.write_all(&1u16.to_le_bytes())?; // audio format (PCM)
        file.write_all(&num_channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;
        
        // data chunk
        file.write_all(b"data")?;
        file.write_all(&data_size.to_le_bytes())?;
        file.write_all(audio_data)?;
        
        Ok(temp_path)
    }
    
    /// Check if a new transcript is significantly different from the previous one
    /// This helps avoid duplicate content while allowing natural speech updates
    fn is_transcript_significantly_different(old_transcript: &str, new_transcript: &str) -> bool {
        if old_transcript.is_empty() {
            return !new_transcript.is_empty();
        }
        
        if new_transcript.is_empty() {
            return false;
        }
        
        // Simple approach: Only accept if new transcript is significantly longer (more than 5 words added)
        let old_words: Vec<&str> = old_transcript.split_whitespace().collect();
        let new_words: Vec<&str> = new_transcript.split_whitespace().collect();
        
        // Must have at least 5 more words to be considered an update
        if new_words.len() <= old_words.len() + 5 {
            return false;
        }
        
        // Check if the new transcript just contains repetitive patterns
        let new_text = new_transcript.to_lowercase();
        
        // Split into sentences and check for obvious repetition
        let sentences: Vec<&str> = new_text.split('.').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        
        if sentences.len() >= 2 {
            // Look for repeated sentences (very strict)
            for i in 0..sentences.len() {
                for j in (i + 1)..sentences.len() {
                    let sent1_words: std::collections::HashSet<&str> = sentences[i].split_whitespace().collect();
                    let sent2_words: std::collections::HashSet<&str> = sentences[j].split_whitespace().collect();
                    
                    if !sent1_words.is_empty() && !sent2_words.is_empty() {
                        let intersection = sent1_words.intersection(&sent2_words).count();
                        let similarity = intersection as f64 / sent1_words.len().min(sent2_words.len()) as f64;
                        
                        // If any two sentences are more than 70% similar, reject
                        if similarity > 0.7 {
                            return false;
                        }
                    }
                }
            }
        }
        
        // Accept if it passes all checks
        true
    }

    async fn transcribe_with_parakeet(wav_path: &str, _transcript_sender: &mpsc::Sender<TranscriptEvent>) -> Result<String> {
        let python_script = format!(
            r#"
import warnings
import os
import torch
warnings.filterwarnings("ignore")
os.environ['TRANSFORMERS_VERBOSITY'] = 'error'

# Force CPU-only to avoid MPS logits issues
device = "cpu"

model = None
try:
    # Try Whisper first with CPU-only for stability
    import whisper
    model = whisper.load_model("base", device="cpu")
    
    # Transcribe with CPU-only
    result = model.transcribe("{0}")
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
        result = pipe("{0}")
        transcript = result.get('text', '').strip()
        
        if transcript:
            print("TRANSCRIPT:" + transcript, flush=True)
        else:
            print("TRANSCRIPT:", flush=True)
            
    except Exception as e:
        print("ERROR:" + str(e), flush=True)
        
except Exception as e:
    print("ERROR:" + str(e), flush=True)
"#,
            wav_path
        );

        let result = tokio::task::spawn_blocking(move || {
            Command::new("python3")
                .args(&["-c", &python_script])
                .output()
        }).await??;

        let output = String::from_utf8_lossy(&result.stdout);
        
        // Extract transcript
        for line in output.lines() {
            if line.starts_with("TRANSCRIPT:") {
                let transcript = line.strip_prefix("TRANSCRIPT:").unwrap_or("").trim();
                return Ok(transcript.to_string());
            } else if line.starts_with("ERROR:") {
                return Err(eyre!("Transcription error: {}", line.strip_prefix("ERROR:").unwrap_or("")));
            }
        }

        Ok(String::new())
    }
}
