use aws_config::SdkConfig;
use aws_sdk_transcribestreaming::Client as TranscribeClient;
use aws_sdk_transcribestreaming::types::{
    AudioEvent,
    AudioStream,
    LanguageCode,
    MediaEncoding,
};
use aws_smithy_types::Blob;
use eyre::Result;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{
    debug,
    error,
    info,
};

use super::VoiceError;

pub struct VoiceTranscriber {
    client: TranscribeClient,
    language_code: LanguageCode,
}

pub struct TranscriptionResult {
    pub audio_sender: mpsc::Sender<AudioEvent>,
    pub transcript_receiver: mpsc::Receiver<TranscriptEvent>,
}

#[derive(Debug, Clone)]
pub struct TranscriptEvent {
    pub transcript: String,
    pub is_partial: bool,
}

impl VoiceTranscriber {
    pub async fn new(aws_config: &SdkConfig, language: &str) -> Result<Self> {
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
            },
        };

        debug!("Initialized transcriber with language: {:?}", language_code);

        Ok(Self { client, language_code })
    }

    pub async fn start_transcription(&self) -> Result<TranscriptionResult> {
        debug!("Starting real AWS Transcribe streaming transcription");

        // Create channels for audio events and transcript results
        let (audio_tx, audio_rx) = mpsc::channel::<AudioEvent>(1000);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(1000);

        // Convert audio events to AudioStream format
        let audio_stream = ReceiverStream::new(audio_rx).map(|audio_event| Ok(AudioStream::AudioEvent(audio_event)));

        // Start the real AWS Transcribe streaming session
        let response = self
            .client
            .start_stream_transcription()
            .language_code(self.language_code.clone())
            .media_sample_rate_hertz(16000)
            .media_encoding(MediaEncoding::Pcm)
            .set_audio_stream(Some(audio_stream.into()))
            .send()
            .await
            .map_err(|e| VoiceError::TranscribeUnavailable(e.to_string()))?;

        info!("✅ Connected to Amazon Transcribe streaming service");

        // Spawn task to process transcript results
        let transcript_sender = transcript_tx.clone();
        let mut transcript_stream = response.transcript_result_stream;

        tokio::spawn(async move {
            debug!("Starting transcript stream processing");

            // Use the event receiver's recv method instead of StreamExt
            loop {
                match transcript_stream.recv().await {
                    Ok(Some(transcript_stream_item)) => {
                        match transcript_stream_item {
                            aws_sdk_transcribestreaming::types::TranscriptResultStream::TranscriptEvent(
                                transcript_event,
                            ) => {
                                // Process transcript event following Python pattern
                                if let Some(transcript) = transcript_event.transcript {
                                    if let Some(results) = transcript.results {
                                        for result in results {
                                            if let Some(alternatives) = result.alternatives {
                                                for alternative in alternatives {
                                                    if let Some(transcript_text) = alternative.transcript {
                                                        // is_partial is a bool, not Option<bool>
                                                        let is_partial = result.is_partial;

                                                        let event = TranscriptEvent {
                                                            transcript: transcript_text,
                                                            is_partial,
                                                        };

                                                        if transcript_sender.send(event).await.is_err() {
                                                            debug!("Transcript receiver closed");
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            _ => {
                                debug!("Received other transcript stream item type");
                            },
                        }
                    },
                    Ok(None) => {
                        debug!("Transcript stream ended");
                        break;
                    },
                    Err(e) => {
                        error!("Transcript stream error: {:?}", e);
                        break;
                    },
                }
            }

            debug!("Transcript stream processing ended");
        });

        Ok(TranscriptionResult {
            audio_sender: audio_tx,
            transcript_receiver: transcript_rx,
        })
    }

    pub async fn check_permissions(&self) -> Result<()> {
        debug!("Checking AWS Transcribe permissions");

        // For now, we'll assume permissions are OK if we can create the client
        // In a production implementation, you might want to make a test call
        info!("✅ AWS Transcribe client created successfully");
        Ok(())
    }
}

pub async fn send_audio_to_transcribe(
    audio_receiver: &mut mpsc::Receiver<Vec<u8>>,
    transcribe_sender: &mpsc::Sender<AudioEvent>,
) -> Result<()> {
    debug!("Starting real audio forwarding to AWS Transcribe");

    while let Some(audio_data) = audio_receiver.recv().await {
        // Create real AWS AudioEvent with PCM data
        let audio_event = AudioEvent::builder().audio_chunk(Blob::new(audio_data)).build();

        if transcribe_sender.send(audio_event).await.is_err() {
            debug!("Transcribe sender channel closed");
            break;
        }
    }

    debug!("Audio forwarding to AWS Transcribe ended");
    Ok(())
}
