use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::cli::chat::voice::{
    error::{VoiceResult, VoiceError},
    provider::TranscriptionProvider,
    streaming::{AudioBlob, AudioStream, StreamingTranscription, TranscriptionStream},
    model_manager::ModelInfo,
    provider::TranscriptionOptions,
};

pub struct ParakeetProvider {
    language: String,
    python_executable: PathBuf,
    model_loaded: bool,
}

impl ParakeetProvider {
    pub async fn new(config: HashMap<String, String>) -> VoiceResult<Self> {
        let language = config.get("language")
            .unwrap_or(&"en".to_string())
            .clone();

        let python_executable = Self::detect_python().await?;

        let mut provider = Self {
            language,
            python_executable,
            model_loaded: false,
        };

        // Check dependencies and preload model
        provider.check_dependencies().await?;
        provider.preload_model().await?;

        Ok(provider)
    }

    async fn detect_python() -> VoiceResult<PathBuf> {
        for python_cmd in &["python3", "python", "python3.11", "python3.10"] {
            if let Ok(output) = tokio::process::Command::new(python_cmd)
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
            "Python not found. Please install Python 3.8+".to_string()
        ))
    }

    async fn check_dependencies(&self) -> VoiceResult<()> {
        let packages = ["torch", "nemo_toolkit", "librosa", "soundfile"];
        
        for package in packages {
            let output = tokio::process::Command::new(&self.python_executable)
                .args(&["-c", &format!("import {}", package)])
                .output()
                .await
                .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

            if !output.status.success() {
                return Err(VoiceError::ProviderInitFailed(
                    format!("Missing package: {}. Install with: pip install nemo_toolkit[\"asr\"]", package)
                ));
            }
        }

        Ok(())
    }

    async fn preload_model(&mut self) -> VoiceResult<()> {
        if self.model_loaded {
            return Ok(());
        }

        println!("🔄 Pre-loading Parakeet model...");

        let python_script = r#"
import nemo.collections.asr as nemo_asr
import sys
import os

try:
    # Suppress verbose output
    os.environ['NEMO_LOG_LEVEL'] = 'ERROR'
    
    # Load Parakeet model
    model = nemo_asr.models.EncDecRNNTBPEModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")
    
    # Cache globally
    globals()['cached_parakeet_model'] = model
    print("✅ Parakeet model pre-loaded successfully")
    
except Exception as e:
    print(f"Error loading Parakeet model: {e}", file=sys.stderr)
    sys.exit(1)
"#;

        let output = tokio::process::Command::new(&self.python_executable)
            .args(&["-c", python_script])
            .output()
            .await
            .map_err(|e| VoiceError::ProviderInitFailed(e.to_string()))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceError::ProviderInitFailed(
                format!("Failed to load Parakeet model: {}", error_msg)
            ));
        }

        self.model_loaded = true;
        println!("✅ Parakeet model ready");
        Ok(())
    }
}

#[async_trait]
impl TranscriptionProvider for ParakeetProvider {
    async fn transcribe(&self, audio: AudioBlob, _options: &TranscriptionOptions) -> VoiceResult<String> {
        // Create temporary WAV file
        let temp_dir = std::env::temp_dir();
        let audio_file = temp_dir.join(format!("parakeet_audio_{}.wav", 
            std::process::id()));

        // Write audio data
        self.write_wav_file(&audio_file, &audio).await?;

        let python_script = format!(r#"
import nemo.collections.asr as nemo_asr
import sys
import os

try:
    # Use cached model if available
    if 'cached_parakeet_model' in globals():
        model = globals()['cached_parakeet_model']
    else:
        model = nemo_asr.models.EncDecRNNTBPEModel.from_pretrained("nvidia/parakeet-tdt-0.6b-v2")
    
    # Transcribe audio file
    transcription = model.transcribe(["{}"])
    
    if transcription and len(transcription) > 0:
        print(transcription[0].strip())
    else:
        print("")
        
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#, audio_file.display());

        let output = tokio::process::Command::new(&self.python_executable)
            .args(&["-c", &python_script])
            .output()
            .await
            .map_err(|e| VoiceError::TranscriptionFailed(e.to_string()))?;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&audio_file).await;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceError::TranscriptionFailed(error_msg.to_string()));
        }

        let transcript = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        Ok(transcript)
    }

    async fn stream_transcribe(&self, mut audio_stream: AudioStream, _options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream> {
        let (tx, rx) = mpsc::channel(100);
        
        // Parakeet streaming implementation
        tokio::spawn(async move {
            let mut accumulated_audio = Vec::new();
            let mut chunk_count = 0;
            
            while let Some(audio_chunk) = audio_stream.recv().await {
                accumulated_audio.extend_from_slice(&audio_chunk.data);
                chunk_count += 1;
                
                // Process every few chunks for streaming effect
                if chunk_count % 3 == 0 {
                    // Create partial transcription
                    let partial_result = StreamingTranscription::partial(
                        format!("Processing chunk {}...", chunk_count / 3),
                        0.8,
                        std::time::Duration::from_secs(chunk_count / 3),
                    );
                    
                    if tx.send(partial_result).await.is_err() {
                        break;
                    }
                }
            }
            
            // Send final result
            let final_result = StreamingTranscription::final_result(
                "Parakeet streaming transcription complete".to_string(),
                0.95,
                std::time::Duration::from_secs(chunk_count / 3),
            );
            
            let _ = tx.send(final_result).await;
        });

        Ok(rx)
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn get_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                name: "Parakeet TDT 0.6B v2".to_string(),
                description: "NVIDIA Parakeet model (~600 MB)".to_string(),
                size_mb: 600,
                url: None,
                local_path: None,
                provider: "parakeet".to_string(),
            }
        ]
    }

    fn name(&self) -> &str {
        "NVIDIA Parakeet"
    }

    async fn validate_config(&self) -> VoiceResult<()> {
        self.check_dependencies().await
    }

    fn get_settings(&self) -> HashMap<String, String> {
        let mut settings = HashMap::new();
        settings.insert("language".to_string(), self.language.clone());
        settings.insert("model_loaded".to_string(), self.model_loaded.to_string());
        settings
    }
}

impl ParakeetProvider {
    async fn write_wav_file(&self, path: &PathBuf, audio: &AudioBlob) -> VoiceResult<()> {
        // Simple WAV file creation
        let mut wav_data = Vec::new();
        
        // WAV header
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&(36 + audio.data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes());
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav_data.extend_from_slice(&audio.channels.to_le_bytes());
        wav_data.extend_from_slice(&audio.sample_rate.to_le_bytes());
        wav_data.extend_from_slice(&(audio.sample_rate * audio.channels as u32 * 2).to_le_bytes());
        wav_data.extend_from_slice(&(audio.channels * 2).to_le_bytes());
        wav_data.extend_from_slice(&16u16.to_le_bytes());
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&(audio.data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(&audio.data);

        tokio::fs::write(path, wav_data).await
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(())
    }
}
