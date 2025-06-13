use http::{Request, Response, StatusCode, header::LOCATION};
use http_body::Body;
use http_body_util::BodyExt;
use std::{
    collections::VecDeque, convert::Infallible, error::Error as StdError, net::IpAddr, pin::Pin,
    sync::Arc, time::Duration,
};
use tokio::{
    sync::{
        RwLock,
        mpsc::{self, error::SendError},
        oneshot::{self, error::RecvError},
    },
    time::sleep,
};
use tracing::{debug, info, warn};
use webrtc::{
    api::{
        APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_H264, MIME_TYPE_OPUS, MediaEngine},
        setting_engine::SettingEngine,
    },
    ice_transport::ice_connection_state::RTCIceConnectionState,
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration,
        policy::{
            bundle_policy::RTCBundlePolicy, ice_transport_policy::RTCIceTransportPolicy,
            rtcp_mux_policy::RTCRtcpMuxPolicy,
        },
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp::codecs::opus::OpusPacket,
    rtp_transceiver::{
        RTCRtpTransceiverInit,
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
};

use crate::{
    error::{Error, ErrorType},
    mirrors::Mirror,
    utils::{codecs::H264Packet, io::SampleBuilder},
};

#[derive(Clone)]
pub struct WHIP {
    inner_tx: mpsc::UnboundedSender<WHIPEvent>,
}

impl WHIP {
    pub fn new(host: IpAddr) -> Self {
        let inner = mpsc::unbounded_channel();
        let (inner_tx_a, inner_tx_b, mut inner_rx) = (inner.0.clone(), inner.0, inner.1);
        let inner: Arc<WHIPInner> = Arc::new(WHIPInner::default());

        let inner_tx = inner_tx_a;
        tokio::spawn(async move {
            let mut active = false;

            while let Some(payload) = inner_rx.recv().await {
                match payload {
                    WHIPEvent::NewRequest(offer, path, resp_tx) => {
                        if active {
                            continue;
                        }

                        let Ok(sdp) = init_peer(host, &inner, offer, inner_tx.clone()).await else {
                            let _ = resp_tx.send(Err(StatusCode::INTERNAL_SERVER_ERROR));
                            continue;
                        };

                        let resp = Response::builder()
                            .header(LOCATION, path)
                            .status(StatusCode::CREATED)
                            .body(sdp)
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
                        let _ = resp_tx.send(resp);

                        active = true;
                    }
                    WHIPEvent::EndRequest => {
                        active = false;
                    }
                    WHIPEvent::RetrieveMirrors(mirrors_tx) => {
                        let mirrors = inner.view_mirrors().await;
                        let _ = mirrors_tx.send(mirrors);
                    }
                    WHIPEvent::NewMirror(mirror, done_tx) => {
                        inner.add_mirror(mirror).await;
                        if active {
                            inner.call_connected_callback().await;
                        }
                        let _ = done_tx.send(());
                    }
                    WHIPEvent::EndMirror(id, done_tx) => {
                        inner.remove_mirror(id).await;
                        let _ = done_tx.send(());
                    }
                }
            }

            inner_rx.close();
        });

        let inner_tx = inner_tx_b;
        Self { inner_tx }
    }

    async fn add_request(
        &self,
        offer: String,
        path: String,
    ) -> Result<Result<Response<String>, StatusCode>, Error<dyn ErrorInner>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.inner_tx
            .send(WHIPEvent::NewRequest(offer, path, resp_tx))?;
        resp_rx.await.map_err(Into::into)
    }

    pub async fn view_mirrors(&self) -> Result<Vec<bool>, Error<dyn ErrorInner>> {
        let (mirrors_tx, mirrors_rx) = oneshot::channel();
        self.inner_tx.send(WHIPEvent::RetrieveMirrors(mirrors_tx))?;
        mirrors_rx.await.map_err(Into::into)
    }

    pub async fn add_mirror<M: Mirror + Send + Sync + 'static>(
        &self,
        mirror: M,
    ) -> Result<(), Error<dyn ErrorInner>> {
        let (done_tx, done_rx) = oneshot::channel();
        self.inner_tx
            .send(WHIPEvent::NewMirror(Box::new(mirror), done_tx))?;
        done_rx.await.map_err(Into::into)
    }

    pub async fn remove_mirror(&self, id: usize) -> Result<(), Error<dyn ErrorInner>> {
        let (done_tx, done_rx) = oneshot::channel();
        self.inner_tx.send(WHIPEvent::EndMirror(id, done_tx))?;
        done_rx.await.map_err(Into::into)
    }

    #[allow(clippy::type_complexity)]
    pub fn into_closure<ReqBody>(
        &self,
    ) -> impl FnMut(
        Request<ReqBody>,
    ) -> Pin<
        Box<dyn Future<Output = Result<Result<Response<String>, StatusCode>, Infallible>> + Send>,
    > + Clone
    + use<ReqBody>
    where
        ReqBody: Body + Send + 'static,
        <ReqBody as Body>::Data: std::marker::Send,
        <ReqBody as Body>::Error: std::fmt::Debug,
    {
        let mut whip = Some(self.clone());
        move |req: Request<ReqBody>| {
            let whip = whip.take().unwrap();
            Box::pin(async move {
                let path = req.uri().path().to_owned();
                let offer =
                    String::from_utf8(req.into_body().collect().await.unwrap().to_bytes().into())
                        .unwrap();
                let res = whip.add_request(offer, path).await.unwrap();
                Ok(res)
            })
        }
    }
}

async fn init_peer(
    host: IpAddr,
    inner: &Arc<WHIPInner>,
    offer: String,
    inner_tx: mpsc::UnboundedSender<WHIPEvent>,
) -> Result<String, Error<dyn ErrorInner>> {
    let audio_payload = 111;
    let audio_codec = "opus";
    let video_payload = 102;
    let video_codec = "H264";
    let video_rtxpayload = 103;

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

    let mut s = SettingEngine::default();
    s.disable_srtp_replay_protection(true);
    s.set_include_loopback_candidate(true);
    if !host.is_unspecified() {
        let ip_filter = Box::new(move |ipaddr| ipaddr == host);
        s.set_ip_filter(ip_filter);
    }

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

    peer_connection
        .add_transceiver_from_kind(
            RTPCodecType::Audio,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Recvonly,
                send_encodings: vec![],
            }),
        )
        .await?;
    peer_connection
        .add_transceiver_from_kind(
            RTPCodecType::Video,
            Some(RTCRtpTransceiverInit {
                direction: RTCRtpTransceiverDirection::Recvonly,
                send_encodings: vec![],
            }),
        )
        .await?;

    let inner_track = inner.clone();
    let pc = Arc::downgrade(&peer_connection);
    peer_connection.on_track(Box::new(move |track, _, _| {
        let media_ssrc = track.ssrc();

        if track.kind() == RTPCodecType::Video {
            let pc2 = pc.clone();
            tokio::spawn(async move {
                let mut result = Result::<usize, _>::Ok(0);
                while result.is_ok() {
                    let timeout = sleep(Duration::from_secs(3));
                    tokio::pin!(timeout);

                    tokio::select! {
                        _ = timeout.as_mut() => {
                            if let Some(pc) = pc2.upgrade() {
                                result = pc.write_rtcp(&[Box::new(PictureLossIndication {
                                    sender_ssrc: 0,
                                    media_ssrc,
                                })]).await;
                            } else {
                                break;
                            }
                        }
                    };
                }
                debug!("[WebRTC] closing video pli thread");
            });
        }

        let inner_track = inner_track.clone();

        tokio::spawn(async move {
            info!(
                "[WebRTC] Track has started, of type {}: {}",
                track.payload_type(),
                track.codec().capability.mime_type
            );

            match track.kind() {
                RTPCodecType::Audio => {
                    let mut s = SampleBuilder::new(OpusPacket, 15, 48000);

                    while let Ok((rtp, _)) = track.read_rtp().await {
                        let is_emit = s.push(rtp);
                        if !is_emit {
                            s = SampleBuilder::new(OpusPacket, 15, 48000);
                        }
                        while let Some(payload) = s.pop() {
                            inner_track.write_audio_sample(&payload).await;
                        }
                    }
                }
                RTPCodecType::Video => {
                    let mut s = SampleBuilder::new(H264Packet::default(), 30, 90000);

                    while let Ok((rtp, _)) = track.read_rtp().await {
                        let is_emit = s.push(rtp);
                        if !is_emit {
                            s = SampleBuilder::new(H264Packet::default(), 30, 90000);
                        }
                        while let Some(payload) = s.pop() {
                            inner_track.write_video_sample(&payload).await;
                        }
                    }
                }
                _ => {}
            };

            warn!(
                "[WebRTC] on_track finished, of type {}: {}",
                track.payload_type(),
                track.codec().capability.mime_type
            );
        });

        Box::pin(async {})
    }));

    let mut inner_tx = Some(inner_tx);
    let mut inner_ice = Some(inner.clone());
    let mut pc = Some(peer_connection.clone());
    peer_connection.on_ice_connection_state_change(Box::new(
        move |connection_state: RTCIceConnectionState| {
            info!(
                "[WebRTC] ICE connection state changed to: {}",
                connection_state
            );
            let (inner_tx, inner_ice, pc) = match connection_state {
                RTCIceConnectionState::Connected => (None, inner_ice.take(), None),
                RTCIceConnectionState::Disconnected => (inner_tx.take(), None, None),
                RTCIceConnectionState::Failed => (None, None, pc.take()),
                _ => (None, None, None),
            };
            Box::pin(async move {
                if let Some(inner_ice) = inner_ice {
                    inner_ice.call_connected_callback().await;
                }
                if let Some(inner_tx) = inner_tx {
                    let _ = inner_tx.send(WHIPEvent::EndRequest);
                }
                if let Some(pc) = pc {
                    let _ = pc.close().await;
                    warn!("[WebRTC] closing peer");
                }
            })
        },
    ));

    debug!("[WebRTC] waiting for offer");
    let offer = RTCSessionDescription::offer(offer)?;
    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;
    let _ = gather_complete.recv().await;
    debug!("[WebRTC] offer set, sending answer");
    let local_desc = peer_connection.local_description().await.ok_or(Error {
        kind: ErrorType::WHIPPeer,
        source: None,
    })?;

    Ok(local_desc.sdp)
}

enum WHIPEvent {
    NewRequest(
        String,
        String,
        oneshot::Sender<Result<Response<String>, StatusCode>>,
    ),
    EndRequest,
    RetrieveMirrors(oneshot::Sender<Vec<bool>>),
    NewMirror(Box<dyn Mirror + Send + Sync>, oneshot::Sender<()>),
    EndMirror(usize, oneshot::Sender<()>),
}

#[derive(Default)]
struct WHIPInner {
    map: RwLock<Vec<Option<usize>>>,
    mirrors: RwLock<VecDeque<(usize, Box<dyn Mirror + Send + Sync>)>>,
}

impl WHIPInner {
    async fn view_mirrors(&self) -> Vec<bool> {
        self.map.read().await.iter().map(|&x| x.is_some()).collect()
    }

    async fn add_mirror(&self, mirror: Box<dyn Mirror + Send + Sync>) {
        let mut map = self.map.write().await;
        let mut deque = self.mirrors.write().await;

        let seq = deque.len();
        deque.push_back((map.len(), mirror));
        map.push(Some(seq));
    }

    async fn remove_mirror(&self, id: usize) {
        let mut map = self.map.write().await;
        let mut deque = self.mirrors.write().await;

        let Some(pos) = map.get_mut(id) else {
            return;
        };
        let Some(seq) = pos else {
            return;
        };
        let Some((_, mirror)) = deque.remove(*seq) else {
            return;
        };
        mirror.close();
        *pos = None;
    }

    async fn write_audio_sample(&self, payload: &Sample) {
        let mut map = self.map.write().await;
        let mut deque = self.mirrors.write().await;

        let len = deque.len();
        for seq in 0..len {
            let Some((id, mirror)) = deque.pop_front() else {
                continue;
            };
            let pos = map.get_mut(id).unwrap();
            let Ok(_) = mirror.write_audio_sample(payload).await else {
                *pos = None;
                continue;
            };
            *pos = Some(seq);
            deque.push_back((id, mirror));
        }
    }

    async fn write_video_sample(&self, payload: &Sample) {
        let mut map = self.map.write().await;
        let mut deque = self.mirrors.write().await;

        let len = deque.len();
        for seq in 0..len {
            let Some((id, mirror)) = deque.pop_front() else {
                continue;
            };
            let pos = map.get_mut(id).unwrap();
            let Ok(_) = mirror.write_video_sample(payload).await else {
                *pos = None;
                continue;
            };
            *pos = Some(seq);
            deque.push_back((id, mirror));
        }
    }

    async fn call_connected_callback(&self) {
        let mut map = self.map.write().await;
        let mut deque = self.mirrors.write().await;

        let len = deque.len();
        for seq in 0..len {
            let Some((id, mirror)) = deque.pop_front() else {
                continue;
            };
            let pos = map.get_mut(id).unwrap();
            let Ok(_) = mirror.call_connected_callback() else {
                *pos = None;
                continue;
            };
            *pos = Some(seq);
            deque.push_back((id, mirror));
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

impl From<SendError<WHIPEvent>> for Error<dyn ErrorInner> {
    fn from(err: SendError<WHIPEvent>) -> Self {
        Self {
            kind: ErrorType::WHIPIPC,
            source: Some(Box::new(err)),
        }
    }
}

impl From<RecvError> for Error<dyn ErrorInner> {
    fn from(err: RecvError) -> Self {
        Self {
            kind: ErrorType::WHIPIPC,
            source: Some(Box::new(err)),
        }
    }
}

impl From<webrtc::Error> for Error<dyn ErrorInner> {
    fn from(err: webrtc::Error) -> Self {
        Self {
            kind: ErrorType::WHIPPeer,
            source: Some(Box::new(err)),
        }
    }
}
