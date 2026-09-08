use super::CommitObject;
use crate::DaslCid;
use crate::firehose::FirehoseCommit;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Op {
    pub path: Arc<str>,
    pub kind: OpKind,
}

#[derive(Debug, Clone)]
pub enum OpKind {
    Create { cid: DaslCid },
    Update { cid: DaslCid, prev: DaslCid },
    Delete { prev: DaslCid },
}

impl Op {
    pub fn collection(&self) -> &str {
        let Some((c, _)) = self.path.split_once('/') else {
            info!("invalid record path (missing /) encountered");
            return &self.path;
        };
        c
    }
    pub fn rkey(&self) -> &str {
        let Some((_, r)) = self.path.split_once('/') else {
            info!("invalid record path (missing /) encountered");
            return "";
        };
        r
    }
    /// the the CID for this op
    ///
    /// NOTE: for a Delete, this will return the *prev* cid!
    pub fn cid(&self) -> DaslCid {
        match self.kind {
            OpKind::Create { cid } => cid,
            OpKind::Update { cid, .. } => cid,
            OpKind::Delete { prev } => prev,
        }
    }
    /// the CID for the `prev` of this op, if not Create
    ///
    /// returns None for create
    ///
    /// NOTE: the option-wrapping vs .cid() which (maybe surprisingly) returns
    /// `prev` for delete.
    pub fn prev(&self) -> Option<DaslCid> {
        match self.kind {
            OpKind::Create { .. } => None,
            OpKind::Update { prev, .. } => Some(prev),
            OpKind::Delete { prev } => Some(prev),
        }
    }
}

pub struct Commit<'a> {
    pub object: &'a CommitObject,
    pub ops: &'a [Op],
    blocks: &'a HashMap<DaslCid, Bytes>,
}

/// a convenient view into a firehose commit event
impl<'a> Commit<'a> {
    pub(crate) fn new(c: &'a FirehoseCommit) -> Self {
        Self {
            object: &c.commit,
            ops: &c.ops,
            blocks: &c.blocks,
        }
    }
    pub fn block(&self, cid: DaslCid) -> Option<&'a [u8]> {
        self.blocks.get(&cid).map(Bytes::as_ref)
    }
    pub fn blocks(&self) -> &'a HashMap<DaslCid, Bytes> {
        self.blocks
    }
    pub fn added(&self) -> impl Iterator<Item = &'a Op> {
        self.ops
            .iter()
            .filter(|o| matches!(o.kind, OpKind::Create { .. }))
    }
    pub fn updated(&self) -> impl Iterator<Item = &'a Op> {
        self.ops
            .iter()
            .filter(|o| matches!(o.kind, OpKind::Update { .. }))
    }
    pub fn deleted(&self) -> impl Iterator<Item = &'a Op> {
        self.ops
            .iter()
            .filter(|o| matches!(o.kind, OpKind::Delete { .. }))
    }
}
