use rand::{Rng, distr::Uniform};
use serde_json::json;
use std::{error::Error as StdError, sync::Arc, time::Duration};
use tokio::{
    sync::mpsc::{self, error::SendError},
    task::JoinHandle,
    time::sleep,
};

use super::Notifier;
use crate::error::{Error, ErrorType};

pub async fn handle(
    notify: &Arc<Notifier>,
    heartbeat_interval: u64,
    egress_tx: &mpsc::UnboundedSender<String>,
    mut nonce_rx: mpsc::UnboundedReceiver<u64>,
) -> Result<JoinHandle<Result<(), Error<dyn ErrorInner>>>, Error<dyn ErrorInner>> {
    const JS_MAX_INT: u64 = (1u64 << 53) - 1;
    let nonce_range = Uniform::try_from(0..JS_MAX_INT).unwrap();
    let mut is_first = true;

    let notifier = notify.clone();
    let egress_tx = egress_tx.clone();
    Ok(tokio::spawn(async move {
        let notify = notifier.heartbeat.notified();
        let mut notify = Box::pin(notify);

        loop {
            let multiplier: f64 = if is_first {
                rand::rng().random_range(0.0..1.0)
            } else {
                1.0
            };
            sleep(Duration::from_millis(
                heartbeat_interval * multiplier as u64,
            ))
            .await;

            let nonce = rand::rng().sample(nonce_range);
            let payload = json!({
                "op": 3,
                "d": {
                    "t": nonce,
                    "seq_ack": 1
                }
            });
            egress_tx.send(payload.to_string())?;

            let item;
            tokio::select! {
                res = nonce_rx.recv() => item = res,
                _ = (&mut notify) => break,
            }

            let Some(received_nonce) = item else {
                return Ok(());
            };
            if nonce != received_nonce {
                return Ok(());
            }

            is_first = false;
        }

        notifier.close();
        Ok(())
    }))
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
