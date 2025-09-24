use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::{
    Deserialize, Deserializer,
    de::{self, IntoDeserializer},
};
use serde_json::{
    from_str, json,
    value::{RawValue, to_raw_value},
};
use std::{collections::HashMap, error::Error as StdError, sync::Arc};
use tokio::{
    sync::{
        mpsc::{self, error::SendError},
        oneshot::{self, error::RecvError},
    },
    task::JoinHandle,
};
use tokio_websockets::{ClientBuilder, Connector, Limits, Message as WebSocketMessage};
use tracing::{debug, info, warn};
use twilight_gateway::error::ChannelError;
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_H264, MIME_TYPE_OPUS, MediaEngine},
        setting_engine::SettingEngine,
    },
    ice_transport::ice_connection_state::RTCIceConnectionState,
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        policy::{
            bundle_policy::RTCBundlePolicy, ice_transport_policy::RTCIceTransportPolicy,
            rtcp_mux_policy::RTCRtcpMuxPolicy,
        },
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_sender::RTCRtpSender,
    },
};

use super::{DAVEPayload, Notifier};
use crate::error::{Error, ErrorType};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub async fn handle(
    notify: &Arc<Notifier>,
    user_id: String,
    session_id: String,
    server: String,
    channel: String,
    token: String,
    endpoint: String,
    audio_payload: u8,
    audio_codec: &'static str,
    video_payload: u8,
    video_codec: &'static str,
    video_rtxpayload: u8,
    mut egress_rx: mpsc::UnboundedReceiver<WebSocketMessage>,
    feed_tx: oneshot::Sender<(
        Arc<RTCPeerConnection>,
        Arc<RTCRtpSender>,
        Arc<RTCRtpSender>,
        Vec<GatewayStream>,
    )>,
    nego_tx: Option<oneshot::Sender<()>>,
    connected_tx: Option<oneshot::Sender<()>>,
    mut remote_tx: Option<oneshot::Sender<(String, u16, tokio_websockets::Payload)>>,
    nonce_tx: mpsc::UnboundedSender<u64>,
    mut heartbeat_tx: Option<oneshot::Sender<u64>>,
    dave_tx: &mpsc::UnboundedSender<DAVEPayload>,
) -> Result<JoinHandle<Result<(), Error<dyn ErrorInner>>>, Error<dyn ErrorInner>> {
    let uri = format!("wss://{}/?v=9", endpoint);
    let tls = Arc::new(Connector::new()?);
    let (mut client, _) = ClientBuilder::new()
        .uri(&uri)
        .expect("URL should be valid")
        .limits(Limits::unlimited())
        .connector(&tls)
        .connect()
        .await?;

    debug!("[WS] sending identify");
    let payload = json!({
        "op": 0,
        "d": {
            "server_id": server,
            "channel_id": channel,
            "user_id": user_id,
            "session_id": session_id,
            "token": token,
            "max_dave_protocol_version": 1,
            "video": true,
            "streams":[{
                "type": "screen",
                "rid": "100",
                "quality": 100
            }]
        }
    });
    client
        .send(WebSocketMessage::text(payload.to_string()))
        .await?;

    let (peer_connection, audio_rtp_sender, video_rtp_sender) = init_feed(
        audio_payload,
        audio_codec,
        video_payload,
        video_codec,
        video_rtxpayload,
        nego_tx,
        connected_tx,
    )
    .await?;
    let mut feed = Some((feed_tx, peer_connection, audio_rtp_sender, video_rtp_sender));

    let notifier = notify.clone();
    let dave_tx = dave_tx.clone();
    Ok(tokio::spawn(async move {
        let notify = notifier.endpoint.notified();
        let mut notify = Box::pin(notify);

        let mut session = None;
        loop {
            let (ingress, egress);
            tokio::select! {
                res = client.next() => (ingress, egress) = (Some(res), None),
                res = egress_rx.recv() => (ingress, egress) = (None, Some(res)),
                _ = (&mut notify) => break,
            }

            if let Some(item) = ingress {
                let Some(Ok(item)) = item else {
                    let (mut client_resume, _) = ClientBuilder::new()
                        .uri(&uri)
                        .expect("URL should be valid")
                        .limits(Limits::unlimited())
                        .connector(&tls)
                        .connect()
                        .await?;

                    let payload = json!({
                        "op": 7,
                        "d": {
                            "token": token,
                            "session_id": session_id,
                            "server_id": server,
                            "seq_ack": 1
                        }
                    });
                    client_resume
                        .send(WebSocketMessage::text(payload.to_string()))
                        .await?;

                    client = client_resume;
                    continue;
                };

                if let Some((code, _)) = item.as_close() {
                    let code = u16::from(code);
                    match code {
                        4004 | 4006..=4014 | 4016..=4020 => break,
                        _ => {}
                    }
                };

                debug!("[WS] got message from endpoint: {item:?}");
                let Some(item) = item.as_text() else {
                    let item = item.into_payload();
                    if let Some((sdp, dave_protocol_version)) = session.take()
                        && let Some(remote_tx) = remote_tx.take()
                    {
                        let _ = remote_tx.send((sdp, dave_protocol_version, item));
                        continue;
                    }
                    let _ = dave_tx.send(DAVEPayload::Binary(item));
                    continue;
                };
                let Ok(EndpointPayload(item)) = from_str(item) else {
                    continue;
                };

                match item {
                    EndpointEvent::OpCode2 { streams, .. } => {
                        if let Some((
                            feed_tx,
                            peer_connection,
                            audio_rtp_sender,
                            video_rtp_sender,
                        )) = feed.take()
                        {
                            let _ = feed_tx.send((
                                peer_connection,
                                audio_rtp_sender,
                                video_rtp_sender,
                                streams,
                            ));
                        }
                    }
                    EndpointEvent::OpCode4 {
                        sdp,
                        dave_protocol_version,
                        ..
                    } => {
                        session = Some((sdp, dave_protocol_version));
                    }
                    EndpointEvent::OpCode6 { t } => {
                        let _ = nonce_tx.send(t);
                    }
                    EndpointEvent::OpCode8 {
                        heartbeat_interval, ..
                    } => {
                        if let Some(heartbeat_tx) = heartbeat_tx.take() {
                            let _ = heartbeat_tx.send(heartbeat_interval);
                        }
                    }
                    EndpointEvent::OpCode9 {} => {}
                    EndpointEvent::OpCode11 { user_ids } => {
                        let _ = dave_tx.send(DAVEPayload::OpCode11(user_ids));
                    }
                    EndpointEvent::OpCode13 { user_id } => {
                        let _ = dave_tx.send(DAVEPayload::OpCode13(user_id));
                    }
                    EndpointEvent::OpCode21 {
                        transition_id,
                        protocol_version,
                    } => {
                        let _ =
                            dave_tx.send(DAVEPayload::OpCode21(transition_id, protocol_version));
                    }
                    EndpointEvent::OpCode22 { transition_id } => {
                        let _ = dave_tx.send(DAVEPayload::OpCode22(transition_id));
                    }
                    EndpointEvent::OpCode24 {
                        protocol_version,
                        epoch,
                    } => {
                        let _ = dave_tx.send(DAVEPayload::OpCode24(protocol_version, epoch));
                    }
                }
            }
            if let Some(item) = egress {
                let Some(payload) = item else {
                    break;
                };

                debug!("[WS] message sent to endpoint: {:?}", payload);
                if client.send(payload).await.is_err() {
                    break;
                };
            }
        }
        client.close().await?;
        warn!("[WS] endpoint closed");

        notifier.close();
        Ok(())
    }))
}

async fn init_feed(
    audio_payload: u8,
    audio_codec: &str,
    video_payload: u8,
    video_codec: &str,
    video_rtxpayload: u8,
    mut nego_tx: Option<oneshot::Sender<()>>,
    mut connected_tx: Option<oneshot::Sender<()>>,
) -> Result<(Arc<RTCPeerConnection>, Arc<RTCRtpSender>, Arc<RTCRtpSender>), Error<dyn ErrorInner>> {
    let mut m = MediaEngine::default();
    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: match video_codec {
                    "H264" => MIME_TYPE_H264.to_owned(),
                    _ => format!("video/{video_codec}"),
                },
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: video_payload,
            ..Default::default()
        },
        RTPCodecType::Video,
    )?;
    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: "video/rtx".to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: format!("apt={video_payload}"),
                rtcp_feedback: vec![],
            },
            payload_type: video_rtxpayload,
            ..Default::default()
        },
        RTPCodecType::Video,
    )?;
    m.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: match audio_codec {
                    "opus" => MIME_TYPE_OPUS.to_owned(),
                    _ => format!("audio/{audio_codec}"),
                },
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: audio_payload,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let runes: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let username_fragment = generate_crypto_random_string(4, runes);
    let password = generate_crypto_random_string(24, runes);
    let mut s = SettingEngine::default();
    s.set_ice_credentials(username_fragment, password);
    s.enable_sender_rtx(true);

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .with_setting_engine(s)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![],
        ice_transport_policy: RTCIceTransportPolicy::All,
        bundle_policy: RTCBundlePolicy::MaxBundle,
        rtcp_mux_policy: RTCRtcpMuxPolicy::Require,
        ..Default::default()
    };
    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    let mut pc = Some(peer_connection.clone());
    peer_connection.on_ice_connection_state_change(Box::new(
        move |connection_state: RTCIceConnectionState| {
            info!(
                "[WebRTC] ICE connection state changed to: {}",
                connection_state
            );
            let (connected_tx, pc) = match connection_state {
                RTCIceConnectionState::Connected => (connected_tx.take(), None),
                RTCIceConnectionState::Failed => (None, pc.take()),
                _ => (None, None),
            };
            Box::pin(async move {
                if let Some(connected_tx) = connected_tx {
                    let _ = connected_tx.send(());
                }
                if let Some(pc) = pc {
                    let _ = pc.close().await;
                    warn!("[WebRTC] closing peer");
                }
            })
        },
    ));

    peer_connection.on_negotiation_needed(Box::new(move || {
        debug!("[WebRTC] Negotiation needed");
        if let Some(nego_tx) = nego_tx.take() {
            let _ = nego_tx.send(());
        }
        Box::pin(async {})
    }));

    let audio_rtp_transceiver = peer_connection
        .add_transceiver_from_kind(RTPCodecType::Audio, None)
        .await?;
    let audio_rtp_sender = audio_rtp_transceiver.sender().await;
    let sender = audio_rtp_sender.clone();
    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = sender.read(&mut rtcp_buf).await {}
        debug!("[WebRTC] audio rtp_sender.read loop exit");
        Ok::<(), ()>(())
    });

    let video_rtp_transceiver = peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, None)
        .await?;
    let video_rtp_sender = video_rtp_transceiver.sender().await;
    let sender = video_rtp_sender.clone();
    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = sender.read(&mut rtcp_buf).await {}
        debug!("[WebRTC] video rtp_sender.read loop exit");
        Ok::<(), ()>(())
    });

    Ok((peer_connection, audio_rtp_sender, video_rtp_sender))
}

fn generate_crypto_random_string(n: usize, runes: &[u8]) -> String {
    let mut rng = rand::rng();

    let rand_string: String = (0..n)
        .map(|_| {
            let idx = rng.random_range(0..runes.len());
            runes[idx] as char
        })
        .collect();

    rand_string
}

#[derive(Debug)]
struct EndpointPayload(EndpointEvent);

impl<'de> Deserialize<'de> for EndpointPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value: HashMap<String, &RawValue> = HashMap::deserialize(deserializer)?;

        let op = value
            .get("op")
            .ok_or_else(|| de::Error::missing_field("op"))?;
        let op = to_raw_value(&op.to_string()).map_err(de::Error::custom)?;
        value.insert("op".to_string(), &op);

        EndpointEvent::deserialize(value.into_deserializer())
            .map(Self)
            .map_err(de::Error::custom)
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "op", content = "d")]
enum EndpointEvent {
    #[serde(rename = "2")]
    OpCode2 {
        streams: Vec<GatewayStream>,
        #[allow(dead_code)]
        ssrc: u32,
        #[allow(dead_code)]
        port: u16,
        #[allow(dead_code)]
        modes: Vec<String>,
        #[allow(dead_code)]
        ip: String,
        #[allow(dead_code)]
        experiments: Vec<String>,
    },
    #[serde(rename = "4")]
    OpCode4 {
        #[allow(dead_code)]
        video_codec: String,
        sdp: String,
        #[allow(dead_code)]
        media_session_id: String,
        #[allow(dead_code)]
        dave_protocol_version: u16,
        #[allow(dead_code)]
        audio_codec: String,
    },
    #[serde(rename = "6")]
    OpCode6 { t: u64 },
    #[serde(rename = "8")]
    OpCode8 {
        #[allow(dead_code)]
        v: u8,
        heartbeat_interval: u64,
    },
    #[serde(rename = "9")]
    OpCode9 {},
    #[serde(rename = "11")]
    OpCode11 { user_ids: Vec<String> },
    #[serde(rename = "13")]
    OpCode13 { user_id: String },
    #[serde(rename = "21")]
    OpCode21 {
        transition_id: u16,
        protocol_version: u16,
    },
    #[serde(rename = "22")]
    OpCode22 { transition_id: u16 },
    #[serde(rename = "24")]
    OpCode24 { protocol_version: u16, epoch: u8 },
}

#[derive(Deserialize, Debug)]
pub struct GatewayStream {
    #[allow(dead_code)]
    pub r#type: String,
    pub ssrc: u32,
    pub rtx_ssrc: u32,
    #[allow(dead_code)]
    pub rid: String,
    #[allow(dead_code)]
    pub quality: u8,
    #[allow(dead_code)]
    pub active: bool,
    #[allow(dead_code)]
    pub max_bitrate: Option<u32>,
    #[allow(dead_code)]
    pub max_framerate: Option<u8>,
    #[allow(dead_code)]
    pub max_resolution: Option<GatewayResolution>,
}

#[derive(Deserialize, Debug)]
pub struct GatewayResolution {
    #[allow(dead_code)]
    r#type: String,
    #[allow(dead_code)]
    width: u16,
    #[allow(dead_code)]
    height: u16,
}

pub trait ErrorInner: super::ErrorInner {}

impl<T: super::ErrorInner> ErrorInner for T {}

impl StdError for Error<dyn ErrorInner> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn StdError + 'static))
    }
}

impl From<SendError<String>> for Error<dyn ErrorInner> {
    fn from(err: SendError<String>) -> Self {
        Self {
            kind: ErrorType::DiscordIPC,
            source: Some(Box::new(err)),
        }
    }
}

impl From<ChannelError> for Error<dyn ErrorInner> {
    fn from(err: ChannelError) -> Self {
        Self {
            kind: ErrorType::DiscordEndpoint,
            source: Some(Box::new(err)),
        }
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

impl From<tokio_websockets::Error> for Error<dyn ErrorInner> {
    fn from(err: tokio_websockets::Error) -> Self {
        Self {
            kind: ErrorType::DiscordEndpoint,
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
