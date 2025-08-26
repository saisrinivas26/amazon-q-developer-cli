
use cpal::traits::{
    DeviceTrait,
    HostTrait,
    StreamTrait,
};
use cpal::{
    Device,
    Stream,
    StreamConfig,
};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{
    debug,
    error,
    warn,
};

use super::VoiceError;

pub struct AudioCapture {
    device: Device,
    config: StreamConfig,
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(VoiceError::MicrophoneUnavailable)?;

        debug!("Using audio device: {}", device.name().unwrap_or_default());

        let supported_config = device
            .default_input_config()
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        debug!("Device default config: {:?}", supported_config);

        // Use the device's exact default configuration to avoid compatibility issues
        let config = StreamConfig {
            channels: supported_config.channels(),
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default, // Use default buffer size for compatibility
        };

        debug!("Using exact device config for maximum compatibility: {:?}", config);

        Ok(Self { device, config })
    }

    /// Resample mono f32 audio to exactly 16 kHz using linear interpolation
    /// Handles both upsampling and downsampling correctly
    fn resample_mono_to_16k(mono: &[f32], in_rate: u32) -> Vec<f32> {
        if mono.is_empty() || in_rate == 16000 { 
            return mono.to_vec(); 
        }
        
        let in_len = mono.len();
        // Target length proportional to rates (avoid zero)
        let out_len = ((in_len as u64 * 16000) / in_rate as u64).max(1) as usize;
        let scale = in_rate as f64 / 16000.0;

        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = i as f64 * scale;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;

            let s0 = mono[idx.min(in_len - 1)];
            let s1 = mono[(idx + 1).min(in_len - 1)];
            out.push(s0 + (s1 - s0) * frac);
        }
        out
    }

    pub fn start_capture(&self, audio_sender: mpsc::Sender<Vec<u8>>) -> Result<Stream> {
        let sender = audio_sender.clone();
        let config = self.config.clone();

        // Build the input stream with the device's native format
        let supported_config = self
            .device
            .default_input_config()
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        let stream = match supported_config.sample_format() {
            cpal::SampleFormat::F32 => self.build_input_stream_f32(&config, sender)?,
            cpal::SampleFormat::I16 => self.build_input_stream_i16(&config, sender)?,
            cpal::SampleFormat::U16 => self.build_input_stream_u16(&config, sender)?,
            sample_format => {
                error!("Unsupported sample format: {:?}", sample_format);
                return Err(VoiceError::UnsupportedAudioFormat.into());
            },
        };

        stream
            .play()
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        debug!("Audio capture started successfully with native device format");
        Ok(stream)
    }

    fn build_input_stream_f32(&self, config: &StreamConfig, sender: mpsc::Sender<Vec<u8>>) -> Result<Stream> {
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate.0;

        debug!("Building F32 stream: {} channels, {} Hz", channels, sample_rate);

        let stream = self
            .device
            .build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Convert to mono if needed
                    let mono_data: Vec<f32> = if channels == 1 {
                        data.to_vec()
                    } else {
                        data.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
                    };

                    // Resample to exactly 16kHz using linear interpolation
                    let resampled = Self::resample_mono_to_16k(&mono_data, sample_rate);
                    
                    // Convert resampled f32 to i16 PCM
                    let pcm_data: Vec<i16> = resampled.iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();

                    // Convert to bytes (little-endian)
                    let bytes: Vec<u8> = pcm_data.iter().flat_map(|&sample| sample.to_le_bytes()).collect();

                    if let Err(e) = sender.try_send(bytes) {
                        match e {
                            mpsc::error::TrySendError::Full(_) => {
                                warn!("Audio buffer full, dropping audio data");
                            },
                            mpsc::error::TrySendError::Closed(_) => {
                                debug!("Audio channel closed");
                            },
                        }
                    }
                },
                |err| error!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(stream)
    }

    fn build_input_stream_i16(&self, config: &StreamConfig, sender: mpsc::Sender<Vec<u8>>) -> Result<Stream> {
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate.0;

        debug!("Building I16 stream: {} channels, {} Hz", channels, sample_rate);

        let stream = self
            .device
            .build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert to mono if needed
                    let mono_i16: Vec<i16> = if channels == 1 {
                        data.to_vec()
                    } else {
                        data.chunks(channels).map(|f| {
                            (f.iter().map(|&x| x as i32).sum::<i32>() / channels as i32) as i16
                        }).collect()
                    };

                    // Convert to f32, resample, then back to i16
                    let mono_f32: Vec<f32> = mono_i16.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let resampled = Self::resample_mono_to_16k(&mono_f32, sample_rate);
                    let pcm_data: Vec<i16> = resampled.iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();

                    // Convert to bytes (little-endian)
                    let bytes: Vec<u8> = pcm_data.iter().flat_map(|&sample| sample.to_le_bytes()).collect();

                    if let Err(e) = sender.try_send(bytes) {
                        match e {
                            mpsc::error::TrySendError::Full(_) => {
                                warn!("Audio buffer full, dropping audio data");
                            },
                            mpsc::error::TrySendError::Closed(_) => {
                                debug!("Audio channel closed");
                            },
                        }
                    }
                },
                |err| error!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(stream)
    }

    fn build_input_stream_u16(&self, config: &StreamConfig, sender: mpsc::Sender<Vec<u8>>) -> Result<Stream> {
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate.0;

        debug!("Building U16 stream: {} channels, {} Hz", channels, sample_rate);

        let stream = self
            .device
            .build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    // Convert to mono if needed and convert u16 to i16
                    let mono_i16: Vec<i16> = if channels == 1 {
                        data.iter().map(|&u| (u as i32 - 32768) as i16).collect()
                    } else {
                        data.chunks(channels).map(|f| {
                            let avg = f.iter().map(|&x| x as i32).sum::<i32>() / channels as i32;
                            (avg - 32768) as i16
                        }).collect()
                    };

                    // Convert to f32, resample, then back to i16
                    let mono_f32: Vec<f32> = mono_i16.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let resampled = Self::resample_mono_to_16k(&mono_f32, sample_rate);
                    let pcm_data: Vec<i16> = resampled.iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();

                    // Convert to bytes (little-endian)
                    let bytes: Vec<u8> = pcm_data.iter().flat_map(|&sample| sample.to_le_bytes()).collect();

                    if let Err(e) = sender.try_send(bytes) {
                        match e {
                            mpsc::error::TrySendError::Full(_) => {
                                warn!("Audio buffer full, dropping audio data");
                            },
                            mpsc::error::TrySendError::Closed(_) => {
                                debug!("Audio channel closed");
                            },
                        }
                    }
                },
                |err| error!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(stream)
    }

    pub fn check_permissions() -> Result<()> {
        // Try to create a device to check permissions
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(VoiceError::MicrophoneUnavailable)?;

        // Try to get the default config to verify access
        let _config = device
            .default_input_config()
            .map_err(|e| VoiceError::AudioProcessingError(e.to_string()))?;

        Ok(())
    }

}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub fn request_microphone_permission() -> Result<bool> {
        // On macOS, we can check if we have microphone permission
        // This is a simplified check - in a real implementation,
        // you might want to use AVAudioSession APIs
        AudioCapture::check_permissions().map(|_| true)
    }
}

#[cfg(not(target_os = "macos"))]
mod other_platforms {
    use super::*;

    pub fn request_microphone_permission() -> Result<bool> {
        AudioCapture::check_permissions().map(|_| true)
    }
}

pub fn request_microphone_permission() -> Result<bool> {
    #[cfg(target_os = "macos")]
    return macos::request_microphone_permission();

    #[cfg(not(target_os = "macos"))]
    return other_platforms::request_microphone_permission();
}

pub fn diagnose_audio_devices() -> Result<()> {
    let host = cpal::default_host();

    println!("🔍 Audio Device Diagnostics:");
    println!("Host: {:?}", host.id());

    // List input devices
    match host.input_devices() {
        Ok(devices) => {
            println!("📱 Available input devices:");
            for (i, device) in devices.enumerate() {
                if let Ok(name) = device.name() {
                    println!("  {}. {}", i + 1, name);

                    // Show supported configurations
                    if let Ok(configs) = device.supported_input_configs() {
                        for config in configs {
                            println!("     - {:?}", config);
                        }
                    }
                }
            }
        },
        Err(e) => {
            println!("❌ Failed to enumerate input devices: {}", e);
        },
    }

    // Check default input device
    match host.default_input_device() {
        Some(device) => {
            if let Ok(name) = device.name() {
                println!("🎤 Default input device: {}", name);

                match device.default_input_config() {
                    Ok(config) => {
                        println!("   Default config: {:?}", config);
                        println!("   ✅ This configuration will be used for maximum compatibility");
                    },
                    Err(e) => {
                        println!("   ❌ Failed to get default config: {}", e);
                    },
                }
            }
        },
        None => {
            println!("❌ No default input device found");
        },
    }

    Ok(())
}
