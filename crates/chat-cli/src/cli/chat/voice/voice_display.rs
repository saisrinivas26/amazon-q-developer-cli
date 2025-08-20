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

        // Draw initial box
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
        execute!(io::stdout(), cursor::MoveTo(0, 5))?;

        let elapsed = self.start_time.elapsed().as_secs_f32();
        let avg_confidence = if self.confidence_history.is_empty() {
            0.0
        } else {
            self.confidence_history.iter().sum::<f32>() / self.confidence_history.len() as f32
        };

        // Draw box border
        println!("┌─────────────────────────────────────────────────────────────────────────────┐");
        
        // Timer and confidence line
        println!("│ ⏱️  Recording: {:.1}s | Confidence: {:.0}% | Status: {}           │", 
            elapsed, 
            avg_confidence * 100.0,
            if self.is_final { "Complete" } else { "Listening" }
        );
        
        // Voice activity bar
        print!("│ 🎙️  [");
        for &active in &self.voice_activity {
            if active {
                print!("█");
            } else {
                print!("░");
            }
        }
        // Fill remaining space
        for _ in self.voice_activity.len()..40 {
            print!("░");
        }
        println!("]                                    │");
        
        // Transcript line with proper padding
        let transcript_display = if self.current_transcript.len() > 70 {
            format!("{}...", &self.current_transcript[..67])
        } else {
            self.current_transcript.clone()
        };
        
        println!("│ 💬  {:<70} │", transcript_display);
        
        if self.is_final {
            println!("│                                                                             │");
            println!("│ Options: [Enter] Submit as-is  [E] Edit  [R] Re-record  [C] Cancel        │");
        }
        
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
        
        println!("\n🎯 Options:");
        println!("   • Press [Enter] to submit as-is");
        println!("   • Press [e] + [Enter] to edit");
        println!("   • Press [r] + [Enter] to re-record");
        println!("   • Press [Ctrl+C] to cancel");
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
