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

        // Only append activity when voice is detected (prevents wiping the bar with idle falses)
        if voice_active {
            self.voice_activity.push(true);
        }

        if let Some(conf) = confidence {
            // keep values in [0.0, 1.0]
            self.confidence_history.push(conf.clamp(0.0, 1.0));
        }

        // Keep only last 40 activity samples and last 10 confidence samples
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
        let avg_confidence_opt = if self.confidence_history.is_empty() {
            None
        } else {
            Some(self.confidence_history.iter().sum::<f32>() / self.confidence_history.len() as f32)
        };

        // Draw box border (77 characters wide interior)
        println!("┌─────────────────────────────────────────────────────────────────────────────┐");
        
        // Timer and confidence line - properly pad to 77 chars
        let status = if self.is_final { "Complete" } else { "Listening" };
        let timer_text = match avg_confidence_opt {
            Some(avg) => format!("⏱️  Recording: {:.1}s | Confidence: {:.0}% | Status: {}", 
                elapsed, avg * 100.0, status),
            None => format!("⏱️  Recording: {:.1}s | Confidence: — | Status: {}", 
                elapsed, status),
        };
        let padding = 77_usize.saturating_sub(timer_text.chars().count());
        println!("│ {}{} │", timer_text, " ".repeat(padding));
        
        // Voice activity bar - properly pad to 77 chars
        let mut activity_display = String::from("🎙️  [");
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
        let padding = 77_usize.saturating_sub(activity_display.chars().count());
        println!("│ {}{} │", activity_display, " ".repeat(padding));
        
        // Transcript line - properly pad to 77 chars
        let transcript_prefix = "💬  ";
        let available_width = 77_usize.saturating_sub(transcript_prefix.chars().count());
        let transcript_display = if self.current_transcript.chars().count() > available_width {
            let truncated: String = self.current_transcript.chars().take(available_width.saturating_sub(3)).collect();
            format!("{}...", truncated)
        } else {
            self.current_transcript.clone()
        };
        let full_line = format!("{}{}", transcript_prefix, transcript_display);
        let padding = 77_usize.saturating_sub(full_line.chars().count());
        println!("│ {}{} │", full_line, " ".repeat(padding));
        
        // Empty line
        println!("│{:<77}│", "");
        
        // Options line (only when final) - properly pad to 77 chars
        if self.is_final {
            let options_text = "Options: [Enter] Submit as-is  [E] Edit  [Ctrl+C] Cancel";
            let padding = 77_usize.saturating_sub(options_text.chars().count());
            println!("│ {}{} │", options_text, " ".repeat(padding));
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
