use std::io::{self, Write};
use std::time::Instant;
use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType},
};

pub struct VoiceDisplay {
    start_time: Instant,
    voice_activity: Vec<u8>,
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

    pub fn update_streaming(&mut self, text: &str, _confidence: Option<f32>, voice_activity_level: u8) -> io::Result<()> {
        self.current_transcript = text.to_string();

        // Store the activity level directly (0-8 scale) for proportional bars
        self.voice_activity.push(voice_activity_level);

        // Keep only last 40 activity samples for sliding window effect
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
        
        // Voice activity bar with proportional levels (like Whisper)
        let mut activity_display = String::from("🎙️  [");
        for &level in &self.voice_activity {
            // Convert 0-8 activity level to bar character
            // 0 = silent, 1-2 = quiet, 3-4 = normal, 5-6 = loud, 7-8 = very loud
            let bar_char = match level {
                0 => '░',           // Silent
                1..=2 => '▒',       // Quiet
                3..=4 => '▓',       // Normal speech  
                5..=6 => '█',       // Loud
                7..=8 => '█',       // Very loud
                _ => '░',           // Fallback
            };
            activity_display.push(bar_char);
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
