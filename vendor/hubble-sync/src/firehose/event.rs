//! what a `FirehoseSubscriber` builds and sends to repo actors

use std::sync::Arc;
use std::time::SystemTime;

use jacquard_api::com_atproto::sync::subscribe_repos::{Account, Identity};
use tokio::sync::mpsc;

use super::{FirehoseCommit, FirehoseSync, Seq};
use crate::Host;

/// A DID-bound event from the firehose
///
/// carries the source with it, so multi-relay might actually work later.
///
/// `info` events are handled by the subscriber directly.
#[derive(Debug)]
pub struct FirehoseEvent {
    pub host: Arc<Host>,
    pub seq: Seq,
    pub time: SystemTime,
    pub upstream_time: SystemTime,
    pub payload: FirehosePayload,
    pub ack: FirehoseAck,
}

#[derive(Debug)]
pub enum FirehosePayload {
    Commit(Box<FirehoseCommit>),
    Sync(Box<FirehoseSync>),
    Account(Box<Account>),
    Identity(Box<Identity>),
}

/// auto-ack on drop for firehose events
///
/// this lets the persisted cursor update to the next processed seq
#[derive(Debug)]
pub struct FirehoseAck {
    seq: Seq,
    sender: Option<mpsc::UnboundedSender<Seq>>,
}

impl FirehoseAck {
    pub fn new(seq: Seq, sender: mpsc::UnboundedSender<Seq>) -> Self {
        Self {
            seq,
            sender: Some(sender),
        }
    }

    /// manually ack an event
    ///
    /// usually not needed since it auto-acks on drop
    pub fn ack(mut self) {
        self.do_ack();
    }

    fn do_ack(&mut self) {
        if let Some(s) = self.sender.take() {
            // unbounded send is non-blocking, and receiver gone is fine
            let _ = s.send(self.seq);
        }
    }
}

impl Drop for FirehoseAck {
    fn drop(&mut self) {
        self.do_ack()
    }
}
