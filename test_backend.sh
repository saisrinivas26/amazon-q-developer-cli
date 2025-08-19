#!/bin/bash

echo "Testing voice backends..."
echo

echo "1. Testing help to see all available backends:"
./target/debug/chat_cli chat /voice --help
echo

echo "2. Testing local-whisper backend (should work now):"
echo "/voice --backend local-whisper" | timeout 5s ./target/debug/chat_cli chat || echo "Command completed or timed out"
echo

echo "3. Testing local-parakeet backend:"
echo "/voice --backend local-parakeet" | timeout 5s ./target/debug/chat_cli chat || echo "Command completed or timed out"
echo

echo "Backend testing complete!"
