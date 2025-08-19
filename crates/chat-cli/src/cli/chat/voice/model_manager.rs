use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

use super::error::{VoiceResult, VoiceError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_mb: u64,
    pub url: Option<String>,
    pub local_path: Option<PathBuf>,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percentage: u8,
    pub speed_mbps: f64,
}

impl DownloadProgress {
    pub fn new(model_id: String, downloaded: u64, total: u64) -> Self {
        let percentage = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0) as u8
        } else {
            0
        };

        Self {
            model_id,
            downloaded_bytes: downloaded,
            total_bytes: total,
            percentage,
            speed_mbps: 0.0,
        }
    }
}

pub struct ModelManager {
    cache_dir: PathBuf,
    models: HashMap<String, ModelInfo>,
}

impl ModelManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        let mut models = HashMap::new();
        
        // Whisper models
        models.insert("whisper-tiny".to_string(), ModelInfo {
            id: "whisper-tiny".to_string(),
            name: "Whisper Tiny".to_string(),
            description: "Fastest, basic accuracy (39 MB)".to_string(),
            size_mb: 39,
            url: Some("https://openaipublic.azureedge.net/main/whisper/models/65147644a518d12f04e32d6f3b26facc3f8dd46e5390956a9424a650c0ce22b9f/tiny.pt".to_string()),
            local_path: None,
            provider: "whisper".to_string(),
        });

        models.insert("whisper-base".to_string(), ModelInfo {
            id: "whisper-base".to_string(),
            name: "Whisper Base".to_string(),
            description: "Good balance of speed and accuracy (74 MB)".to_string(),
            size_mb: 74,
            url: Some("https://openaipublic.azureedge.net/main/whisper/models/ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e/base.pt".to_string()),
            local_path: None,
            provider: "whisper".to_string(),
        });

        // Parakeet models (downloaded via NeMo)
        models.insert("parakeet-tdt".to_string(), ModelInfo {
            id: "parakeet-tdt".to_string(),
            name: "Parakeet TDT 0.6B".to_string(),
            description: "NVIDIA Parakeet model (~600 MB)".to_string(),
            size_mb: 600,
            url: None, // Downloaded via NeMo
            local_path: None,
            provider: "parakeet".to_string(),
        });

        Self {
            cache_dir,
            models,
        }
    }

    pub async fn ensure_model(&mut self, model_id: &str) -> VoiceResult<PathBuf> {
        if let Some(model) = self.models.get(model_id) {
            if let Some(path) = &model.local_path {
                if path.exists() {
                    return Ok(path.clone());
                }
            }

            // Model not cached, download it
            let path = self.download_model(model_id).await?;
            
            // Update model info with local path
            if let Some(model) = self.models.get_mut(model_id) {
                model.local_path = Some(path.clone());
            }
            
            Ok(path)
        } else {
            Err(VoiceError::ModelNotFound(model_id.to_string()))
        }
    }

    pub fn is_cached(&self, model_id: &str) -> bool {
        if let Some(model) = self.models.get(model_id) {
            if let Some(path) = &model.local_path {
                return path.exists();
            }
        }
        false
    }

    pub fn get_model_path(&self, model_id: &str) -> Option<PathBuf> {
        self.models.get(model_id)?.local_path.clone()
    }

    pub fn list_models(&self) -> Vec<&ModelInfo> {
        self.models.values().collect()
    }

    pub fn get_models_for_provider(&self, provider: &str) -> Vec<&ModelInfo> {
        self.models.values()
            .filter(|model| model.provider == provider)
            .collect()
    }

    async fn download_model(&self, model_id: &str) -> VoiceResult<PathBuf> {
        let model = self.models.get(model_id)
            .ok_or_else(|| VoiceError::ModelNotFound(model_id.to_string()))?;

        match model.provider.as_str() {
            "whisper" => self.download_whisper_model(model).await,
            "parakeet" => self.setup_parakeet_model(model).await,
            _ => Err(VoiceError::ProviderInitFailed(
                format!("Unknown provider: {}", model.provider)
            )),
        }
    }

    async fn download_whisper_model(&self, model: &ModelInfo) -> VoiceResult<PathBuf> {
        let url = model.url.as_ref()
            .ok_or_else(|| VoiceError::ConfigError("No download URL for model".to_string()))?;

        let model_path = self.cache_dir.join(format!("{}.pt", model.id));
        
        // Create cache directory if it doesn't exist
        if let Some(parent) = model_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;
        }

        // Download with progress (simplified)
        println!("🔄 Downloading {} model...", model.name);
        
        let response = reqwest::get(url).await
            .map_err(|_e| VoiceError::NetworkError)?;
        
        let bytes = response.bytes().await
            .map_err(|_e| VoiceError::NetworkError)?;
        
        tokio::fs::write(&model_path, bytes).await
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        println!("✅ {} model downloaded successfully", model.name);
        Ok(model_path)
    }

    async fn setup_parakeet_model(&self, model: &ModelInfo) -> VoiceResult<PathBuf> {
        // Parakeet models are handled by NeMo toolkit
        // Return a placeholder path - actual model loading happens in provider
        let model_path = self.cache_dir.join(format!("{}.nemo", model.id));
        
        println!("✅ {} model will be downloaded by NeMo on first use", model.name);
        Ok(model_path)
    }

    pub async fn download_with_progress(
        &self,
        model_id: &str,
    ) -> VoiceResult<(PathBuf, mpsc::Receiver<DownloadProgress>)> {
        let (progress_tx, progress_rx) = mpsc::channel(100);
        
        let model = self.models.get(model_id)
            .ok_or_else(|| VoiceError::ModelNotFound(model_id.to_string()))?;

        // Spawn download task with progress reporting
        let model_clone = model.clone();
        let cache_dir = self.cache_dir.clone();
        
        tokio::spawn(async move {
            // Simulate progress updates
            for i in 0..=100 {
                let progress = DownloadProgress::new(
                    model_clone.id.clone(),
                    (model_clone.size_mb * 1024 * 1024 * i / 100) as u64,
                    (model_clone.size_mb * 1024 * 1024) as u64,
                );
                
                if progress_tx.send(progress).await.is_err() {
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        });

        let model_path = cache_dir.join(format!("{}.pt", model.id));
        Ok((model_path, progress_rx))
    }
}
