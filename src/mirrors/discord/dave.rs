use davey::{
    DaveSession, ProposalsOperationType,
    errors::{CreateKeyPackageError, InitError, ReinitError},
};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    error::Error as StdError,
    num::NonZeroU16,
    sync::Arc,
};
use tokio::{
    sync::{
        RwLock,
        mpsc::{self, error::SendError},
        oneshot,
    },
    task::JoinHandle,
};
use tokio_websockets::Message as WebSocketMessage;
use tracing::warn;

use super::{DAVEInstance, DAVEPayload, Notifier};
use crate::error::{Error, ErrorType};

pub const DAVE_TRANSITION_READY: u8 = 23;
pub const MLS_EXTERNAL_SENDER: u8 = 25;
pub const MLS_KEY_PACKAGE: u8 = 26;
pub const MLS_PROPOSALS: u8 = 27;
pub const MLS_COMMIT_WELCOME: u8 = 28;
pub const MLS_ANNOUNCE_COMMIT_TRANSITION: u8 = 29;
pub const MLS_WELCOME: u8 = 30;
pub const MLS_INVALID_COMMIT_WELCOME: u8 = 31;

pub async fn handle(
    notify: &Arc<Notifier>,
    egress_tx: &mpsc::UnboundedSender<WebSocketMessage>,
    mut dave_rx: mpsc::UnboundedReceiver<DAVEPayload>,
    mut instance_tx: Option<oneshot::Sender<Arc<RwLock<DAVEInstance>>>>,
) -> Result<JoinHandle<Result<(), Error<dyn ErrorInner>>>, Error<dyn ErrorInner>> {
    let notifier = notify.clone();
    let egress_tx = egress_tx.clone();
    Ok(tokio::spawn(async move {
        let notify = notifier.dave.notified();
        let mut notify = Box::pin(notify);

        let mut dave_protocol_version = 0;
        let mut user_id = 0;
        let mut channel_id = 0;
        let mut clients_connected = HashSet::new();
        let mut pending_transitions = HashMap::new();
        let mut is_downgraded = false;
        let mut dave_instance: Option<Arc<RwLock<DAVEInstance>>> = None;
        loop {
            let item;
            tokio::select! {
                res = dave_rx.recv() => item = res,
                _ = (&mut notify) => break,
            }

            let Some(item) = item else {
                break;
            };
            match (item, &dave_instance) {
                (DAVEPayload::Binary(payload), Some(dave_instance)) => {
                    if payload.len() < 3 {
                        continue;
                    }
                    match payload[2] {
                        MLS_EXTERNAL_SENDER => {
                            let data = &payload[3..];
                            let Ok(_) = dave_instance
                                .write()
                                .await
                                .get_session()
                                .set_external_sender(data)
                            else {
                                break;
                            };
                        }
                        MLS_PROPOSALS => {
                            let optype = match payload[3] {
                                0 => ProposalsOperationType::APPEND,
                                1 => ProposalsOperationType::REVOKE,
                                _ => continue,
                            };
                            let data = &payload[4..];
                            let clients_connected: Vec<u64> =
                                clients_connected.clone().into_iter().collect();
                            let Ok(commit_welcome) =
                                dave_instance.write().await.get_session().process_proposals(
                                    optype,
                                    data,
                                    Some(clients_connected.as_slice()),
                                )
                            else {
                                break;
                            };
                            let Some(commit_welcome) = commit_welcome else {
                                continue;
                            };
                            let mut commit = commit_welcome.commit;
                            let welcome = commit_welcome.welcome;
                            commit.insert(0, MLS_COMMIT_WELCOME);
                            if let Some(mut welcome) = welcome {
                                commit.append(&mut welcome);
                            }
                            let payload = WebSocketMessage::binary(commit);
                            egress_tx.send(payload)?;
                        }
                        MLS_ANNOUNCE_COMMIT_TRANSITION => {
                            let transition_id = (payload[3] as u16 * 256) + payload[4] as u16;
                            let data = &payload[5..];
                            let mut instance = dave_instance.write().await;
                            let Ok(_) = instance.get_session().process_commit(data) else {
                                let Ok(_) = recover_from_invalid_commit(
                                    &egress_tx,
                                    &mut instance,
                                    dave_protocol_version,
                                    transition_id,
                                    user_id,
                                    channel_id,
                                ) else {
                                    break;
                                };
                                continue;
                            };
                            if transition_id != 0 {
                                pending_transitions.insert(transition_id, dave_protocol_version);
                                let payload = json!({
                                    "op": DAVE_TRANSITION_READY,
                                    "d": {
                                        "transition_id": transition_id
                                    }
                                });
                                egress_tx.send(WebSocketMessage::text(payload.to_string()))?;
                            }
                        }
                        MLS_WELCOME => {
                            let transition_id = (payload[3] as u16 * 256) + payload[4] as u16;
                            let data = &payload[5..];
                            let mut instance = dave_instance.write().await;
                            let Ok(_) = instance.get_session().process_welcome(data) else {
                                let Ok(_) = recover_from_invalid_commit(
                                    &egress_tx,
                                    &mut instance,
                                    dave_protocol_version,
                                    transition_id,
                                    user_id,
                                    channel_id,
                                ) else {
                                    break;
                                };
                                continue;
                            };
                            if transition_id != 0 {
                                pending_transitions.insert(transition_id, dave_protocol_version);
                                let payload = json!({
                                    "op": DAVE_TRANSITION_READY,
                                    "d": {
                                        "transition_id": transition_id
                                    }
                                });
                                egress_tx.send(WebSocketMessage::text(payload.to_string()))?;
                            }
                        }
                        _ => {}
                    }
                }
                (
                    DAVEPayload::OpCode4(
                        version,
                        user,
                        channel,
                        local_audio_track,
                        local_video_track,
                    ),
                    None,
                ) => {
                    dave_protocol_version = version;
                    user_id = user;
                    channel_id = channel;
                    let Ok(session) = reinit_dave_session(
                        &egress_tx,
                        None,
                        dave_protocol_version,
                        user_id,
                        channel_id,
                    ) else {
                        break;
                    };
                    let Some(session) = session else {
                        continue;
                    };
                    let inst = Arc::new(RwLock::new(DAVEInstance {
                        session,
                        dave_protocol_version,
                        local_audio_track,
                        local_video_track,
                    }));
                    if let Some(instance_tx) = instance_tx.take() {
                        let _ = instance_tx.send(inst.clone());
                    }
                    dave_instance = Some(inst);
                }
                (DAVEPayload::OpCode11(user_ids), _) => {
                    for id in user_ids {
                        let Ok(id): Result<u64, _> = id.parse() else {
                            continue;
                        };
                        clients_connected.insert(id);
                    }
                }
                (DAVEPayload::OpCode13(user_id), _) => {
                    let Ok(id): Result<u64, _> = user_id.parse() else {
                        continue;
                    };
                    clients_connected.remove(&id);
                }
                (DAVEPayload::OpCode21(transition_id, protocol_version), Some(dave_instance)) => {
                    pending_transitions.insert(transition_id, protocol_version);

                    if transition_id == 0 {
                        execute_pending_transition(
                            &mut dave_protocol_version,
                            &mut pending_transitions,
                            &mut is_downgraded,
                            dave_instance,
                            transition_id,
                        )
                        .await;
                    } else {
                        if protocol_version == 0 {
                            dave_instance
                                .write()
                                .await
                                .get_session()
                                .set_passthrough_mode(true, Some(30));
                        }
                        let payload = json!({
                            "op": DAVE_TRANSITION_READY,
                            "d": {
                                "transition_id": transition_id
                            }
                        });
                        egress_tx.send(WebSocketMessage::text(payload.to_string()))?;
                    }
                }
                (DAVEPayload::OpCode22(transition_id), Some(dave_instance)) => {
                    execute_pending_transition(
                        &mut dave_protocol_version,
                        &mut pending_transitions,
                        &mut is_downgraded,
                        dave_instance,
                        transition_id,
                    )
                    .await;
                }
                (DAVEPayload::OpCode24(protocol_version, epoch), Some(dave_instance)) => {
                    if epoch == 1 {
                        let mut instance = dave_instance.write().await;
                        dave_protocol_version =
                            instance.set_dave_protocol_version(protocol_version);
                        let Ok(_) = reinit_dave_session(
                            &egress_tx,
                            Some(&mut instance),
                            dave_protocol_version,
                            user_id,
                            channel_id,
                        ) else {
                            break;
                        };
                    }
                }
                _ => {}
            }
        }
        warn!("[WS] dave closed");

        notifier.close();
        Ok(())
    }))
}

fn recover_from_invalid_commit(
    egress_tx: &mpsc::UnboundedSender<WebSocketMessage>,
    dave_instance: &mut DAVEInstance,
    dave_protocol_version: u16,
    transition_id: u16,
    user_id: u64,
    channel_id: u64,
) -> Result<(), Error<dyn ErrorInner>> {
    let payload = json!({
        "op": MLS_INVALID_COMMIT_WELCOME,
        "d": {
            "transition_id": transition_id
        }
    });
    egress_tx.send(WebSocketMessage::text(payload.to_string()))?;
    let _ = reinit_dave_session(
        egress_tx,
        Some(dave_instance),
        dave_protocol_version,
        user_id,
        channel_id,
    )?;
    Ok(())
}

fn reinit_dave_session(
    egress_tx: &mpsc::UnboundedSender<WebSocketMessage>,
    dave_instance: Option<&mut DAVEInstance>,
    dave_protocol_version: u16,
    user_id: u64,
    channel_id: u64,
) -> Result<Option<DaveSession>, Error<dyn ErrorInner>> {
    let mut artifact = None;

    if dave_protocol_version > 0 {
        let session = match dave_instance {
            Some(dave_instance) => {
                let session = dave_instance.get_session();
                let Some(version) = NonZeroU16::new(dave_protocol_version) else {
                    return Err(Error {
                        kind: ErrorType::DiscordIPC,
                        source: None,
                    });
                };
                session.reinit(version, user_id, channel_id, None)?;
                session
            }
            _ => {
                let Some(version) = NonZeroU16::new(dave_protocol_version) else {
                    return Err(Error {
                        kind: ErrorType::DiscordIPC,
                        source: None,
                    });
                };
                artifact = DaveSession::new(version, user_id, channel_id, None).map(Some)?;
                let Some(session) = artifact.as_mut() else {
                    return Ok(artifact);
                };
                session
            }
        };
        let mut key = session.create_key_package()?;
        key.insert(0, MLS_KEY_PACKAGE);
        let payload = WebSocketMessage::binary(key);
        egress_tx.send(payload)?;
    } else {
        let session = match dave_instance {
            Some(dave_instance) => dave_instance.get_session(),
            _ => return Ok(artifact),
        };
        let _ = session.reset();
        session.set_passthrough_mode(true, Some(10));
    }

    Ok(artifact)
}

async fn execute_pending_transition(
    dave_protocol_version: &mut u16,
    pending_transitions: &mut HashMap<u16, u16>,
    is_downgraded: &mut bool,
    dave_instance: &Arc<RwLock<DAVEInstance>>,
    transition_id: u16,
) {
    let old_version = *dave_protocol_version;
    let Some(new_version) = pending_transitions.remove(&transition_id) else {
        warn!(
            "[DAVE] received execute transition, but we don't have a pending transition for {transition_id}"
        );
        return;
    };
    let mut instance = dave_instance.write().await;
    *dave_protocol_version = instance.set_dave_protocol_version(new_version);

    if old_version != *dave_protocol_version && *dave_protocol_version == 0 {
        *is_downgraded = true;
    } else if transition_id > 0 && *is_downgraded {
        *is_downgraded = false;
        instance.get_session().set_passthrough_mode(true, Some(10));
    }
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

impl From<InitError> for Error<dyn ErrorInner> {
    fn from(err: InitError) -> Self {
        Self {
            kind: ErrorType::DiscordDAVE,
            source: Some(Box::new(err)),
        }
    }
}

impl From<ReinitError> for Error<dyn ErrorInner> {
    fn from(err: ReinitError) -> Self {
        Self {
            kind: ErrorType::DiscordDAVE,
            source: Some(Box::new(err)),
        }
    }
}

impl From<CreateKeyPackageError> for Error<dyn ErrorInner> {
    fn from(err: CreateKeyPackageError) -> Self {
        Self {
            kind: ErrorType::DiscordDAVE,
            source: Some(Box::new(err)),
        }
    }
}
