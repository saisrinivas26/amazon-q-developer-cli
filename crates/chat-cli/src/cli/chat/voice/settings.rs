use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use eyre::Result;

use super::TranscriptionBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub selected_provider: TranscriptionBackend,
    pub language: String,
    pub temperature: f32,
    pub api_keys: HashMap<String, String>,
    pub model_paths: HashMap<String, String>,
    pub audio_settings: AudioSettings,
    pub streaming_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub sample_rate: u32,
    pub channels: u16,
    pub vad_threshold_db: f64,
    pub silence_timeout_ms: u64,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            selected_provider: TranscriptionBackend::AwsTranscribe,
            language: "en".to_string(),
            temperature: 0.0,
            api_keys: HashMap::new(),
            model_paths: HashMap::new(),
            audio_settings: AudioSettings::default(),
            streaming_enabled: true,
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            vad_threshold_db: -40.0,
            silence_timeout_ms: 5000,
        }
    }
}

impl VoiceSettings {
    pub fn validate_provider(&self) -> Result<()> {
        match self.selected_provider {
            TranscriptionBackend::AwsTranscribe => {
                // AWS credentials will be validated at runtime
                Ok(())
            }
            TranscriptionBackend::LocalWhisper => {
                if let Some(model_path) = self.model_paths.get("whisper") {
                    if !PathBuf::from(model_path).exists() {
                        return Err(eyre::eyre!("Whisper model not found at: {}", model_path));
                    }
                }
                Ok(())
            }
            TranscriptionBackend::LocalParakeet => {
                // Parakeet models are downloaded automatically
                Ok(())
            }
        }
    }

    pub fn get_api_key(&self, provider: &str) -> Option<&String> {
        self.api_keys.get(provider)
    }

    pub fn set_api_key(&mut self, provider: String, key: String) {
        self.api_keys.insert(provider, key);
    }

    pub fn get_model_path(&self, model: &str) -> Option<&String> {
        self.model_paths.get(model)
    }

    pub fn set_model_path(&mut self, model: String, path: String) {
        self.model_paths.insert(model, path);
    }
}
