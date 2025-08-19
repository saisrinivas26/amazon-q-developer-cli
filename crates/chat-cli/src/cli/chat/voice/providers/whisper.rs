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

pub struct WhisperProvider {
    model_path: Option<PathBuf>,
    language: String,
    python_executable: PathBuf,
}

impl WhisperProvider {
    pub async fn new(config: HashMap<String, String>) -> VoiceResult<Self> {
        let language = config.get("language")
            .unwrap_or(&"en".to_string())
            .clone();

        let model_path = config.get("model_path")
            .map(PathBuf::from);

        // Detect Python executable
        let python_executable = Self::detect_python().await?;

        let provider = Self {
            model_path,
            language,
            python_executable,
        };

        // Validate setup
        provider.check_dependencies().await?;

        Ok(provider)
    }

    async fn detect_python() -> VoiceResult<PathBuf> {
        // Try common Python executables
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
        // Check required Python packages
        let packages = ["torch", "transformers", "librosa", "soundfile"];
        
        for package in packages {
            let output = tokio::process::Command::new(&self.python_executable)
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

        Ok(())
    }
}

#[async_trait]
impl TranscriptionProvider for WhisperProvider {
    async fn transcribe(&self, audio: AudioBlob, options: &TranscriptionOptions) -> VoiceResult<String> {
        // Create temporary WAV file
        let temp_dir = std::env::temp_dir();
        let audio_file = temp_dir.join(format!("whisper_audio_{}.wav", 
            std::process::id()));

        // Write audio data to file
        self.write_wav_file(&audio_file, &audio).await?;

        // Run Whisper transcription
        let model_name = options.model.as_deref().unwrap_or("base");
        
        let python_script = format!(r#"
import whisper
import sys

try:
    model = whisper.load_model("{}")
    result = model.transcribe("{}", language="{}")
    print(result["text"].strip())
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#, model_name, audio_file.display(), self.language);

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

    async fn stream_transcribe(&self, mut _audio_stream: AudioStream, _options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream> {
        // Whisper doesn't natively support streaming, so we'll simulate it
        let (tx, rx) = mpsc::channel(100);
        
        tokio::spawn(async move {
            // Placeholder for streaming implementation
            let _ = tx.send(StreamingTranscription::new(
                "Whisper streaming not yet implemented".to_string(),
                true
            )).await;
        });

        Ok(rx)
    }

    fn supports_streaming(&self) -> bool {
        false // Whisper doesn't natively support streaming
    }

    fn get_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "whisper-tiny".to_string(),
                name: "Whisper Tiny".to_string(),
                description: "Fastest, basic accuracy (39 MB)".to_string(),
                size_mb: 39,
                url: None,
                local_path: self.model_path.clone(),
                provider: "whisper".to_string(),
            },
            ModelInfo {
                id: "whisper-base".to_string(),
                name: "Whisper Base".to_string(),
                description: "Good balance (74 MB)".to_string(),
                size_mb: 74,
                url: None,
                local_path: self.model_path.clone(),
                provider: "whisper".to_string(),
            },
        ]
    }

    fn name(&self) -> &str {
        "OpenAI Whisper"
    }

    async fn validate_config(&self) -> VoiceResult<()> {
        self.check_dependencies().await
    }

    fn get_settings(&self) -> HashMap<String, String> {
        let mut settings = HashMap::new();
        settings.insert("language".to_string(), self.language.clone());
        if let Some(path) = &self.model_path {
            settings.insert("model_path".to_string(), path.display().to_string());
        }
        settings
    }
}

impl WhisperProvider {
    async fn write_wav_file(&self, path: &PathBuf, audio: &AudioBlob) -> VoiceResult<()> {
        // Simple WAV file creation (you may want to use a proper WAV library)
        let mut wav_data = Vec::new();
        
        // WAV header (simplified)
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&(36 + audio.data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        wav_data.extend_from_slice(&audio.channels.to_le_bytes());
        wav_data.extend_from_slice(&audio.sample_rate.to_le_bytes());
        wav_data.extend_from_slice(&(audio.sample_rate * audio.channels as u32 * 2).to_le_bytes()); // byte rate
        wav_data.extend_from_slice(&(audio.channels * 2).to_le_bytes()); // block align
        wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&(audio.data.len() as u32).to_le_bytes());
        wav_data.extend_from_slice(&audio.data);

        tokio::fs::write(path, wav_data).await
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(())
    }
}
