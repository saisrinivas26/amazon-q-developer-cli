use std::io::{self, Write};
use std::time::Instant;
use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType},
};

pub struct VoiceDisplay {
    start_time: Instant,
    voice_activity: Vec<bool>,
    current_transcript: String,
    is_final: bool,
}

impl VoiceDisplay {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            voice_activity: Vec::new(),
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

    pub fn update_streaming(&mut self, text: &str, _confidence: Option<f32>, voice_active: bool) -> io::Result<()> {
        self.current_transcript = text.to_string();

        // Only append activity when voice is detected (prevents wiping the bar with idle falses)
        if voice_active {
            self.voice_activity.push(true);
        }

        // Keep only last 40 activity samples
        if self.voice_activity.len() > 40 {
            self.voice_activity.remove(0);
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

        // Timer and status line (no confidence display)
        let status = if self.is_final { "Complete" } else { "Listening" };
        let timer_text = format!("⏱️  Recording: {:.1}s | Status: {}", elapsed, status);
        println!("{}", timer_text);
        
        // Voice activity bar
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
        println!("{}", activity_display);
        
        // Transcript line
        let transcript_display = if self.current_transcript.is_empty() {
            "💬 ".to_string()
        } else {
            format!("💬 {}", self.current_transcript)
        };
        println!("{}", transcript_display);
        
        // Empty line for spacing
        println!();
        
        // Options line (only when final)
        if self.is_final {
            println!("Options: [Enter] Submit as-is  [E] Edit  [Ctrl+C] Cancel");
        }
        
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
