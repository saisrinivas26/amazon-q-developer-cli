# Voice Mode Testing Instructions

## Prerequisites
1. **AWS Credentials**: Ensure you have AWS credentials configured
   ```bash
   aws configure list
   # or check if you have credentials in ~/.aws/credentials
   ```

2. **Microphone Permissions**: Make sure your terminal has microphone access permissions

## Test Steps

### Step 1: Basic Voice Mode Test
```bash
cd /Users/somarsai/Desktop/Desktop/qcli/amazon-q-for-command-line
./target/debug/chat_cli chat --voice
```

**Expected behavior:**
- Should show voice setup help
- Should attempt to initialize microphone
- Should show "🎤 Voice mode activated. Speak now..."
- Should wait for voice input for ~3 seconds
- Should return a test message: "Hello, this is a test voice input"

### Step 2: Test Different Languages
```bash
./target/debug/chat_cli chat --voice --voice-language es-US
./target/debug/chat_cli chat --voice --voice-language fr-FR
```

### Step 3: Test Voice with Actual Chat
```bash
./target/debug/chat_cli chat --voice "What is the weather like?"
```

### Step 4: Test Error Handling
```bash
# Test without AWS credentials (should show fallback message)
AWS_PROFILE=nonexistent ./target/debug/chat_cli chat --voice
```

## Expected Output Examples

### Successful Voice Activation:
```
🎤 Voice Mode Setup
==================

Requirements:
• Microphone access permission
• AWS credentials with Transcribe permissions
• Stable internet connection

Usage:
• Speak clearly into your microphone
• Pause briefly when finished speaking
• Press Ctrl+C to exit voice mode

🎤 Voice mode activated. Speak now...
   (Press Ctrl+C to stop)

✓ Transcription complete: "Hello, this is a test voice input"
```

### Error Cases:
```
❌ Voice setup failed: Microphone not available or permission denied
💡 Falling back to text input mode
```

## Troubleshooting

1. **Microphone Permission Denied**:
   - Go to System Preferences > Security & Privacy > Privacy > Microphone
   - Add your terminal application

2. **AWS Credentials Issues**:
   - Run `aws configure` to set up credentials
   - Or set environment variables: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`

3. **Build Issues**:
   ```bash
   cargo clean
   cargo build --bin chat_cli
   ```

## Current Implementation Status

✅ **Implemented:**
- Voice CLI arguments (`--voice`, `--voice-language`)
- AWS Transcribe client integration
- Audio capture setup (CPAL)
- Error handling and fallback
- Voice setup help

🚧 **Simplified for Demo:**
- Currently returns a test message instead of real transcription
- AWS Transcribe streaming implementation is simplified
- Real-time audio processing is basic

🔄 **Next Steps for Production:**
- Implement full AWS Transcribe streaming API
- Add real-time transcript processing
- Improve audio quality and noise handling
- Add voice activity detection
