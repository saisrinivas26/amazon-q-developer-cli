# 🎤 Amazon Q CLI Voice Implementation - Complete Design Document

## 📋 **Executive Summary**

This document outlines the complete design and implementation of voice-to-text functionality for Amazon Q CLI, enabling users to interact with Amazon Q through natural speech input. The implementation includes two phases: direct AWS Transcribe integration (Phase 1) and hosted service architecture (Phase 2).

## 🎯 **Project Overview**

### **Objective**
Enable Amazon Q CLI users to input prompts using voice commands through a `/voice` slash command, providing a seamless speech-to-text experience with real-time feedback and editable transcription results.

### **Key Features**
- Real-time voice transcription using AWS Transcribe
- Visual voice activity feedback
- Editable transcription results before submission
- Multi-language support
- Cross-platform audio capture
- Professional user interface with live status updates

## 🏗️ **Phase 1: Direct AWS Transcribe Integration**

### **Architecture Overview**

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│   User Voice    │───▶│   Audio Capture  │───▶│   AWS Transcribe    │
│   (Microphone)  │    │   (CPAL Library) │    │   Streaming API     │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
                                │                          │
                                ▼                          ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│   Chat Prompt   │◀───│  User Interface  │◀───│  Real-time Results  │
│   Execution     │    │  (Edit/Submit)   │    │  (Partial/Final)    │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
```

### **Component Design**

#### **1. Audio Capture Module (`audio_capture.rs`)**
```rust
pub struct AudioCapture {
    device: Device,
    config: StreamConfig,
}

impl AudioCapture {
    pub fn new() -> Result<Self>
    pub fn start_capture(&self, audio_sender: mpsc::Sender<Vec<u8>>) -> Result<Stream>
    pub fn check_permissions() -> Result<()>
}
```

**Responsibilities:**
- Cross-platform microphone access using CPAL
- Audio format conversion (any format → 16kHz PCM mono)
- Real-time audio streaming to transcription service
- Device compatibility and error handling

**Key Features:**
- **Adaptive Configuration**: Uses device's native format, converts to AWS requirements
- **Multi-format Support**: Handles F32, I16, U16 sample formats
- **Smart Resampling**: Converts any sample rate to 16kHz for AWS Transcribe
- **Channel Conversion**: Mono conversion from multi-channel input

#### **2. Voice Transcriber Module (`transcriber.rs`)**
```rust
pub struct VoiceTranscriber {
    client: TranscribeClient,
    language_code: LanguageCode,
}

pub struct TranscriptionResult {
    pub audio_sender: mpsc::Sender<AudioEvent>,
    pub transcript_receiver: mpsc::Receiver<TranscriptEvent>,
}
```

**Responsibilities:**
- AWS Transcribe Streaming API integration
- Real-time audio event processing
- Transcript event handling (partial/final results)
- Language code management

**Key Features:**
- **Real-time Streaming**: Direct AWS Transcribe streaming integration
- **Event Processing**: Handles partial and final transcription results
- **Multi-language Support**: Supports all AWS Transcribe language codes
- **Error Handling**: Comprehensive AWS service error management

#### **3. Voice Handler Module (`voice_handler.rs`)**
```rust
pub struct VoiceHandler {
    transcriber: VoiceTranscriber,
    audio_capture: AudioCapture,
}

impl VoiceHandler {
    pub async fn listen_for_speech(&self) -> Result<Option<String>>
    pub async fn check_setup(&self) -> Result<()>
}
```

**Responsibilities:**
- Orchestrates audio capture and transcription
- User interface management and display updates
- Voice activity visualization
- Transcript editing and submission workflow

**Key Features:**
- **Single-line Status Display**: Clean, non-flooding interface
- **Voice Activity Bar**: Real-time visual feedback
- **Editable Transcripts**: Review and edit before submission
- **Silence Detection**: Automatic recording termination

### **User Interface Design**

#### **Recording Interface**
```
🔴 Recording, press ENTER when done...

🗣️  Speak into your microphone now!
📝 Transcription will appear below:

⏱️  8.3s | 🎙️  [████████████░░░░░░░░] | 💬 Your transcribed text appears here
```

#### **Final Review Interface**
```
✅ Transcription complete!
📝 Your transcribed text:
┌─────────────────────────────────────────────────────────────────────────────┐
│ Your complete transcribed text appears here with proper word wrapping and   │
│ formatting within the box boundaries for easy reading and review.           │
└─────────────────────────────────────────────────────────────────────────────┘

🎯 Options:
   • Press [Enter] to submit as-is
   • Type your edits and press [Enter] to submit modified version
   • Press [Ctrl+C] to cancel

✏️  Edit (or press Enter to submit): 
```

### **Technical Implementation Details**

#### **Audio Processing Pipeline**
1. **Device Detection**: Enumerate and select default input device
2. **Format Negotiation**: Use device's native format for compatibility
3. **Real-time Conversion**: Convert to 16kHz PCM mono for AWS
4. **Streaming**: Send audio chunks to AWS Transcribe via WebSocket

#### **Transcription Processing**
1. **Stream Initialization**: Create AWS Transcribe streaming session
2. **Event Handling**: Process partial and final transcript events
3. **Text Aggregation**: Build complete transcript from partial results
4. **Activity Detection**: Monitor voice activity for UI feedback

#### **Display Management**
1. **Single-line Updates**: Use carriage return for in-place updates
2. **Voice Activity Visualization**: Dynamic bar based on speech detection
3. **Text Truncation**: Smart truncation for single-line display
4. **Clean Termination**: Proper cursor positioning after recording

### **Error Handling Strategy**

#### **Audio Errors**
- **Device Unavailable**: Graceful fallback with diagnostics
- **Permission Denied**: Clear instructions for microphone access
- **Format Unsupported**: Automatic format conversion
- **Stream Interruption**: Reconnection and recovery

#### **AWS Service Errors**
- **Credentials Invalid**: Clear authentication guidance
- **Service Unavailable**: Retry logic with exponential backoff
- **Network Issues**: Timeout handling and user feedback
- **Quota Exceeded**: Informative error messages

#### **User Experience Errors**
- **No Speech Detected**: Timeout with retry option
- **Transcription Empty**: Fallback to text input mode
- **Edit Conflicts**: Input validation and sanitization

## 🚀 **Phase 2: Hosted Service Architecture**

### **Architecture Overview**

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│   User Voice    │───▶│   Audio Capture  │───▶│  CodeWhisperer      │
│   (Microphone)  │    │   (CPAL Library) │    │  Hosted Service     │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
                                │                          │
                                │                          ▼
                                │               ┌─────────────────────┐
                                │               │  Speech-to-Text     │
                                │               │  Model Selection    │
                                │               │  (Transcribe/Other) │
                                │               └─────────────────────┘
                                │                          │
                                ▼                          ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│   Chat Prompt   │◀───│  User Interface  │◀───│  Transcription      │
│   Execution     │    │  (Edit/Submit)   │    │  Results            │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
```

### **Hosted Service Design**

#### **Service Architecture**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CodeWhisperer Hosted Service                         │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐  │
│  │   API Gateway   │  │  Load Balancer  │  │     Authentication          │  │
│  │   (WebSocket)   │  │   (Regional)    │  │     (IAM/Cognito)          │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘  │
│                                │                                            │
│  ┌─────────────────────────────▼─────────────────────────────────────────┐  │
│  │                    Audio Processing Service                           │  │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │  │
│  │  │  Audio Buffer   │  │  Format Convert │  │   Quality Check     │  │  │
│  │  │  (Streaming)    │  │  (Multi-format) │  │   (Noise Filter)    │  │  │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                │                                            │
│  ┌─────────────────────────────▼─────────────────────────────────────────┐  │
│  │                 Speech-to-Text Model Router                           │  │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │  │
│  │  │ AWS Transcribe  │  │   Whisper API   │  │   Custom Models     │  │  │
│  │  │   (Primary)     │  │  (Fallback)     │  │   (Specialized)     │  │  │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                │                                            │
│  ┌─────────────────────────────▼─────────────────────────────────────────┐  │
│  │                    Response Processing                                │  │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │  │
│  │  │ Text Processing │  │  Confidence     │  │   Result Caching    │  │  │
│  │  │ (Formatting)    │  │  Scoring        │  │   (Performance)     │  │  │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### **Client-Side Changes for Phase 2**

#### **Modified Transcriber Module**
```rust
pub struct HostedVoiceTranscriber {
    client: reqwest::Client,
    websocket_url: String,
    auth_token: String,
    language_preference: String,
}

impl HostedVoiceTranscriber {
    pub async fn new(config: &HostedServiceConfig) -> Result<Self>
    pub async fn start_transcription(&self) -> Result<HostedTranscriptionResult>
    pub async fn send_audio_chunk(&self, audio_data: Vec<u8>) -> Result<()>
    pub async fn end_transcription(&self) -> Result<String>
}
```

#### **WebSocket Communication Protocol**
```json
// Client -> Service (Audio Chunk)
{
    "type": "audio_chunk",
    "session_id": "uuid-session-id",
    "sequence": 123,
    "audio_data": "base64-encoded-audio",
    "timestamp": "2024-01-15T10:30:00Z"
}

// Service -> Client (Partial Result)
{
    "type": "partial_transcript",
    "session_id": "uuid-session-id",
    "sequence": 123,
    "text": "Hello this is a partial",
    "confidence": 0.85,
    "timestamp": "2024-01-15T10:30:01Z"
}

// Service -> Client (Final Result)
{
    "type": "final_transcript",
    "session_id": "uuid-session-id",
    "text": "Hello this is a complete transcription",
    "confidence": 0.92,
    "model_used": "aws-transcribe",
    "processing_time_ms": 150
}
```

### **Hosted Service Implementation**

#### **API Endpoints**
```
POST /v1/transcription/session
  - Create new transcription session
  - Returns: session_id, websocket_url

WebSocket /v1/transcription/stream/{session_id}
  - Real-time audio streaming
  - Bidirectional communication

GET /v1/transcription/models
  - List available speech-to-text models
  - Returns: model capabilities and languages

POST /v1/transcription/session/{session_id}/end
  - Gracefully end transcription session
  - Returns: final transcript and metadata
```

#### **Model Selection Logic**
```rust
pub enum SpeechModel {
    AwsTranscribe {
        region: String,
        language_code: String,
    },
    WhisperApi {
        model_size: WhisperModelSize,
        language: Option<String>,
    },
    CustomModel {
        model_id: String,
        endpoint: String,
    },
}

pub struct ModelRouter {
    primary_model: SpeechModel,
    fallback_models: Vec<SpeechModel>,
    selection_criteria: SelectionCriteria,
}

impl ModelRouter {
    pub async fn select_model(&self, audio_metadata: AudioMetadata) -> SpeechModel
    pub async fn process_audio(&self, model: SpeechModel, audio: AudioChunk) -> TranscriptResult
    pub async fn handle_fallback(&self, error: ModelError) -> Option<SpeechModel>
}
```

#### **Quality and Performance Features**
```rust
pub struct AudioProcessor {
    noise_reduction: NoiseFilter,
    quality_enhancer: AudioEnhancer,
    format_converter: FormatConverter,
}

pub struct TranscriptProcessor {
    confidence_scorer: ConfidenceAnalyzer,
    text_formatter: TextFormatter,
    language_detector: LanguageDetector,
}

pub struct CacheManager {
    result_cache: LruCache<AudioHash, TranscriptResult>,
    model_cache: HashMap<ModelId, LoadedModel>,
    session_store: SessionStore,
}
```

### **Configuration Management**

#### **Phase 1 Configuration**
```rust
pub struct VoiceConfig {
    pub aws_region: String,
    pub language_code: String,
    pub silence_timeout_seconds: u64,
    pub audio_sample_rate: u32,
    pub voice_activity_threshold: f32,
}
```

#### **Phase 2 Configuration**
```rust
pub struct HostedVoiceConfig {
    pub service_endpoint: String,
    pub auth_method: AuthMethod,
    pub preferred_models: Vec<String>,
    pub fallback_enabled: bool,
    pub quality_settings: QualitySettings,
    pub caching_enabled: bool,
}

pub enum AuthMethod {
    IamRole(String),
    ApiKey(String),
    OAuth2(OAuth2Config),
}
```

### **Migration Strategy (Phase 1 → Phase 2)**

#### **Backward Compatibility**
```rust
pub enum VoiceProvider {
    DirectAws(VoiceTranscriber),
    HostedService(HostedVoiceTranscriber),
}

impl VoiceProvider {
    pub async fn from_config(config: &VoiceConfiguration) -> Result<Self> {
        match config.provider_type {
            ProviderType::DirectAws => {
                Ok(VoiceProvider::DirectAws(VoiceTranscriber::new(config).await?))
            }
            ProviderType::HostedService => {
                Ok(VoiceProvider::HostedService(HostedVoiceTranscriber::new(config).await?))
            }
        }
    }
}
```

#### **Feature Parity Matrix**
| Feature | Phase 1 (Direct AWS) | Phase 2 (Hosted) | Notes |
|---------|---------------------|-------------------|-------|
| Real-time Transcription | ✅ | ✅ | Same UX |
| Voice Activity Bar | ✅ | ✅ | Enhanced with confidence |
| Multi-language Support | ✅ | ✅ | Extended language support |
| Editable Transcripts | ✅ | ✅ | Same workflow |
| Offline Capability | ❌ | ❌ | Future consideration |
| Custom Models | ❌ | ✅ | Hosted service advantage |
| Model Fallback | ❌ | ✅ | Improved reliability |
| Advanced Audio Processing | ❌ | ✅ | Noise reduction, enhancement |

## 📊 **Performance Specifications**

### **Latency Requirements**
- **Audio Capture**: < 50ms buffer latency
- **Network Transmission**: < 100ms to service
- **Transcription Processing**: < 200ms for partial results
- **UI Updates**: < 16ms for smooth animation (60fps)
- **End-to-end Latency**: < 500ms for real-time experience

### **Throughput Specifications**
- **Audio Streaming**: 16kHz PCM mono (32 KB/s)
- **Concurrent Sessions**: 1000+ per service instance
- **Model Processing**: 10x real-time for batch processing
- **WebSocket Connections**: 5000+ concurrent connections

### **Quality Metrics**
- **Transcription Accuracy**: > 95% for clear speech
- **Language Detection**: > 98% accuracy
- **Confidence Scoring**: Calibrated confidence intervals
- **Noise Handling**: Effective up to 20dB SNR

## 🔒 **Security Considerations**

### **Data Protection**
- **Audio Encryption**: TLS 1.3 for all transmissions
- **At-rest Encryption**: AES-256 for temporary storage
- **Data Retention**: Configurable retention policies
- **PII Handling**: Automatic PII detection and redaction

### **Authentication & Authorization**
- **Client Authentication**: IAM roles or API keys
- **Session Management**: Secure session tokens
- **Rate Limiting**: Per-user and per-service limits
- **Audit Logging**: Comprehensive access logging

### **Privacy Compliance**
- **Data Minimization**: Process only necessary audio data
- **User Consent**: Clear consent for audio processing
- **Data Locality**: Regional data processing requirements
- **Right to Deletion**: User data deletion capabilities

## 🧪 **Testing Strategy**

### **Unit Testing**
- **Audio Processing**: Mock audio streams and format conversion
- **Transcription Logic**: Mock AWS responses and error conditions
- **UI Components**: Display update logic and user interactions
- **Error Handling**: Comprehensive error scenario coverage

### **Integration Testing**
- **End-to-end Workflows**: Complete voice-to-prompt flows
- **AWS Service Integration**: Real AWS Transcribe testing
- **Cross-platform Compatibility**: Windows, macOS, Linux testing
- **Network Conditions**: Various network scenarios

### **Performance Testing**
- **Latency Measurement**: Real-time performance metrics
- **Memory Usage**: Audio buffer and processing memory
- **CPU Utilization**: Transcription processing overhead
- **Concurrent Users**: Multi-user load testing

### **User Acceptance Testing**
- **Voice Quality**: Various microphone and environment tests
- **Language Support**: Multi-language transcription accuracy
- **User Experience**: Usability and accessibility testing
- **Error Recovery**: Graceful error handling validation

## 📈 **Monitoring & Observability**

### **Metrics Collection**
```rust
pub struct VoiceMetrics {
    pub transcription_latency: Histogram,
    pub audio_quality_score: Gauge,
    pub error_rate: Counter,
    pub session_duration: Histogram,
    pub model_accuracy: Gauge,
}
```

### **Logging Strategy**
- **Structured Logging**: JSON format with correlation IDs
- **Performance Logs**: Latency and throughput metrics
- **Error Logs**: Detailed error context and stack traces
- **Audit Logs**: User actions and system events

### **Alerting Rules**
- **High Error Rate**: > 5% transcription failures
- **High Latency**: > 1s end-to-end latency
- **Service Unavailable**: AWS Transcribe service issues
- **Resource Exhaustion**: Memory or CPU threshold alerts

## 🚀 **Deployment Strategy**

### **Phase 1 Deployment**
1. **Feature Flag**: Gradual rollout with feature toggles
2. **A/B Testing**: Compare voice vs text input efficiency
3. **Regional Rollout**: Start with primary AWS regions
4. **User Feedback**: Collect and iterate based on usage

### **Phase 2 Deployment**
1. **Service Infrastructure**: Deploy hosted service components
2. **Migration Tools**: Seamless transition from Phase 1
3. **Load Testing**: Validate service capacity and performance
4. **Monitoring Setup**: Comprehensive observability stack

### **Rollback Strategy**
- **Feature Toggles**: Instant disable capability
- **Graceful Degradation**: Fallback to text input mode
- **Data Preservation**: Maintain user preferences and settings
- **Communication Plan**: User notification and support

## 📚 **Documentation Plan**

### **User Documentation**
- **Setup Guide**: Installation and configuration instructions
- **Usage Tutorial**: Step-by-step voice command usage
- **Troubleshooting**: Common issues and solutions
- **Language Support**: Supported languages and configuration

### **Developer Documentation**
- **API Reference**: Complete API documentation
- **Architecture Guide**: System design and component interaction
- **Extension Guide**: Adding new speech models or languages
- **Contributing Guide**: Development setup and contribution process

### **Operations Documentation**
- **Deployment Guide**: Service deployment and configuration
- **Monitoring Runbook**: Operational procedures and troubleshooting
- **Security Guide**: Security best practices and compliance
- **Disaster Recovery**: Backup and recovery procedures

## 🎯 **Success Metrics**

### **User Adoption**
- **Feature Usage**: % of users trying voice input
- **Retention Rate**: Users continuing to use voice features
- **Session Length**: Average voice session duration
- **User Satisfaction**: NPS scores for voice experience

### **Technical Performance**
- **Transcription Accuracy**: Word Error Rate (WER) < 5%
- **System Reliability**: 99.9% uptime for voice services
- **Response Time**: < 500ms end-to-end latency
- **Error Rate**: < 1% transcription failures

### **Business Impact**
- **Productivity Gain**: Faster prompt input vs typing
- **Accessibility**: Improved access for users with disabilities
- **User Engagement**: Increased Q CLI usage and interaction
- **Cost Efficiency**: Optimized transcription service costs

## 🔮 **Future Enhancements**

### **Advanced Features**
- **Voice Commands**: Direct action commands ("Create a file", "Run tests")
- **Context Awareness**: Understanding previous conversation context
- **Emotion Detection**: Sentiment analysis from voice tone
- **Multi-speaker Support**: Handling multiple speakers in conversation

### **Platform Extensions**
- **Mobile Support**: Voice input for mobile Q applications
- **Web Integration**: Browser-based voice input capabilities
- **IDE Plugins**: Voice coding assistance in development environments
- **Smart Devices**: Integration with Alexa and other voice assistants

### **AI/ML Improvements**
- **Custom Wake Words**: Personalized activation phrases
- **Adaptive Learning**: User-specific speech pattern learning
- **Real-time Translation**: Multi-language conversation support
- **Code Understanding**: Voice-to-code generation capabilities

---

## 📝 **Conclusion**

This design document outlines a comprehensive approach to implementing voice functionality in Amazon Q CLI, progressing from direct AWS integration to a sophisticated hosted service architecture. The implementation prioritizes user experience, reliability, and scalability while maintaining security and performance standards.

**Phase 1** provides immediate value with direct AWS Transcribe integration, establishing the foundation for voice interaction. **Phase 2** extends capabilities through a hosted service architecture, enabling advanced features like model selection, improved quality, and enhanced reliability.

The modular design ensures smooth migration between phases while maintaining backward compatibility and user experience consistency. Comprehensive testing, monitoring, and documentation strategies support successful deployment and long-term maintenance.

This implementation positions Amazon Q CLI as a leader in voice-enabled developer tools, providing natural and efficient interaction methods that enhance productivity and accessibility for all users.

---

**Document Version**: 1.0  
**Last Updated**: January 2024  
**Next Review**: March 2024
