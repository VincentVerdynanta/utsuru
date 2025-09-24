use bytes::Bytes;
use davey::{Codec, DaveSession, MediaType};
use serde_json::json;
use std::{
    collections::HashSet,
    error::Error as StdError,
    fmt::{Display, Formatter, Result as FmtResult},
    num::ParseIntError,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{
        Notify, RwLock,
        mpsc::{self, error::SendError},
        oneshot::{self, error::RecvError},
    },
    time::sleep,
};
use tokio_websockets::{Message as WebSocketMessage, Payload};
use tracing::debug;
use twilight_gateway::{Intents, Shard, ShardId};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use uuid::Uuid;
use webrtc::{
    api::media_engine::{MIME_TYPE_H264, MIME_TYPE_OPUS},
    media::Sample,
    peer_connection::sdp::{sdp_type::RTCSdpType, session_description::RTCSessionDescription},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{TrackLocal, track_local_static_sample::TrackLocalStaticSample},
};

use super::Mirror;
use crate::error::{Error, ErrorType};
use crate::utils::{h264_parser::parse_sps, h264_synthesizer::synthesize_sps};

mod dave;
mod endpoint;
mod gateway;
mod heartbeat;

const NALU_SHORT_START_SEQUENCE_SIZE: usize = 3;
const START_CODE_HIGHEST_POSSIBLE_VALUE: u8 = 1;
const START_CODE_END_BYTE_VALUE: u8 = 1;
const START_CODE_LEADING_BYTES_VALUE: u8 = 0;

pub struct DiscordLiveBuilder {
    token: Box<str>,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
}

impl DiscordLiveBuilder {
    pub fn new(token: impl AsRef<str>, guild_id: u64, channel_id: u64) -> Self {
        Self {
            token: token.as_ref().into(),
            guild_id: Id::new(guild_id),
            channel_id: Id::new(channel_id),
        }
    }

    pub async fn connect(
        self,
        trace_tx: Option<mpsc::UnboundedSender<DiscordLiveBuilderState>>,
    ) -> Result<DiscordLive, Error<dyn ErrorInner>> {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let token = String::from(self.token.as_ref());

        let intents =
            Intents::GUILD_MESSAGES | Intents::GUILD_VOICE_STATES | Intents::MESSAGE_CONTENT;
        let shard = Shard::new(ShardId::ONE, token, intents);

        let (voice_tx, voice_rx) = oneshot::channel();
        let voice_tx = Some(voice_tx);
        let (rtcsrv_tx, rtcsrv_rx) = oneshot::channel();
        let rtcsrv_tx = Some(rtcsrv_tx);
        let (wsconn_tx, wsconn_rx) = oneshot::channel();
        let wsconn_tx = Some(wsconn_tx);
        let (feed_tx, feed_rx) = oneshot::channel();
        let (nego_tx, nego_rx) = oneshot::channel();
        let nego_tx = Some(nego_tx);
        let (connected_tx, connected_rx) = oneshot::channel();
        let connected_tx = Some(connected_tx);
        let (remote_tx, remote_rx) = oneshot::channel();
        let remote_tx = Some(remote_tx);
        let (heartbeat_tx, heartbeat_rx) = oneshot::channel();
        let heartbeat_tx = Some(heartbeat_tx);
        let (instance_tx, instance_rx) = oneshot::channel();
        let instance_tx = Some(instance_tx);
        let (egress_tx, egress_rx) = mpsc::unbounded_channel();
        let (nonce_tx, nonce_rx) = mpsc::unbounded_channel();
        let (dave_tx, dave_rx) = mpsc::unbounded_channel();

        let audio_payload = 111;
        let audio_codec = "opus";
        let mut audio_mid: u8 = 0;
        let mut audio_ssrc: u32 = 0;
        let video_payload = 102;
        let video_codec = "H264";
        let video_rtxpayload = 103;
        let mut video_mid: u8 = 1;
        let mut video_ssrc: u32 = 0;
        let mut video_rtxssrc: u32 = 0;

        let notify = Arc::new(Notifier::new());

        if let Err(e) = gateway::handle(&notify, self, shard, voice_tx, rtcsrv_tx, wsconn_tx).await
        {
            notify.close();
            return Err(Error {
                kind: e.kind,
                source: e.source.map(|source| source as Box<dyn ErrorInner>),
            });
        }

        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::VoiceConnecting));
        let (user_id, session_id) = voice_rx.await?;
        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::StreamCreating));
        let (server, channel) = rtcsrv_rx.await?;
        let channel_id: Result<u64, _> = channel.parse();
        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointWSConnecting));
        let (token, endpoint) = wsconn_rx.await?;
        if let Err(e) = endpoint::handle(
            &notify,
            user_id.to_string(),
            session_id,
            server,
            channel,
            token,
            endpoint,
            audio_payload,
            audio_codec,
            video_payload,
            video_codec,
            video_rtxpayload,
            egress_rx,
            feed_tx,
            nego_tx,
            connected_tx,
            remote_tx,
            nonce_tx,
            heartbeat_tx,
            &dave_tx,
        )
        .await
        {
            notify.close();
            return Err(Error {
                kind: e.kind,
                source: e.source.map(|source| source as Box<dyn ErrorInner>),
            });
        }

        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointRTCCreating));
        let (peer_connection, audio_rtp_sender, video_rtp_sender, streams) = feed_rx.await?;

        let heartbeat_interval = heartbeat_rx.await?;
        if let Err(e) = heartbeat::handle(&notify, heartbeat_interval, &egress_tx, nonce_rx).await {
            notify.close();
            return Err(Error {
                kind: e.kind,
                source: e.source.map(|source| source as Box<dyn ErrorInner>),
            });
        }

        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointRTCNegotiation));
        nego_rx.await?;
        if let Err(e) = dave::handle(&notify, &egress_tx, dave_rx, instance_tx).await {
            notify.close();
            return Err(Error {
                kind: e.kind,
                source: e.source.map(|source| source as Box<dyn ErrorInner>),
            });
        }

        let offer = peer_connection.create_offer(None).await?;
        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        peer_connection.set_local_description(offer).await?;
        let _ = gather_complete.recv().await;
        let local_desc = peer_connection.local_description().await.ok_or(Error {
            kind: ErrorType::DiscordEndpoint,
            source: None,
        })?;

        let sdp = local_desc.unmarshal()?;
        let mut attributes = HashSet::new();
        for attribute in sdp.attributes {
            if attribute.key.as_str() == "fingerprint" {
                if let Some(value) = attribute.value {
                    attributes.insert(format!("a={}:{}", attribute.key, value));
                } else {
                    attributes.insert(format!("a={}", attribute.key));
                }
            }
        }
        for media in sdp.media_descriptions {
            for attribute in media.attributes {
                match attribute.key.as_str() {
                    "ice-ufrag" | "ice-pwd" | "ice-options" | "extmap" | "rtpmap" => {
                        if let Some(value) = attribute.value {
                            attributes.insert(format!("a={}:{}", attribute.key, value));
                        } else {
                            attributes.insert(format!("a={}", attribute.key));
                        }
                    }
                    "ssrc" => {
                        if media.media_name.media.as_str() == "audio"
                            && let Some(value) = attribute.value
                        {
                            let mut value = value.split_whitespace();
                            audio_ssrc = value
                                .next()
                                .ok_or(Error {
                                    kind: ErrorType::DiscordEndpoint,
                                    source: None,
                                })?
                                .parse()?;
                        }
                    }
                    "ssrc-group" => {
                        if media.media_name.media.as_str() == "video"
                            && let Some(value) = attribute.value
                        {
                            let mut value = value.split_whitespace();
                            let _ = value.next();
                            video_ssrc = value
                                .next()
                                .ok_or(Error {
                                    kind: ErrorType::DiscordEndpoint,
                                    source: None,
                                })?
                                .parse()?;
                            video_rtxssrc = value
                                .next()
                                .ok_or(Error {
                                    kind: ErrorType::DiscordEndpoint,
                                    source: None,
                                })?
                                .parse()?;
                        }
                    }
                    "mid" => match media.media_name.media.as_str() {
                        "audio" => {
                            if let Some(value) = attribute.value {
                                audio_mid = value
                                    .split_whitespace()
                                    .next()
                                    .ok_or(Error {
                                        kind: ErrorType::DiscordEndpoint,
                                        source: None,
                                    })?
                                    .parse()?;
                            }
                        }
                        "video" => {
                            if let Some(value) = attribute.value {
                                video_mid = value
                                    .split_whitespace()
                                    .next()
                                    .ok_or(Error {
                                        kind: ErrorType::DiscordEndpoint,
                                        source: None,
                                    })?
                                    .parse()?;
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        let attributes = attributes.into_iter().collect::<Vec<_>>().join("\n");

        let sdp = format!("a=extmap-allow-mixed\n{}", attributes);
        let payload = json!({
            "op": 1,
            "d": {
                "protocol": "webrtc",
                "data": sdp,
                "sdp": sdp,
                "codecs": [
                    {"name": audio_codec, "type": "audio", "priority": 1000, "payload_type": audio_payload, "rtx_payload_type": null},
                    {"name": video_codec, "type": "video", "priority": 1000, "payload_type": video_payload, "rtx_payload_type": video_rtxpayload}
                ],
                "rtc_connection_id": Uuid::new_v4().to_string()
            }
        });
        egress_tx.send(WebSocketMessage::text(payload.to_string()))?;
        debug!("[WebRTC] offer sent, waiting for answer");

        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointWSSDP));
        let (remote_sdp, dave_protocol_version, external_payload) = remote_rx.await?;

        let mut answer = RTCSessionDescription::default();
        answer.sdp_type = RTCSdpType::Answer;
        let remote_sdp = remote_sdp
            .replace("ICE/SDP", &format!("UDP/TLS/RTP/SAVPF {audio_payload}"))
            .replace("\n", "\r\n");
        let remote_sdp = format!(
            "v=0\r\no=- 1420070400000 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=msid-semantic: WMS *\r\na=group:BUNDLE 0 1\r\n\
            {remote_sdp}"
        );
        answer.sdp = remote_sdp;

        let parsed = answer.unmarshal()?;
        let port = &parsed.media_descriptions[0].media_name.port.value;
        let connection = &parsed.media_descriptions[0].connection_information;
        let attributes = &parsed.media_descriptions[0].attributes;
        let setup = "passive";
        let direction = "inactive";
        let remote_sdp = format!(
            "v=0\r\no=- 1420070400000 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=msid-semantic: WMS *\r\na=group:BUNDLE 0 1\r\n\
            m=audio {port} UDP/TLS/RTP/SAVPF {audio_payload}\r\na=rtpmap:{audio_payload} {audio_codec}/48000/2\r\na=fmtp:{audio_payload} minptime=10;useinbandfec=1;usedtx=0\r\na=rtcp-fb:{audio_payload} transport-cc\r\na=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\r\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\na=setup:{setup}\r\na=mid:{audio_mid}\r\na=maxptime:60\r\na={direction}\r\na=rtcp-mux\r\n\
            m=video {port} UDP/TLS/RTP/SAVPF {video_payload} {video_rtxpayload}\r\na=rtpmap:{video_payload} {video_codec}/90000\r\na=rtpmap:{video_rtxpayload} rtx/90000\r\na=fmtp:{video_payload} x-google-max-bitrate=2500;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f\r\na=fmtp:{video_rtxpayload} apt={video_payload}\r\na=rtcp-fb:{video_payload} ccm fir\r\na=rtcp-fb:{video_payload} nack\r\na=rtcp-fb:{video_payload} nack pli\r\na=rtcp-fb:{video_payload} goog-remb\r\na=rtcp-fb:{video_payload} transport-cc\r\na=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\r\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\na=extmap:14 urn:ietf:params:rtp-hdrext:toffset\r\na=extmap:13 urn:3gpp:video-orientation\r\na=extmap:5 http://www.webrtc.org/experiments/rtp-hdrext/playout-delay\r\na=setup:{setup}\r\na=mid:{video_mid}\r\na={direction}\r\na=rtcp-mux\r\n"
        );
        answer.sdp = remote_sdp;

        let mut parsed = answer.unmarshal()?;
        for media in &mut parsed.media_descriptions {
            media.connection_information = connection.clone();
            for attribute in attributes {
                media.attributes.push(attribute.clone());
            }
        }
        let remote_sdp = parsed.marshal();
        let inactive_sdp = RTCSessionDescription::answer(remote_sdp)?;

        let direction = "recvonly";
        let remote_sdp = format!(
            "v=0\r\no=- 1420070400000 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=msid-semantic: WMS *\r\na=group:BUNDLE 0 1\r\n\
            m=audio {port} UDP/TLS/RTP/SAVPF {audio_payload}\r\na=rtpmap:{audio_payload} {audio_codec}/48000/2\r\na=fmtp:{audio_payload} minptime=10;useinbandfec=1;usedtx=0\r\na=rtcp-fb:{audio_payload} transport-cc\r\na=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\r\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\na=setup:{setup}\r\na=mid:{audio_mid}\r\na=maxptime:60\r\na={direction}\r\na=rtcp-mux\r\n\
            m=video {port} UDP/TLS/RTP/SAVPF {video_payload} {video_rtxpayload}\r\na=rtpmap:{video_payload} {video_codec}/90000\r\na=rtpmap:{video_rtxpayload} rtx/90000\r\na=fmtp:{video_payload} x-google-max-bitrate=2500;level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f\r\na=fmtp:{video_rtxpayload} apt={video_payload}\r\na=rtcp-fb:{video_payload} ccm fir\r\na=rtcp-fb:{video_payload} nack\r\na=rtcp-fb:{video_payload} nack pli\r\na=rtcp-fb:{video_payload} goog-remb\r\na=rtcp-fb:{video_payload} transport-cc\r\na=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\r\na=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\na=extmap:14 urn:ietf:params:rtp-hdrext:toffset\r\na=extmap:13 urn:3gpp:video-orientation\r\na=extmap:5 http://www.webrtc.org/experiments/rtp-hdrext/playout-delay\r\na=setup:{setup}\r\na=mid:{video_mid}\r\na={direction}\r\na=rtcp-mux\r\n"
        );
        answer.sdp = remote_sdp;

        let mut parsed = answer.unmarshal()?;
        for media in &mut parsed.media_descriptions {
            media.connection_information = connection.clone();
            for attribute in attributes {
                media.attributes.push(attribute.clone());
            }
        }
        let remote_sdp = parsed.marshal();
        let recv_sdp = RTCSessionDescription::answer(remote_sdp)?;

        peer_connection
            .set_remote_description(recv_sdp.clone())
            .await?;
        debug!("[WebRTC] answer received, wait for quit event");
        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointRTCConnecting));
        connected_rx.await?;

        let local_audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "webrtc-rs".to_owned(),
        ));
        audio_rtp_sender
            .replace_track(Some(
                Arc::clone(&local_audio_track) as Arc<dyn TrackLocal + Send + Sync>
            ))
            .await?;

        let local_video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_owned(),
                ..Default::default()
            },
            "video".to_owned(),
            "webrtc-rs".to_owned(),
        ));
        video_rtp_sender
            .replace_track(Some(
                Arc::clone(&local_video_track) as Arc<dyn TrackLocal + Send + Sync>
            ))
            .await?;

        let user_id = user_id.get();
        let channel_id = channel_id?;
        dave_tx.send(DAVEPayload::OpCode4(
            dave_protocol_version,
            user_id,
            channel_id,
            local_audio_track,
            local_video_track,
        ))?;
        dave_tx.send(DAVEPayload::Binary(external_payload))?;
        trace_tx
            .as_ref()
            .map(|tx| tx.send(DiscordLiveBuilderState::EndpointDAVECreating));
        let dave_instance = instance_rx.await?;

        let payload = json!({
            "op": 5,
            "d": {
                "speaking": 1,
                "delay": 5,
                "ssrc": 0
            }
        });
        egress_tx.send(WebSocketMessage::text(payload.to_string()))?;

        let payload = json!({
            "op": 12,
            "d": {
                "audio_ssrc": audio_ssrc,
                "video_ssrc": video_ssrc,
                "rtx_ssrc": video_rtxssrc,
                "streams": [{
                    "type": "video",
                    "rid": "100",
                    "ssrc": video_ssrc,
                    "active": true,
                    "quality": 100,
                    "rtx_ssrc": video_rtxssrc,
                    "max_bitrate": 3500000,
                    "max_framerate": 30,
                    "max_resolution": {
                        "type": "fixed",
                        "width": 1280,
                        "height": 720
                    }
                }]
            }
        });
        let active = payload.to_string();
        let payload = json!({
            "op": 12,
            "d": {
                "audio_ssrc": 0,
                "video_ssrc": streams[0].ssrc,
                "rtx_ssrc": streams[0].rtx_ssrc,
                "streams": [{
                    "type": "video",
                    "rid": "100",
                    "ssrc": streams[0].ssrc,
                    "active": false,
                    "quality": 100,
                    "rtx_ssrc": streams[0].rtx_ssrc,
                    "max_bitrate": 3500000,
                    "max_framerate": 30,
                    "max_resolution": {
                        "type": "fixed",
                        "width": 1280,
                        "height": 720
                    }
                }]
            }
        });
        let inactive = payload.to_string();
        egress_tx.send(WebSocketMessage::text(inactive))?;

        let instance_lock = dave_instance.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(300)).await;

                let Ok(_) = peer_connection
                    .set_remote_description(inactive_sdp.clone())
                    .await
                else {
                    break;
                };

                let local_audio_track = Arc::new(TrackLocalStaticSample::new(
                    RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_OPUS.to_owned(),
                        ..Default::default()
                    },
                    "audio".to_owned(),
                    "webrtc-rs".to_owned(),
                ));
                let Ok(_) = audio_rtp_sender
                    .replace_track(Some(
                        Arc::clone(&local_audio_track) as Arc<dyn TrackLocal + Send + Sync>
                    ))
                    .await
                else {
                    break;
                };
                {
                    instance_lock
                        .write()
                        .await
                        .replace_audio_track(local_audio_track);
                }

                let local_video_track = Arc::new(TrackLocalStaticSample::new(
                    RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_H264.to_owned(),
                        ..Default::default()
                    },
                    "video".to_owned(),
                    "webrtc-rs".to_owned(),
                ));
                let Ok(_) = video_rtp_sender
                    .replace_track(Some(
                        Arc::clone(&local_video_track) as Arc<dyn TrackLocal + Send + Sync>
                    ))
                    .await
                else {
                    break;
                };
                {
                    instance_lock
                        .write()
                        .await
                        .replace_video_track(local_video_track);
                }

                let Ok(_) = peer_connection
                    .set_remote_description(recv_sdp.clone())
                    .await
                else {
                    break;
                };
            }
        });

        Ok(DiscordLive {
            notify,
            active,
            dave_instance,
            egress_tx,
        })
    }
}

pub struct DiscordLive {
    notify: Arc<Notifier>,
    active: String,
    dave_instance: Arc<RwLock<DAVEInstance>>,
    egress_tx: mpsc::UnboundedSender<WebSocketMessage>,
}

impl Mirror for DiscordLive {
    fn write_audio_sample<'a>(
        &'a self,
        payload: &'a mut Sample,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async {
            if self.notify.is_closed() {
                return Err(Error {
                    kind: ErrorType::DiscordEndpoint,
                    source: None,
                });
            }
            self.dave_instance
                .write()
                .await
                .write_audio_sample(payload)
                .await
                .map_err(|err| Error {
                    kind: ErrorType::DiscordEndpoint,
                    source: Some(err.into()),
                })
        })
    }

    fn write_video_sample<'a>(
        &'a self,
        payload: &'a mut Sample,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async {
            if self.notify.is_closed() {
                return Err(Error {
                    kind: ErrorType::DiscordEndpoint,
                    source: None,
                });
            }
            self.dave_instance
                .write()
                .await
                .write_video_sample(payload)
                .await
                .map_err(|err| Error {
                    kind: ErrorType::DiscordEndpoint,
                    source: Some(err.into()),
                })
        })
    }

    fn call_connected_callback(&self) -> Result<(), Error> {
        if self.notify.is_closed() {
            return Err(Error {
                kind: ErrorType::DiscordEndpoint,
                source: None,
            });
        }
        self.egress_tx
            .send(WebSocketMessage::text(self.active.clone()))
            .map_err(|err| Error {
                kind: ErrorType::DiscordEndpoint,
                source: Some(err.into()),
            })
    }

    fn close(&self) {
        self.notify.close()
    }
}

struct DAVEInstance {
    session: DaveSession,
    dave_protocol_version: u16,
    local_audio_track: Arc<TrackLocalStaticSample>,
    local_video_track: Arc<TrackLocalStaticSample>,
}

impl DAVEInstance {
    fn get_session(&mut self) -> &mut DaveSession {
        &mut self.session
    }

    fn set_dave_protocol_version(&mut self, version: u16) -> u16 {
        self.dave_protocol_version = version;
        self.dave_protocol_version
    }

    fn replace_audio_track(&mut self, track: Arc<TrackLocalStaticSample>) {
        self.local_audio_track = track;
    }

    fn replace_video_track(&mut self, track: Arc<TrackLocalStaticSample>) {
        self.local_video_track = track;
    }

    async fn write_audio_sample(&mut self, payload: &mut Sample) -> Result<(), webrtc::Error> {
        if self.dave_protocol_version == 0 || !self.session.is_ready() {
            return self.local_audio_track.write_sample(payload).await;
        }

        let Ok(data) = self
            .session
            .encrypt(MediaType::AUDIO, Codec::OPUS, &payload.data)
        else {
            return self.local_audio_track.write_sample(payload).await;
        };
        payload.data = Bytes::copy_from_slice(&data);

        self.local_audio_track.write_sample(payload).await
    }

    async fn write_video_sample(&mut self, payload: &mut Sample) -> Result<(), webrtc::Error> {
        if self.dave_protocol_version == 0 || !self.session.is_ready() {
            return self.local_video_track.write_sample(payload).await;
        }

        let mut data = Vec::new();
        let mut nalu_indexes = Vec::new();
        let mut i = 0;
        while i < (payload.data.len() - NALU_SHORT_START_SEQUENCE_SIZE) {
            if payload.data[i + 2] > START_CODE_HIGHEST_POSSIBLE_VALUE {
                i += NALU_SHORT_START_SEQUENCE_SIZE;
            } else if payload.data[i + 1] != START_CODE_LEADING_BYTES_VALUE {
                i += 2;
            } else if payload.data[i] != START_CODE_LEADING_BYTES_VALUE
                || payload.data[i + 2] != START_CODE_END_BYTE_VALUE
            {
                i += 1;
            } else {
                if i >= 1 && payload.data[i - 1] == START_CODE_LEADING_BYTES_VALUE {
                    nalu_indexes.push((i - 1, 4));
                } else {
                    nalu_indexes.push((i, 3));
                }
                i += NALU_SHORT_START_SEQUENCE_SIZE;
            }
        }

        for pos in 0..nalu_indexes.len() {
            let (nalu, start_size) = nalu_indexes[pos];
            let next_nalu = nalu_indexes
                .get(pos + 1)
                .map(|v| v.0)
                .unwrap_or(payload.data.len());
            match payload.data[nalu + start_size] & 0x1F {
                1 | 5 | 8 => {
                    data.extend_from_slice(&payload.data[nalu..next_nalu]);
                }
                7 => {
                    let (mut sps, _) =
                        parse_sps(&payload.data[(nalu + start_size + 1)..next_nalu]).unwrap();
                    if !sps.vui_parameters.bitstream_restriction_flag {
                        sps.vui_parameters.bitstream_restriction_flag = true;
                        sps.vui_parameters.motion_vectors_over_pic_boundaries_flag = true;
                        sps.vui_parameters.max_bytes_per_pic_denom = 2;
                        sps.vui_parameters.max_bits_per_mb_denom = 1;
                        sps.vui_parameters.log2_max_mv_length_horizontal = 16;
                        sps.vui_parameters.log2_max_mv_length_vertical = 16;
                        sps.vui_parameters.max_num_reorder_frames = 0;
                        sps.vui_parameters.max_dec_frame_buffering = sps.max_num_ref_frames as u32;
                    }
                    data.extend_from_slice(&payload.data[nalu..][..(start_size + 1)]);
                    synthesize_sps(&sps, &mut data, false).unwrap();
                }
                _ => {}
            }
        }

        let Ok(data) = self.session.encrypt(MediaType::VIDEO, Codec::H264, &data) else {
            return self.local_video_track.write_sample(payload).await;
        };
        payload.data = Bytes::copy_from_slice(&data);

        self.local_video_track.write_sample(payload).await
    }
}

enum DAVEPayload {
    Binary(Payload),
    OpCode4(
        u16,
        u64,
        u64,
        Arc<TrackLocalStaticSample>,
        Arc<TrackLocalStaticSample>,
    ),
    OpCode11(Vec<String>),
    OpCode13(String),
    OpCode21(u16, u16),
    OpCode22(u16),
    OpCode24(u16, u8),
}

pub(super) struct Notifier {
    is_closed: AtomicBool,
    gateway: Arc<Notify>,
    endpoint: Arc<Notify>,
    heartbeat: Arc<Notify>,
    dave: Arc<Notify>,
}

impl Notifier {
    fn new() -> Self {
        Self {
            is_closed: AtomicBool::new(false),
            gateway: Arc::new(Notify::new()),
            endpoint: Arc::new(Notify::new()),
            heartbeat: Arc::new(Notify::new()),
            dave: Arc::new(Notify::new()),
        }
    }

    fn close(&self) {
        self.gateway.notify_one();
        self.endpoint.notify_one();
        self.heartbeat.notify_one();
        self.dave.notify_one();
        self.is_closed.store(true, Ordering::Relaxed);
    }

    fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }
}

pub enum DiscordLiveBuilderState {
    VoiceConnecting,
    StreamCreating,
    EndpointWSConnecting,
    EndpointWSSDP,
    EndpointRTCCreating,
    EndpointRTCNegotiation,
    EndpointRTCConnecting,
    EndpointDAVECreating,
}

impl Display for DiscordLiveBuilderState {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            DiscordLiveBuilderState::VoiceConnecting => f.write_str("connecting to voice channel"),
            DiscordLiveBuilderState::StreamCreating => {
                f.write_str("creating new live stream session")
            }
            DiscordLiveBuilderState::EndpointWSConnecting => {
                f.write_str("connecting to live stream endpoint")
            }
            DiscordLiveBuilderState::EndpointWSSDP => {
                f.write_str("waiting remote sdp from live stream endpoint")
            }
            DiscordLiveBuilderState::EndpointRTCCreating => f.write_str("creating new rtc client"),
            DiscordLiveBuilderState::EndpointRTCNegotiation => {
                f.write_str("rtc client currently applying all changes still pending")
            }
            DiscordLiveBuilderState::EndpointRTCConnecting => {
                f.write_str("rtc client currently connecting to live stream endpoint")
            }
            DiscordLiveBuilderState::EndpointDAVECreating => {
                f.write_str("creating new dave session")
            }
        }
    }
}

pub trait ErrorInner: StdError + Send + Sync {}

impl<T: StdError + Send + Sync> ErrorInner for T {}

impl StdError for Error<dyn ErrorInner> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn StdError + 'static))
    }
}

impl From<RecvError> for Error<dyn ErrorInner> {
    fn from(err: RecvError) -> Self {
        Self {
            kind: ErrorType::DiscordIPC,
            source: Some(Box::new(err)),
        }
    }
}

impl From<webrtc::Error> for Error<dyn ErrorInner> {
    fn from(err: webrtc::Error) -> Self {
        Self {
            kind: ErrorType::DiscordEndpoint,
            source: Some(Box::new(err)),
        }
    }
}

impl From<ParseIntError> for Error<dyn ErrorInner> {
    fn from(err: ParseIntError) -> Self {
        Self {
            kind: ErrorType::DiscordEndpoint,
            source: Some(Box::new(err)),
        }
    }
}

impl From<SendError<DAVEPayload>> for Error<dyn ErrorInner> {
    fn from(err: SendError<DAVEPayload>) -> Self {
        Self {
            kind: ErrorType::DiscordIPC,
            source: Some(Box::new(err)),
        }
    }
}

impl From<SendError<WebSocketMessage>> for Error<dyn ErrorInner> {
    fn from(err: SendError<WebSocketMessage>) -> Self {
        Self {
            kind: ErrorType::DiscordIPC,
            source: Some(Box::new(err)),
        }
    }
}
