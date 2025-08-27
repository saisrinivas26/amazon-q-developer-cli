//! NVIDIA Parakeet-based transcription provider using NeMo models

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::io::Write;
use tokio::sync::mpsc;
use tokio::process::{Command, Child, ChildStdin, ChildStdout};
use tokio::time::{timeout, Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;
use serde_json;

struct PyWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

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
    python_worker: Arc<Mutex<Option<PyWorker>>>,
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

        let mut provider = Self {
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
        provider.start_worker().await?;        // NEW: keep the model hot

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
        // Always use CPU for Parakeet stability on macOS
        // Silent - no need to announce CPU usage
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

    async fn start_worker(&mut self) -> VoiceResult<()> {
        let py = &self.python_executable;

        let script = r#"
import os, sys, json, warnings, logging
from contextlib import redirect_stdout, redirect_stderr

os.environ.update({
    "NEMO_LOG_LEVEL": "ERROR",
    "TRANSFORMERS_VERBOSITY": "error",
    "HF_HUB_DISABLE_PROGRESS_BARS": "1",
    "PYTHONWARNINGS": "ignore",
    "TOKENIZERS_PARALLELISM": "false",
})

warnings.filterwarnings("ignore")
logging.basicConfig(level=logging.ERROR)

try:
    fnull = open(os.devnull, "w")
    with redirect_stdout(fnull), redirect_stderr(fnull):
        import nemo.collections.asr as nemo_asr
        m = nemo_asr.models.ASRModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2").to("cpu")
    m.eval()
except Exception as e:
    print(json.dumps({"ok": False, "phase": "load", "error": str(e)}))
    sys.stdout.flush()
    raise

print("READY"); sys.stdout.flush()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
        if req.get("cmd") == "transcribe":
            wav = req.get("wav")
            if not wav or not os.path.exists(wav):
                print(json.dumps({"ok": False, "error": "WAV not found"})); sys.stdout.flush(); continue
            outs = m.transcribe([wav], return_hypotheses=False, timestamps=False)
            text = ""
            if outs:
                text = outs[0].strip() if isinstance(outs[0], str) else (getattr(outs[0], "text", "") or str(outs[0]) or "")
                text = text.strip()
            print(json.dumps({"ok": True, "text": text})); sys.stdout.flush()
    except Exception as e:
        print(json.dumps({"ok": False, "error": str(e)})); sys.stdout.flush()
"#;

        let mut child = TokioCommand::new(py)
            .args(&["-u", "-c", script])   // -u = unbuffered
            .env("NEMO_LOG_LEVEL", "ERROR")
            .env("TRANSFORMERS_VERBOSITY", "error")
            .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
            .env("PYTHONWARNINGS", "ignore")
            .env("TOKENIZERS_PARALLELISM", "false")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| VoiceError::ProviderInitFailed("No worker stdin".into()))?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or_else(|| VoiceError::ProviderInitFailed("No worker stdout".into()))?);

        // Wait for READY - loop until we see it (ignore NeMo chatter)
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(VoiceError::ProviderInitFailed("Worker startup timeout".into()));
            }
            
            let mut line = String::new();
            let n = timeout(Duration::from_secs(5), stdout.read_line(&mut line)).await
                .map_err(|_| VoiceError::ProviderInitFailed("Worker startup read timeout".into()))?
                .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

            if n == 0 { // EOF
                return Err(VoiceError::ProviderInitFailed("Worker exited before READY".into()));
            }

            let trimmed = line.trim();
            if trimmed == "READY" {
                break; // Success!
            }

            // Log and ignore noisy lines from NeMo/HF
            eprintln!("(worker stdout noise) {}", trimmed);
        }

        *self.python_worker.lock().await = Some(PyWorker { child, stdin, stdout });
        Ok(())
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
        
        // VAD detection without debug spam

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
        eprintln!("🔍 DEBUG: Starting Parakeet transcription for: {}", wav_path);
        
        // Fast path: JSON-RPC to the hot worker
        if let Some(w) = self.python_worker.lock().await.as_mut() {
            eprintln!("🔍 DEBUG: Using hot worker path (fast transcription)");
            let req = format!("{{\"cmd\":\"transcribe\",\"wav\":\"{}\"}}\n", wav_path);
            eprintln!("🔍 DEBUG: Sending JSON request: {}", req.trim());
            
            w.stdin.write_all(req.as_bytes()).await
                .map_err(|e| VoiceError::TranscriptionFailed(e.to_string()))?;
            w.stdin.flush().await
                .map_err(|e| VoiceError::TranscriptionFailed(e.to_string()))?;

            eprintln!("🔍 DEBUG: Waiting for worker response...");
            let mut line = String::new();
            loop {
                line.clear();
                let n = timeout(Duration::from_secs(60), w.stdout.read_line(&mut line)).await
                    .map_err(|_| VoiceError::TranscriptionFailed("Worker response timeout".into()))?
                    .map_err(|e| VoiceError::TranscriptionFailed(e.to_string()))?;
                if n == 0 { 
                    return Err(VoiceError::TranscriptionFailed("Worker closed pipe".into())); 
                }

                let trimmed = line.trim();
                if trimmed.is_empty() { 
                    continue; 
                }

                eprintln!("🔍 DEBUG: Worker response: {}", trimmed);
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(v) if v.get("ok").is_some() => {
                        if v["ok"].as_bool().unwrap_or(false) {
                            let text = v["text"].as_str().unwrap_or("").to_string();
                            eprintln!("🔍 DEBUG: Hot worker success: '{}'", text);
                            return Ok(text);
                        } else {
                            let err = v["error"].as_str().unwrap_or("Unknown worker error").to_string();
                            eprintln!("🔍 DEBUG: Hot worker error: {}", err);
                            return Err(VoiceError::TranscriptionFailed(err));
                        }
                    }
                    _ => {
                        // Surface for debugging, then keep waiting
                        eprintln!("(worker stdout noise) {}", trimmed);
                        continue;
                    }
                }
            }
        }

        eprintln!("🔍 DEBUG: No hot worker available, using fallback one-shot Python");

        // Fallback: your existing one-shot Python path
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
os.environ["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
torch.set_float32_matmul_precision("high")
torch.backends.cudnn.benchmark = False
torch.set_num_threads(4)

# Force CPU-only mode to prevent MPS hanging on macOS
device = "cpu"

try:
    import nemo.collections.asr as nemo_asr
    print("DEBUG: Imported NeMo ASR", file=sys.stderr)

    audio_path = "{0}"
    print(f"DEBUG: Audio path: {{audio_path}}", file=sys.stderr)

    # Check if audio file exists and has data
    if not os.path.exists(audio_path):
        print("ERROR:Audio file not found", file=sys.stderr)
        sys.exit(1)
    file_size = os.path.getsize(audio_path)
    print(f"DEBUG: Audio file size: {{file_size}} bytes", file=sys.stderr)
    if file_size < 100:  # Arbitrary small size check
        print("DEBUG: Audio file too small, likely silent", file=sys.stderr)
        print("TRANSCRIPT:", flush=True)
        sys.exit(0)

    # Load model
    print("DEBUG: Loading model...", file=sys.stderr)
    with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
        asr_model = nemo_asr.models.ASRModel.from_pretrained({1})
    asr_model = asr_model.to(device)
    print("DEBUG: Model loaded on CPU", file=sys.stderr)

    # Transcribe (force plain strings)
    print("DEBUG: Starting transcription...", file=sys.stderr)
    outputs = asr_model.transcribe([audio_path], return_hypotheses=False, timestamps=False)
    print(f"DEBUG: Transcription outputs type={{type(outputs)}}, len={{len(outputs) if outputs else 0}}", file=sys.stderr)

    text = ""
    if outputs:
        # When return_hypotheses=False NeMo returns List[str]
        if isinstance(outputs[0], str):
            text = outputs[0].strip()
        else:
            # Fallback if toolkit changes shape
            item = outputs[0]
            text = getattr(item, "text", "") if hasattr(item, "text") else (str(item) if item else "")
            text = (text or "").strip()

    print("TRANSCRIPT:" + text, flush=True)
        
except Exception as e:
    print(f"ERROR:{{str(e)}}", file=sys.stderr)
    import traceback
    traceback.print_exc(file=sys.stderr)
    raise
"#,
            wav_path, model_source
        );

        let result = timeout(
            Duration::from_secs(240), // Increased timeout for model loading on macOS
            Command::new(&self.python_executable)
                .args(&["-c", &python_script])
                .output()
        ).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                
                // NEW: bail if Python failed (segfault/import error/etc.)
                if !output.status.success() {
                    return Err(VoiceError::TranscriptionFailed(
                        format!(
                            "Python exited with status {:?}. stderr:\n{}",
                            output.status.code(),
                            stderr.trim()
                        ),
                    ));
                }
                
                // Silent operation for clean batch mode interface
                
                // 1) Happy path - extract transcript
                if let Some(line) = stdout.lines().find(|l| l.starts_with("TRANSCRIPT:")) {
                    let transcript = line.strip_prefix("TRANSCRIPT:").unwrap_or("").trim();
                    // Remove timestamp info from final transcript
                    let clean_transcript = if let Some(bracket_pos) = transcript.find(" [Words:") {
                        &transcript[..bracket_pos]
                    } else {
                        transcript
                    };
                    return Ok(clean_transcript.to_string());
                }

                // 2) Explicit error markers in stdout or stderr
                if stdout.contains("ERROR:") || stderr.contains("ERROR:") {
                    return Err(VoiceError::TranscriptionFailed(
                        format!("Python error: {}", stderr.trim())
                    ));
                }

                // 3) NEW: Debug on fallback to empty
                eprintln!("DEBUG: Parakeet stdout: {}", stdout);
                eprintln!("DEBUG: Parakeet stderr: {}", stderr);
                Ok(String::new())
            }
            Ok(Err(e)) => Err(VoiceError::TranscriptionFailed(format!("Python command failed: {}", e))),
            Err(_) => Err(VoiceError::TranscriptionFailed("NVIDIA Parakeet transcription timed out after 240 seconds".to_string())),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for ParakeetProvider {
    async fn stream_transcribe(
        &self,
        mut audio_stream: AudioStream,
        _options: &TranscriptionOptions,
    ) -> VoiceResult<TranscriptionStream> {
        let (tx, rx) = mpsc::channel(1);

        // Capture just what we need for the batch processing
        let provider_self = Self {
            language: self.language.clone(),
            python_executable: self.python_executable.clone(),
            vad_threshold_db: self.vad_threshold_db,
            model_path: self.model_path.clone(),
            python_worker: self.python_worker.clone(), // ✅ reuse worker for session persistence
        };

        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let mut buf = Vec::<u8>::new();

            // Read all audio (pure batch mode - no live processing)
            while let Some(blob) = audio_stream.recv().await {
                buf.extend_from_slice(&blob.data);
            }

            if buf.is_empty() {
                let _ = tx.send(StreamingTranscription::final_result(
                    "No audio recorded".to_string(),
                    0.0,
                    start.elapsed(),
                )).await;
                return;
            }

            // Single transcription pass like Whisper - 16 kHz mono PCM expected
            match provider_self.create_wav_file(&buf, 16_000).await {
                Ok(wav) => {
                    let result = provider_self.transcribe_with_parakeet(&wav).await;
                    let _ = std::fs::remove_file(&wav);

                    match result {
                        Ok(t) if !t.trim().is_empty() => {
                            let _ = tx.send(StreamingTranscription::final_result(
                                t.trim().to_string(),
                                0.95, // Finite confidence for successful transcription
                                start.elapsed(),
                            )).await;
                        }
                        Ok(_) => {
                            let _ = tx.send(StreamingTranscription::final_result(
                                "No speech detected".to_string(),
                                0.5,
                                start.elapsed(),
                            )).await;
                        }
                        Err(_) => {
                            let _ = tx.send(StreamingTranscription::final_result(
                                "Transcription failed".to_string(),
                                0.0,
                                start.elapsed(),
                            )).await;
                        }
                    }
                }
                Err(_) => {
                    let _ = tx.send(StreamingTranscription::final_result(
                        "Audio processing failed".to_string(),
                        0.0,
                        start.elapsed(),
                    )).await;
                }
            }
        });

        Ok(rx)
    }

    fn supports_streaming(&self) -> bool {
        false  // Pure batch mode like Whisper
    }
}
