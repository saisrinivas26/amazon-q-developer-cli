use std::time::Duration;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingTranscription {
    pub partial_text: String,
    pub confidence: f32,
    pub is_final: bool,
    pub timestamp: Duration,
    pub word_count: usize,
}

impl StreamingTranscription {
    pub fn new(text: String, is_final: bool) -> Self {
        Self {
            word_count: text.split_whitespace().count(),
            partial_text: text,
            confidence: 1.0,
            is_final,
            timestamp: Duration::from_secs(0),
        }
    }

    pub fn partial(text: String, confidence: f32, timestamp: Duration) -> Self {
        Self {
            word_count: text.split_whitespace().count(),
            partial_text: text,
            confidence,
            is_final: false,
            timestamp,
        }
    }

    pub fn final_result(text: String, confidence: f32, timestamp: Duration) -> Self {
        Self {
            word_count: text.split_whitespace().count(),
            partial_text: text,
            confidence,
            is_final: true,
            timestamp,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.partial_text.trim().is_empty()
    }

    pub fn has_content(&self) -> bool {
        self.word_count > 0
    }
}

pub type TranscriptionStream = mpsc::Receiver<StreamingTranscription>;

#[derive(Debug, Clone)]
pub struct AudioBlob {
    pub data: Vec<u8>,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: AudioFormat,
}

#[derive(Debug, Clone)]
pub enum AudioFormat {
    Wav,
    Raw,
    Flac,
}

impl AudioBlob {
    pub fn new(data: Vec<u8>, sample_rate: u32, channels: u16) -> Self {
        Self {
            data,
            sample_rate,
            channels,
            format: AudioFormat::Raw,
        }
    }

    pub fn size_mb(&self) -> f64 {
        self.data.len() as f64 / (1024.0 * 1024.0)
    }
}

pub type AudioStream = mpsc::Receiver<AudioBlob>;
