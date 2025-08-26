//! NVIDIA Parakeet-based transcription provider using NeMo models

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::io::Write;
use tokio::sync::mpsc;
use tokio::process::{Command, Child};
use tokio::time::{timeout, Duration};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cli::chat::voice::{
    error::{VoiceResult, VoiceError},
    provider::{TranscriptionProvider, TranscriptionOptions},
    streaming::{AudioStream, StreamingTranscription, TranscriptionStream},
};

pub struct ParakeetProvider {
    language: String,
    python_executable: PathBuf,
    vad_threshold_db: f64,
    model_path: Option<String>,
    python_worker: Arc<Mutex<Option<Child>>>,
}

impl ParakeetProvider {
    pub async fn new(config: HashMap<String, String>) -> VoiceResult<Self> {
        let language = config.get("language")
            .unwrap_or(&"en".to_string())
            .clone();

        // Detect Python executable
        let python_executable = Self::detect_python_executable().await?;
        
        // Use configurable VAD threshold (default -48dB for realistic voice detection)
        let vad_threshold_db = config.get("vad_threshold")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(-48.0);

        // Use standard HuggingFace repo ID - it will automatically use cached model if available  
        let model_path = None; // Let HuggingFace handle caching automatically

        let provider = Self {
            language,
            python_executable,
            vad_threshold_db,
            model_path,
            python_worker: Arc::new(Mutex::new(None)),
        };

        // Check dependencies and setup model
        provider.check_dependencies().await?;
        provider.setup_model().await?;
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
            "Python not found. Please install Python 3.8+ for NVIDIA Parakeet support".to_string()
        ))
    }

    async fn check_dependencies(&self) -> VoiceResult<()> {
        // Check required packages for NVIDIA NeMo Parakeet
        let packages = ["torch", "nemo", "librosa", "soundfile"];
        
        for package in packages {
            let output = Command::new(&self.python_executable)
                .args(&["-c", &format!("import {}", package)])
                .output()
                .await
                .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

            if !output.status.success() {
                return Err(VoiceError::ProviderInitFailed(
                    format!("Missing Python package: {}. Install with: pip install nemo_toolkit[\"asr\"]", package)
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
        print("ℹ️  Using CPU for Parakeet (MPS not available)")
except:
    print("ℹ️  Using CPU for Parakeet")
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

    async fn setup_model(&self) -> VoiceResult<()> {
        // Check if local model path is provided
        if let Some(model_path) = &self.model_path {
            println!("✅ Using local NVIDIA Parakeet model at: {}", model_path);
            
            // Verify the local model path exists
            if !std::path::Path::new(model_path).exists() {
                return Err(VoiceError::ProviderInitFailed(
                    format!("Local model path does not exist: {}", model_path)
                ));
            }
            
            return Ok(());
        }

        // First check if model is already cached
        if self.is_model_cached().await {
            println!("✅ Using cached NVIDIA Parakeet model");
            return Ok(());
        }

        println!("🔄 Downloading NVIDIA Parakeet model (first time setup)...");
        println!("   This may take several minutes for first-time model download and initialization...");
        
        // Simple loading spinner instead of fake progress bar
        let (stop_tx, mut stop_rx) = mpsc::channel::<bool>(1);
        
        let spinner_handle = tokio::spawn(async move {
            let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let mut i = 0;
            
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        print!("\r🔄 Loading NVIDIA Parakeet model {} Please wait...", 
                            spinner_chars[i % spinner_chars.len()]);
                        std::io::stdout().flush().ok();
                        i += 1;
                    }
                }
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

        let _ = stop_tx.send(true).await;
        spinner_handle.await.map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;
        
        print!("\r");
        std::io::stdout().flush().ok();
        println!();

        match result {
            Ok(Ok(output)) if output.status.success() && 
                String::from_utf8_lossy(&output.stdout).contains("PARAKEET_MODEL_READY") => {
                println!("✅ NVIDIA Parakeet model ready");
                Ok(())
            }
            Ok(Ok(output)) => {
                let error = String::from_utf8_lossy(&output.stderr);
                Err(VoiceError::ProviderInitFailed(
                    format!("Failed to setup NVIDIA Parakeet model: {}\n\nInstall with: pip install nemo_toolkit[\"asr\"]", error)
                ))
            }
            Ok(Err(e)) => Err(VoiceError::ProviderInitFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::ProviderInitFailed("NVIDIA Parakeet model setup timed out after 5 minutes".to_string())),
        }
    }

    async fn preload_model(&self) -> VoiceResult<()> {
        // Skip preloading if using local model path
        if self.model_path.is_some() {
            println!("✅ Skipping preload - using local model path");
            return Ok(());
        }
        
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
                Err(VoiceError::ProviderInitFailed(format!("Failed to pre-load NVIDIA Parakeet model: {}", error)))
            }
            Ok(Err(e)) => Err(VoiceError::ProviderInitFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::ProviderInitFailed("NVIDIA Parakeet model preloading timed out after 60 seconds".to_string())),
        }
    }

    async fn is_model_cached(&self) -> bool {
        let model_patterns = ["nvidia/parakeet-tdt-0.6b-v2"];
        self.check_model_cached(&model_patterns).await
    }

    async fn check_model_cached(&self, model_patterns: &[&str]) -> bool {
        for pattern in model_patterns {
            let check_script = format!(
                r#"
import os
from pathlib import Path

# Common cache directories for HuggingFace/NeMo models
cache_dirs = [
    Path.home() / ".cache" / "huggingface" / "transformers",
    Path.home() / ".cache" / "torch" / "NeMo",
    Path.home() / ".nemo",
]

model_found = False
for cache_dir in cache_dirs:
    if cache_dir.exists():
        for item in cache_dir.rglob("*"):
            if "{}" in str(item):
                model_found = True
                break
    if model_found:
        break

print("MODEL_CACHED" if model_found else "MODEL_NOT_CACHED")
"#,
                pattern
            );

            if let Ok(output) = Command::new(&self.python_executable)
                .args(&["-c", &check_script])
                .output()
                .await
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.trim() == "MODEL_CACHED" {
                        return true;
                    }
                }
            }
        }
        false
    }

    // Voice Activity Detection with debugging
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

        // Normalize RMS to 0-1 range for i16 samples (max value is 32767)
        let normalized_rms = rms / 32767.0;

        // Convert to dB (avoiding log(0))
        let db = if normalized_rms > 0.0 {
            20.0 * normalized_rms.log10()
        } else {
            -100.0 // Very quiet
        };

        let has_voice = db > threshold_db;
        
        // Debug logging every 100 chunks to avoid spam
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DEBUG_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let count = DEBUG_COUNTER.fetch_add(1, Ordering::Relaxed);
        if count % 100 == 0 {
            println!("🔊 VAD Debug: RMS={:.2}, dB={:.1}, threshold={:.1}, detected={}", 
                rms, db, threshold_db, has_voice);
        }

        has_voice
    }

    async fn create_wav_file(&self, audio_data: &[u8], sample_rate: u32) -> VoiceResult<String> {
        let temp_dir = std::env::temp_dir();
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let wav_path = temp_dir.join(format!("parakeet_{}_{}.wav", std::process::id(), ts));

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

    async fn transcribe_with_parakeet(&self, wav_path: &str) -> VoiceResult<String> {
        // Determine model source - local path or HuggingFace ID
        let model_source = if let Some(model_path) = &self.model_path {
            format!("\"{}\"", model_path)
        } else {
            "\"nvidia/parakeet-tdt-0.6b-v2\"".to_string()
        };

        let python_script = format!(
            r#"
import sys
import logging
import os
import warnings
import soundfile as sf
import numpy as np
import torch
from contextlib import redirect_stderr, redirect_stdout, nullcontext
from io import StringIO

# Logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

os.environ['NEMO_LOG_LEVEL'] = 'ERROR'
warnings.filterwarnings("ignore")

# PyTorch / device setup
torch.set_float32_matmul_precision("high")
torch.backends.cudnn.benchmark = False
torch.set_num_threads(4)

device = "cpu"
try:
    if hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        device = "mps"
        logger.info("Using Metal Performance Shaders (MPS) for GPU acceleration")
    else:
        logger.info("Using CPU for transcription")
except Exception as torch_error:
    logger.warning(f"PyTorch device check failed: {{torch_error}}")
    device = "cpu"

@torch.inference_mode()
def transcribe_audio_optimized(model, audio_path, device):
    """Optimized transcription with inference mode and AMP on MPS only"""
    amp_ctx = torch.autocast(device_type="mps", dtype=torch.float16) if device == "mps" else nullcontext()
    with amp_ctx:
        return model.transcribe([audio_path], timestamps=False)  # timestamps=False is faster

try:
    import nemo.collections.asr as nemo_asr

    # DEBUG: Check audio file before transcription
    audio_path = "{0}"
    print(f"DEBUG: Processing audio file: {{audio_path}}", file=sys.stderr)

    # Load audio (faster than librosa)
    try:
        audio_data, sample_rate = sf.read(audio_path, dtype="float32", always_2d=False)
        print(f"DEBUG: Audio loaded - duration={{len(audio_data)/sample_rate:.2f}}s, sample_rate={{sample_rate}}, samples={{len(audio_data)}}", file=sys.stderr)
        audio_rms = float(np.sqrt(np.mean(audio_data**2)))
        audio_max = float(np.max(np.abs(audio_data)))
        print(f"DEBUG: Audio levels - RMS={{audio_rms:.6f}}, Max={{audio_max:.6f}}", file=sys.stderr)
        if audio_max < 0.001:
            print("DEBUG: WARNING - Audio levels very low, may be silent", file=sys.stderr)
    except Exception as audio_error:
        print(f"DEBUG: Audio file error: {{audio_error}}", file=sys.stderr)
    
    # Load NVIDIA Parakeet model for transcription (local or remote)
    print("DEBUG: Loading Parakeet model...", file=sys.stderr)
    with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
        asr_model = nemo_asr.models.ASRModel.from_pretrained({1})
        if device != "cpu":
            try:
                asr_model = asr_model.to(device)
            except Exception as gpu_error:
                logger.warning(f"GPU setup failed, falling back to CPU: {{gpu_error}}")
                device = "cpu"
                asr_model = asr_model.to(device)
        else:
            asr_model = asr_model.to(device)
    
    print(f"DEBUG: Model loaded on {{device}}", file=sys.stderr)
    
    # Enhanced transcription with optimized function
    print("DEBUG: Starting transcription...", file=sys.stderr)
    transcript_list = transcribe_audio_optimized(asr_model, audio_path, device)
    print(f"DEBUG: Transcription completed, results={{len(transcript_list) if transcript_list else 0}}", file=sys.stderr)
    
    if transcript_list and len(transcript_list) > 0:
        result = transcript_list[0]
        print(f"DEBUG: Raw result type={{type(result)}}, content={{repr(result)}}", file=sys.stderr)
        
        transcript = result.text.strip() if hasattr(result, 'text') else str(result).strip()
        print(f"DEBUG: Extracted transcript='{{transcript}}'", file=sys.stderr)
        
        # Optional: Extract word-level timestamps if available
        timestamps_info = ""
        if hasattr(result, 'timestamp') and result.timestamp:
            if 'word' in result.timestamp:
                word_timestamps = result.timestamp['word']
                timestamps_info = f" [Words: {{len(word_timestamps)}}]"
                print(f"DEBUG: Found {{len(word_timestamps)}} word timestamps", file=sys.stderr)
        
        if transcript:
            print("TRANSCRIPT:" + transcript + timestamps_info, flush=True)
        else:
            print("TRANSCRIPT:", flush=True)
    else:
        print("DEBUG: No transcript results returned", file=sys.stderr)
        print("TRANSCRIPT:", flush=True)
        
except Exception as e:
    logger.error(f"NVIDIA Parakeet transcription failed: {{e}}")
    print(f"ERROR:{{str(e)}}", file=sys.stderr)
    raise
"#,
            wav_path, model_source
        );

        let result = timeout(
            Duration::from_secs(45), // Increased timeout for GPU acceleration
            Command::new(&self.python_executable)
                .args(&["-c", &python_script])
                .output()
        ).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                
                // Log transcription success/failure without excessive debug output
                if output.status.success() {
                    if stdout.lines().any(|line| line.starts_with("TRANSCRIPT:") && !line.strip_prefix("TRANSCRIPT:").unwrap_or("").trim().is_empty()) {
                        println!("📝 NVIDIA Parakeet transcription successful");
                    }
                } else {
                    println!("❌ NVIDIA Parakeet transcription failed with exit code: {}", output.status);
                }
                
                // Extract transcript
                for line in stdout.lines() {
                    if line.starts_with("TRANSCRIPT:") {
                        let transcript = line.strip_prefix("TRANSCRIPT:").unwrap_or("").trim();
                        // Remove timestamp info from final transcript
                        let clean_transcript = if let Some(bracket_pos) = transcript.find(" [Words:") {
                            &transcript[..bracket_pos]
                        } else {
                            transcript
                        };
                        return Ok(clean_transcript.to_string());
                    } else if line.starts_with("ERROR:") {
                        return Err(VoiceError::TranscriptionFailed(
                            format!("NVIDIA Parakeet transcription error: {}", 
                                line.strip_prefix("ERROR:").unwrap_or(""))
                        ));
                    }
                }
                
                // If we got here, check if there were any errors in stderr
                if !stderr.trim().is_empty() {
                    return Err(VoiceError::TranscriptionFailed(
                        format!("Python script failed with stderr: {}", stderr.trim())
                    ));
                }
                
                Ok(String::new())
            }
            Ok(Err(e)) => Err(VoiceError::TranscriptionFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::TranscriptionFailed("NVIDIA Parakeet transcription timed out after 45 seconds".to_string())),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ParakeetProvider {
    async fn stream_transcribe(&self, mut audio_stream: AudioStream, _options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream> {
        let (tx, rx) = mpsc::channel(100);

        // Capture instance fields needed in the async task
        let vad_threshold_db = self.vad_threshold_db;
        let python_executable = self.python_executable.clone();
        let provider_self = Self {
            language: self.language.clone(),
            python_executable: python_executable.clone(),
            vad_threshold_db,
            model_path: self.model_path.clone(),
            python_worker: Arc::new(Mutex::new(None)),
        };

        // Advanced streaming transcription with periodic processing from legacy
        tokio::spawn(async move {
            let mut audio_buffer = Vec::new();
            let mut processed_buffer_size = 0;
            let mut last_process_time = std::time::Instant::now();
            let mut last_activity_event = std::time::Instant::now();
            let session_start_time = std::time::Instant::now();
            let process_interval = std::time::Duration::from_millis(1000); // Faster processing for live streaming
            let activity_event_interval = std::time::Duration::from_millis(150); // More frequent partial updates
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
                    audio_event = audio_stream.recv() => {
                        match audio_event {
                            Some(audio_blob) => {
                                let chunk_bytes = &audio_blob.data;
                                
                                // Use Voice Activity Detection
                                let is_voice_activity = provider_self.detect_voice_activity(chunk_bytes, vad_threshold_db);
                                
                                // Always add to buffer for transcription
                                audio_buffer.extend_from_slice(chunk_bytes);
                                
                                // Debug: Log audio chunk receipt
                                use std::sync::atomic::{AtomicUsize, Ordering};
                                static CHUNK_COUNTER: AtomicUsize = AtomicUsize::new(0);
                                let count = CHUNK_COUNTER.fetch_add(1, Ordering::Relaxed);
                                if count % 50 == 0 {
                                    println!("🎤 Audio chunk #{}: size={} bytes, VAD={}, total_buffer={} bytes", 
                                        count, chunk_bytes.len(), is_voice_activity, audio_buffer.len());
                                }
                                
                                // TEMPORARY: For debugging, always consider we have recent audio
                                // This bypasses VAD to test if the transcription works
                                has_recent_audio = true;
                                last_voice_activity_time = std::time::Instant::now();
                                
                                // Also set if VAD detects voice activity
                                if is_voice_activity {
                                    println!("🔊 VAD detected voice activity!");
                                    has_recent_audio = true;
                                    last_voice_activity_time = std::time::Instant::now();
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    
                    // Timer ticks for activity events and processing
                    _ = timer_interval.tick() => {
                        // Send activity events when we have recent audio
                        if has_recent_audio && last_activity_event.elapsed() >= activity_event_interval {
                            let result = tx.send(StreamingTranscription::partial(
                                final_transcript.clone(),
                                0.8,
                                session_start_time.elapsed(),
                            )).await;
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
                                        match provider_self.create_wav_file(new_audio_chunk, 16000).await {
                                            Ok(wav_path) => {
                                                match provider_self.transcribe_with_parakeet(&wav_path).await {
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
                                                        let _ = tx.send(StreamingTranscription::new(
                                                            final_transcript.clone(),
                                                            false, // partial result
                                                        )).await;
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
            println!("🔄 RUST DEBUG: Final processing - transcript_len={}, buffer_len={}", 
                final_transcript.len(), audio_buffer.len());
                
            if !final_transcript.is_empty() {
                println!("🔄 RUST DEBUG: Using existing transcript: '{}'", final_transcript);
                let _ = tx.send(StreamingTranscription::final_result(
                    final_transcript,
                    0.95,
                    session_start_time.elapsed(),
                )).await;
            } else if !audio_buffer.is_empty() {
                println!("🔄 RUST DEBUG: Processing {} bytes of audio for final transcription", audio_buffer.len());
                
                // Process remaining audio one final time
                match provider_self.create_wav_file(&audio_buffer, 16000).await {
                    Ok(wav_path) => {
                        println!("🔄 RUST DEBUG: Created WAV file: {}", wav_path);
                        println!("🔄 RUST DEBUG: Calling transcribe_with_parakeet...");
                        
                        match provider_self.transcribe_with_parakeet(&wav_path).await {
                            Ok(transcript) if !transcript.trim().is_empty() => {
                                println!("🔄 RUST DEBUG: Transcription successful: '{}'", transcript);
                                let _ = tx.send(StreamingTranscription::final_result(
                                    transcript.trim().to_string(),
                                    0.95,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                            Ok(empty_result) => {
                                println!("🔄 RUST DEBUG: Transcription returned empty: '{}'", empty_result);
                                let _ = tx.send(StreamingTranscription::final_result(
                                    "No speech detected".to_string(),
                                    0.5,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                            Err(e) => {
                                println!("🔄 RUST DEBUG: Transcription error: {}", e);
                                eprintln!("NVIDIA Parakeet transcription error: {}", e);
                                let _ = tx.send(StreamingTranscription::final_result(
                                    "Transcription failed".to_string(),
                                    0.0,
                                    session_start_time.elapsed(),
                                )).await;
                            }
                        }
                        let _ = std::fs::remove_file(wav_path);
                    }
                    Err(e) => {
                        println!("🔄 RUST DEBUG: WAV file creation failed: {}", e);
                        eprintln!("Failed to create WAV file: {}", e);
                        let _ = tx.send(StreamingTranscription::final_result(
                            "Audio processing failed".to_string(),
                            0.0,
                            session_start_time.elapsed(),
                        )).await;
                    }
                }
            } else {
                println!("🔄 RUST DEBUG: No audio buffer to process");
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
        true
    }
}
