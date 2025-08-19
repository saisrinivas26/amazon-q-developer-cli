//! Common utilities for voice transcription providers

use eyre::{eyre, Result};
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use which::which;

/// Voice Activity Detection using RMS (Root Mean Square) analysis
pub fn detect_voice_activity(audio_chunk: &[u8], vad_threshold_db: f64) -> bool {
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
    
    // Convert to decibels with accurate formula
    let db = if rms > 0.0 {
        20.0 * (rms / 32768.0).log10() // Use proper reference level for 16-bit audio
    } else {
        -100.0 // Very quiet
    };

    // Use configurable VAD threshold
    db > vad_threshold_db
}

/// Create a WAV file from raw audio data
pub async fn create_wav_file(audio_data: &[u8], sample_rate: u32) -> Result<String> {
    let mut temp_file = NamedTempFile::new()?;
    
    // Write WAV header
    let num_channels = 1u16;
    let bits_per_sample = 16u16;
    let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = audio_data.len() as u32;
    let file_size = 36 + data_size;
    
    // RIFF header
    temp_file.write_all(b"RIFF")?;
    temp_file.write_all(&file_size.to_le_bytes())?;
    temp_file.write_all(b"WAVE")?;
    
    // fmt chunk
    temp_file.write_all(b"fmt ")?;
    temp_file.write_all(&16u32.to_le_bytes())?; // chunk size
    temp_file.write_all(&1u16.to_le_bytes())?; // audio format (PCM)
    temp_file.write_all(&num_channels.to_le_bytes())?;
    temp_file.write_all(&sample_rate.to_le_bytes())?;
    temp_file.write_all(&byte_rate.to_le_bytes())?;
    temp_file.write_all(&block_align.to_le_bytes())?;
    temp_file.write_all(&bits_per_sample.to_le_bytes())?;
    
    // data chunk
    temp_file.write_all(b"data")?;
    temp_file.write_all(&data_size.to_le_bytes())?;
    temp_file.write_all(audio_data)?;
    
    // Persist the temporary file and return path
    let persistent_path = temp_file.into_temp_path();
    let path_string = persistent_path.to_string_lossy().to_string();
    persistent_path.persist(&path_string)?;
    
    Ok(path_string)
}

/// Detect available Python executable dynamically
pub async fn detect_python_executable() -> Result<PathBuf> {
    // Try common Python executable names in order of preference
    let python_candidates = ["python3", "python", "py"];
    
    for candidate in &python_candidates {
        if let Ok(path) = which(candidate) {
            // Verify it's actually Python 3
            let version_check = Command::new(&path)
                .args(&["-c", "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')"])
                .output()
                .await;
            
            if let Ok(output) = version_check {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    if version.starts_with("3.") {
                        println!("✅ Using Python executable: {} (version {})", path.display(), version.trim());
                        return Ok(path);
                    }
                }
            }
        }
    }
    
    Err(eyre!("No suitable Python 3 executable found. Please install Python 3 and ensure it's in PATH."))
}

/// Check basic Python dependencies with timeout
pub async fn check_python_dependencies(python_executable: &PathBuf, packages: &[&str]) -> Result<()> {
    let import_statements = packages.join(", ");
    let python_check = timeout(
        Duration::from_secs(15), 
        Command::new(python_executable)
            .args(&["-c", &format!("import {}; print('DEPS_OK')", import_statements)])
            .output()
    ).await;

    match python_check {
        Ok(Ok(output)) if output.status.success() && 
            String::from_utf8_lossy(&output.stdout).contains("DEPS_OK") => {
            Ok(())
        }
        Ok(Err(e)) => Err(eyre!("Python command failed: {}", e)),
        Err(_) => Err(eyre!("Python dependency check timed out")),
        _ => Err(eyre!(
            "Required packages not found: {}. Install with:\n  pip install {}",
            packages.join(", "),
            packages.join(" ")
        )),
    }
}

/// Check if model is cached in HuggingFace cache
pub async fn is_model_cached(python_executable: &PathBuf, model_patterns: &[&str]) -> bool {
    let patterns_check = model_patterns
        .iter()
        .map(|pattern| format!("models--{}*", pattern.replace("/", "--")))
        .collect::<Vec<_>>()
        .join("\", \"");

    let cache_check = format!(
        r#"
import os
from pathlib import Path

cache_dir = Path.home() / ".cache" / "huggingface" / "hub"
model_patterns = ["{patterns}"]

for pattern in model_patterns:
    model_dirs = list(cache_dir.glob(pattern))
    if model_dirs:
        print("CACHED")
        exit(0)

print("NOT_CACHED")
"#,
        patterns = patterns_check
    );

    let result = timeout(
        Duration::from_secs(10),
        Command::new(python_executable)
            .args(&["-c", &cache_check])
            .output()
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).contains("CACHED")
        }
        _ => false
    }
}

/// Check for Mac Metal Performance Shaders support
pub async fn check_metal_support(python_executable: &PathBuf) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }

    let metal_check = timeout(
        Duration::from_secs(10),
        Command::new(python_executable)
            .args(&["-c", "import torch; print('METAL_AVAILABLE' if torch.backends.mps.is_available() else 'CPU_ONLY')"])
            .output()
    ).await;
    
    match metal_check {
        Ok(Ok(output)) if String::from_utf8_lossy(&output.stdout).contains("METAL_AVAILABLE") => {
            println!("✅ Mac GPU (Metal) acceleration available");
            true
        }
        _ => {
            println!("⚠️  Metal not available, using CPU");
            false
        }
    }
}

/// Check if a transcript is significantly different from the previous one
pub fn is_transcript_significantly_different(old_transcript: &str, new_transcript: &str) -> bool {
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
