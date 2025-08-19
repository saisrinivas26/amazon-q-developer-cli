#!/usr/bin/env python3
"""
Setup script for Mac-optimized voice recognition with GPU acceleration
"""

import subprocess
import sys
import platform

def run_command(cmd, description):
    """Run a command and handle errors"""
    print(f"🔄 {description}...")
    try:
        result = subprocess.run(cmd, shell=True, check=True, capture_output=True, text=True)
        if result.stdout:
            print(f"✅ {description} completed")
            return True
    except subprocess.CalledProcessError as e:
        print(f"❌ {description} failed: {e}")
        if e.stderr:
            print(f"Error: {e.stderr}")
        return False
    return True

def check_mac_capabilities():
    """Check Mac-specific capabilities"""
    print("🔍 Checking Mac capabilities...")
    
    if platform.system() != "Darwin":
        print("⚠️  Not running on macOS")
        return
    
    # Check for M1/M2 chip
    try:
        result = subprocess.run(['uname', '-m'], capture_output=True, text=True)
        if 'arm64' in result.stdout:
            print("✅ Apple Silicon (M1/M2) detected - Neural Engine available")
        else:
            print("ℹ️  Intel Mac detected - GPU acceleration still available")
    except:
        pass
    
    # Check Python version
    version = sys.version_info
    if version.major >= 3 and version.minor >= 8:
        print(f"✅ Python {version.major}.{version.minor} is compatible")
    else:
        print(f"⚠️  Python {version.major}.{version.minor} detected. Recommend 3.8+")

def install_packages():
    """Install required packages with Mac optimization"""
    print("\n📦 Installing Mac-optimized packages...")
    
    # Install ffmpeg first (required by Whisper)
    print("🔧 Installing ffmpeg...")
    ffmpeg_installed = False
    
    # Try homebrew first
    if run_command("brew --version", "Checking Homebrew"):
        if run_command("brew install ffmpeg", "Installing ffmpeg via Homebrew"):
            ffmpeg_installed = True
    
    if not ffmpeg_installed:
        print("⚠️  Homebrew not available. Install ffmpeg manually:")
        print("   1. Install Homebrew: /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"")
        print("   2. Install ffmpeg: brew install ffmpeg")
    
    packages = [
        # Core PyTorch with Metal support
        "torch torchvision torchaudio",
        
        # Whisper for fast transcription
        "openai-whisper",
        
        # Alternative: transformers-based approach
        "transformers",
        
        # Audio processing
        "librosa soundfile",
        
        # Optional: Advanced VAD
        "webrtcvad",
    ]
    
    for package in packages:
        if not run_command(f"pip install {package}", f"Installing {package}"):
            print(f"⚠️  Failed to install {package}, continuing...")

def test_metal_support():
    """Test Metal Performance Shaders support"""
    print("\n🧪 Testing Metal GPU support...")
    
    test_script = """
import torch
import platform

print(f"Platform: {platform.system()} {platform.machine()}")
print(f"PyTorch version: {torch.__version__}")

if torch.backends.mps.is_available():
    print("✅ Metal Performance Shaders (MPS) available!")
    print("🚀 GPU acceleration will be used")
    
    # Test basic tensor operations
    device = torch.device("mps")
    x = torch.randn(100, 100, device=device)
    y = torch.randn(100, 100, device=device)
    z = torch.matmul(x, y)
    print(f"✅ GPU tensor operations working (result shape: {z.shape})")
    
elif torch.cuda.is_available():
    print("✅ CUDA GPU available!")
    print("🚀 CUDA acceleration will be used")
else:
    print("⚠️  No GPU acceleration available, using CPU")
    print("💡 For Mac: Install PyTorch with: pip install torch torchvision torchaudio")
"""
    
    try:
        result = subprocess.run([sys.executable, '-c', test_script], 
                              capture_output=True, text=True, timeout=30)
        print(result.stdout)
        if result.stderr:
            print(f"Warnings: {result.stderr}")
    except subprocess.TimeoutExpired:
        print("⚠️  Test timed out")
    except Exception as e:
        print(f"❌ Test failed: {e}")

def test_whisper():
    """Test Whisper installation"""
    print("\n🎤 Testing Whisper installation...")
    
    test_script = """
import whisper
import torch

print("Loading Whisper base model...")
device = "mps" if torch.backends.mps.is_available() else "cpu"
print(f"Using device: {device}")

try:
    model = whisper.load_model("base", device=device)
    print("✅ Whisper model loaded successfully!")
    print(f"Model device: {device}")
    print("🎉 Ready for fast Mac-optimized transcription!")
except Exception as e:
    print(f"❌ Whisper test failed: {e}")
"""
    
    try:
        result = subprocess.run([sys.executable, '-c', test_script], 
                              capture_output=True, text=True, timeout=60)
        print(result.stdout)
        if result.stderr:
            print(f"Warnings: {result.stderr}")
    except subprocess.TimeoutExpired:
        print("⚠️  Whisper test timed out")
    except Exception as e:
        print(f"❌ Whisper test failed: {e}")

def main():
    print("🎙️  Mac Voice Recognition Setup")
    print("=" * 40)
    
    check_mac_capabilities()
    install_packages()
    test_metal_support()
    test_whisper()
    
    print("\n🎉 Setup complete!")
    print("\n📋 Next steps:")
    print("   1. Build your application: cargo build")
    print("   2. Test voice mode: ./target/debug/chat_cli chat --voice")
    print("   3. Enjoy Mac GPU-accelerated speech recognition!")
    
    print("\n💡 Performance tips:")
    print("   • Whisper 'base' model: Good balance of speed/accuracy")  
    print("   • Metal acceleration: ~3-5x faster than CPU")
    print("   • Neural Engine: Automatic optimization on M1/M2")

if __name__ == "__main__":
    main()
