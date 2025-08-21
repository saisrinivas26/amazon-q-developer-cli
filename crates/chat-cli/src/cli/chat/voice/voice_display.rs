use std::io::{self, Write};
use std::time::Instant;
use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType},
};

pub struct VoiceDisplay {
    start_time: Instant,
    voice_activity: Vec<bool>,
    confidence_history: Vec<f32>,
    current_transcript: String,
    is_final: bool,
}

impl VoiceDisplay {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            voice_activity: Vec::new(),
            confidence_history: Vec::new(),
            current_transcript: String::new(),
            is_final: false,
        }
    }

    pub fn start_display(&mut self) -> io::Result<()> {
        // Clear screen and setup display area
        execute!(
            io::stdout(),
            Clear(ClearType::All),
            cursor::MoveTo(0, 0),
            cursor::Hide
        )?;

        // Draw initial box at the top
        self.draw_voice_box()?;
        Ok(())
    }

    pub fn update_streaming(&mut self, text: &str, confidence: Option<f32>, voice_active: bool) -> io::Result<()> {
        self.current_transcript = text.to_string();
        self.voice_activity.push(voice_active);
        
        if let Some(conf) = confidence {
            self.confidence_history.push(conf);
        }

        // Keep only last 40 samples for activity bar
        if self.voice_activity.len() > 40 {
            self.voice_activity.remove(0);
        }
        if self.confidence_history.len() > 10 {
            self.confidence_history.remove(0);
        }

        self.draw_voice_box()?;
        Ok(())
    }

    pub fn finalize(&mut self, final_text: &str) -> io::Result<()> {
        self.current_transcript = final_text.to_string();
        self.is_final = true;
        self.draw_voice_box()?;
        
        // Show edit options
        self.show_edit_options()?;
        Ok(())
    }

    fn draw_voice_box(&self) -> io::Result<()> {
        // Move to top and clear display area  
        execute!(io::stdout(), cursor::MoveTo(0, 0))?;

        let elapsed = self.start_time.elapsed().as_secs_f32();
        let avg_confidence = if self.confidence_history.is_empty() {
            0.0
        } else {
            self.confidence_history.iter().sum::<f32>() / self.confidence_history.len() as f32
        };

        // Draw box border (77 characters wide interior)
        println!("┌─────────────────────────────────────────────────────────────────────────────┐");
        
        // Timer and confidence line - account for emoji width
        let status = if self.is_final { "Complete" } else { "Listening" };
        let timer_text = format!("Recording: {:.1}s | Confidence: {:.0}% | Status: {}", 
            elapsed, avg_confidence * 100.0, status);
        // Emoji ⏱️ takes 2 display positions but counts as more chars, so pad to 73
        println!("│ ⏱️  {:<73} │", timer_text);
        
        // Voice activity bar - account for emoji width  
        let mut activity_display = String::new();
        activity_display.push('[');
        for &active in &self.voice_activity {
            if active {
                activity_display.push('█');
            } else {
                activity_display.push('░');
            }
        }
        // Fill remaining space to exactly 40 chars
        for _ in self.voice_activity.len()..40 {
            activity_display.push('░');
        }
        activity_display.push(']');
        // Emoji 🎙️ takes 2 display positions, so pad to 71
        println!("│ 🎙️  {:<71} │", activity_display);
        
        // Transcript line - account for emoji width
        let transcript_display = if self.current_transcript.len() > 65 {
            format!("{}...", &self.current_transcript[..62])
        } else {
            self.current_transcript.clone()
        };
        // Emoji 💬 takes 2 display positions, so pad to 73  
        println!("│ 💬  {:<73} │", transcript_display);
        
        // Empty line
        println!("│{:<77}│", "");
        
        // Options line (only when final) - fix the text and alignment
        if self.is_final {
            let options_text = "Options: [Enter] Submit as-is  [E] Edit  [Ctrl+C] Cancel";
            println!("│ {:<75} │", options_text);
        }
        
        // Bottom border
        println!("└─────────────────────────────────────────────────────────────────────────────┘");
        
        io::stdout().flush()?;
        Ok(())
    }

    fn show_edit_options(&self) -> io::Result<()> {
        execute!(
            io::stdout(),
            cursor::Show,
            cursor::MoveTo(0, 12)
        )?;
        
        print!("\n> ");
        io::stdout().flush()?;
        Ok(())
    }

    pub fn cleanup(&self) -> io::Result<()> {
        execute!(
            io::stdout(),
            cursor::Show,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        Ok(())
    }
}
