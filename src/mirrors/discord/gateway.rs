use futures_util::StreamExt;
use serde::{
    Deserialize, Deserializer,
    de::{self, IntoDeserializer},
};
use serde_json::{
    from_str, json,
    value::{RawValue, to_raw_value},
};
use std::{collections::HashMap, error::Error as StdError, sync::Arc};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{debug, warn};
use twilight_gateway::{
    CloseFrame, Event, EventTypeFlags, Message, Shard, StreamExt as _, error::ChannelError,
};
use twilight_model::{
    gateway::payload::outgoing::UpdateVoiceState,
    id::{Id, marker::UserMarker},
};

use super::{DiscordLiveBuilder, Notifier};
use crate::error::{Error, ErrorType};

pub async fn handle(
    notify: &Arc<Notifier>,
    dc: DiscordLiveBuilder,
    mut shard: Shard,
    mut voice_tx: Option<oneshot::Sender<(Id<UserMarker>, String)>>,
    mut rtcsrv_tx: Option<oneshot::Sender<(String, String)>>,
    mut wsconn_tx: Option<oneshot::Sender<(String, String)>>,
) -> Result<JoinHandle<Result<(), Error<dyn ErrorInner>>>, Error<dyn ErrorInner>> {
    let sender = shard.sender();

    while let Some(item) = shard.next().await {
        match item {
            Ok(Message::Close(e)) => {
                warn!("[WS] gateway error: {e:?}");
                return Err(Error {
                    kind: ErrorType::DiscordAuth,
                    source: None,
                });
            }
            Ok(Message::Text(text)) => {
                if let Ok(Payload(GatewayEvent::OpCode0(Dispatch {
                    event: DispatchEvent::Ready {},
                    ..
                }))) = from_str::<Payload>(&text)
                {
                    break;
                }
            }
            _ => {}
        };
    }

    let update = &UpdateVoiceState::new(dc.guild_id, Some(dc.channel_id), false, false);
    sender.command(update)?;

    let notifier = notify.clone();
    Ok(tokio::spawn(async move {
        let notify = notifier.gateway.notified();
        let mut notify = Box::pin(notify);

        let mut raw = false;
        loop {
            match raw {
                false => {
                    let item;
                    tokio::select! {
                        res = shard.next_event(EventTypeFlags::all()) => item = res,
                        _ = (&mut notify) => break,
                    }

                    let Some(item) = item else {
                        break;
                    };
                    let event = match item {
                        Ok(event) => event,
                        _ => continue,
                    };

                    debug!("[WS] got message from gateway: {event:?}");

                    match event {
                        Event::GatewayClose(_) => break,
                        Event::VoiceStateUpdate(data) => {
                            if let Some(voice_tx) = voice_tx.take() {
                                let _ = voice_tx.send((data.user_id, data.session_id.clone()));

                                let payload = json!({
                                    "op": 18,
                                    "d": {
                                        "type": "guild",
                                        "guild_id": dc.guild_id.to_string(),
                                        "channel_id": dc.channel_id.to_string(),
                                        "preferred_region": null
                                    }
                                });
                                let Ok(_) = sender.send(payload.to_string()) else {
                                    break;
                                };

                                let payload = json!({
                                    "op": 22,
                                    "d": {
                                        "stream_key": format!("guild:{}:{}:{}", dc.guild_id, dc.channel_id, data.user_id),
                                        "paused": false
                                    }
                                });
                                let Ok(_) = sender.send(payload.to_string()) else {
                                    break;
                                };
                            }
                            raw = true;
                        }
                        _ => {}
                    }
                }
                true => {
                    let item;
                    tokio::select! {
                        res = shard.next() => item = res,
                        _ = (&mut notify) => break,
                    }

                    let Some(item) = item else {
                        break;
                    };
                    let text = match item {
                        Ok(Message::Close(Some(CloseFrame {
                            code: 4004 | 4009..=4014,
                            ..
                        }))) => break,
                        Ok(Message::Close(None)) => break,
                        Ok(Message::Text(text)) => text,
                        _ => continue,
                    };

                    let Ok(Payload(payload)) = from_str::<Payload>(&text) else {
                        continue;
                    };
                    debug!("[WS] got message from gateway: {payload:?}");

                    if let GatewayEvent::OpCode0(Dispatch { event, .. }) = payload {
                        match event {
                            DispatchEvent::Create {
                                rtc_server_id,
                                rtc_channel_id,
                                ..
                            } => {
                                if let Some(rtcsrv_tx) = rtcsrv_tx.take() {
                                    let _ = rtcsrv_tx.send((rtc_server_id, rtc_channel_id));
                                }
                            }
                            DispatchEvent::ServerUpdate {
                                token, endpoint, ..
                            } => {
                                if let Some(wsconn_tx) = wsconn_tx.take() {
                                    let _ = wsconn_tx.send((token, endpoint));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        let update = &UpdateVoiceState::new(dc.guild_id, None, false, false);
        sender.command(update)?;
        shard.close(CloseFrame::NORMAL);
        shard.next().await;
        warn!("[WS] gateway closed");

        notifier.close();
        Ok(())
    }))
}

#[derive(Debug)]
struct Payload(GatewayEvent);

impl<'de> Deserialize<'de> for Payload {
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

        GatewayEvent::deserialize(value.into_deserializer())
            .map(Self)
            .map_err(de::Error::custom)
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "op")]
enum GatewayEvent {
    #[serde(rename = "0")]
    OpCode0(Dispatch),
    #[serde(rename = "10")]
    OpCode10 {},
}

#[derive(Deserialize, Debug)]
struct Dispatch {
    #[serde(flatten)]
    event: DispatchEvent,
    #[allow(dead_code)]
    s: u8,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "t", content = "d")]
enum DispatchEvent {
    #[serde(rename = "READY")]
    Ready {},
    #[serde(rename = "STREAM_CREATE")]
    Create {
        #[allow(dead_code)]
        viewer_ids: Vec<String>,
        #[allow(dead_code)]
        stream_key: String,
        rtc_server_id: String,
        rtc_channel_id: String,
        #[allow(dead_code)]
        region: String,
        #[allow(dead_code)]
        paused: bool,
    },
    #[serde(rename = "STREAM_SERVER_UPDATE")]
    ServerUpdate {
        token: String,
        #[allow(dead_code)]
        stream_key: String,
        #[allow(dead_code)]
        guild_id: Option<String>,
        endpoint: String,
    },
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

impl From<ChannelError> for Error<dyn ErrorInner> {
    fn from(err: ChannelError) -> Self {
        Self {
            kind: ErrorType::DiscordGateway,
            source: Some(Box::new(err)),
        }
    }
}
