//! AWS Transcribe streaming provider using pure streaming approach

use async_trait::async_trait;
use tokio::sync::mpsc;
use aws_config::SdkConfig;
use aws_sdk_transcribestreaming::{
    Client as TranscribeClient,
    types::{AudioEvent, LanguageCode, MediaEncoding, AudioStream as AwsAudioStream},
};
use aws_smithy_types::Blob;
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use crate::cli::chat::voice::{
    error::{VoiceResult, VoiceError},
    provider::{TranscriptionProvider, TranscriptionOptions},
    streaming::{AudioStream, StreamingTranscription, TranscriptionStream},
};

pub struct AwsTranscribeProvider {
    client: TranscribeClient,
    language_code: LanguageCode,
}

impl AwsTranscribeProvider {
    pub async fn new_with_aws_config(aws_config: &SdkConfig, language: &str) -> VoiceResult<Self> {
        let client = TranscribeClient::new(aws_config);
        
        let language_code = match language.to_lowercase().as_str() {
            "en-us" | "en" => LanguageCode::EnUs,
            "es-us" | "es" => LanguageCode::EsUs,
            "fr-fr" | "fr" => LanguageCode::FrFr,
            "de-de" | "de" => LanguageCode::DeDe,
            "it-it" | "it" => LanguageCode::ItIt,
            "pt-br" | "pt" => LanguageCode::PtBr,
            "ja-jp" | "ja" => LanguageCode::JaJp,
            "ko-kr" | "ko" => LanguageCode::KoKr,
            "zh-cn" | "zh" => LanguageCode::ZhCn,
            _ => {
                info!("Unsupported language '{}', defaulting to en-US", language);
                LanguageCode::EnUs
            }
        };
        
        debug!("Initialized AWS Transcribe with language: {:?}", language_code);
        
        Ok(Self {
            client,
            language_code,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for AwsTranscribeProvider {
    async fn stream_transcribe(&self, mut audio_stream: AudioStream, _options: &TranscriptionOptions) -> VoiceResult<TranscriptionStream> {
        debug!("Starting pure AWS Transcribe streaming transcription");
        
        // Create result channel  
        let (result_tx, result_rx) = mpsc::channel::<StreamingTranscription>(100);
        
        // Create audio event channel for AWS
        let (audio_tx, audio_rx) = mpsc::channel::<AudioEvent>(1000);
        
        // Start audio forwarding task FIRST
        tokio::spawn(async move {
            debug!("Starting audio forwarding to AWS Transcribe");
            
            while let Some(audio_blob) = audio_stream.recv().await {
                debug!("Forwarding audio chunk of {} bytes", audio_blob.data.len());
                let audio_event = AudioEvent::builder()
                    .audio_chunk(Blob::new(audio_blob.data))
                    .build();
                
                if audio_tx.send(audio_event).await.is_err() {
                    debug!("Audio sender channel closed");
                    break;
                }
            }
            
            debug!("Audio forwarding to AWS Transcribe ended");
        });
        
        // Convert audio events to AWS AudioStream format 
        let aws_audio_stream = ReceiverStream::new(audio_rx).map(|audio_event| {
            Ok(AwsAudioStream::AudioEvent(audio_event))
        });
        
        // Start AWS Transcribe streaming session
        debug!("Connecting to AWS Transcribe...");
        let response = self.client
            .start_stream_transcription()
            .language_code(self.language_code.clone())
            .media_sample_rate_hertz(16000)
            .media_encoding(MediaEncoding::Pcm)
            .set_audio_stream(Some(aws_audio_stream.into()))
            .send()
            .await
            .map_err(|e| {
                error!("Failed to connect to AWS Transcribe: {}", e);
                VoiceError::TranscribeUnavailable(e.to_string())
            })?;

        info!("✅ Connected to Amazon Transcribe streaming service");
        
        // Process transcript results
        let mut transcript_stream = response.transcript_result_stream;
        
        tokio::spawn(async move {
            debug!("Starting transcript stream processing");
            
            loop {
                match transcript_stream.recv().await {
                    Ok(Some(transcript_stream_item)) => {
                        debug!("Received transcript stream item");
                        match transcript_stream_item {
                            aws_sdk_transcribestreaming::types::TranscriptResultStream::TranscriptEvent(transcript_event) => {
                                if let Some(transcript) = transcript_event.transcript {
                                    if let Some(results) = transcript.results {
                                        for result in results {
                                            if let Some(alternatives) = result.alternatives {
                                                for alternative in alternatives {
                                                    if let Some(transcript_text) = alternative.transcript {
                                                        let is_partial = result.is_partial;
                                                        
                                                        debug!("AWS Transcribe result: '{}' (partial: {})", 
                                                               transcript_text, is_partial);
                                                        
                                                        let streaming_result = StreamingTranscription {
                                                            partial_text: transcript_text.clone(),
                                                            confidence: 0.95,
                                                            is_final: !is_partial,
                                                            timestamp: std::time::Duration::from_secs(0),
                                                            word_count: transcript_text.split_whitespace().count(),
                                                        };
                                                        
                                                        if let Err(_) = result_tx.send(streaming_result).await {
                                                            debug!("Transcript receiver closed");
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {
                                debug!("Received other transcript stream item type");
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("Transcript stream ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Transcript stream error: {:?}", e);
                        break;
                    }
                }
            }
            
            debug!("Transcript stream processing completed");
        });
        
        Ok(result_rx)
    }

    fn supports_streaming(&self) -> bool {
        true // AWS Transcribe always uses streaming
    }
}
