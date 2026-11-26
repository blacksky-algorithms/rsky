use anyhow::{bail, Result};
use rsky_lexicon::com::atproto::label::SubscribeLabels;
use rsky_lexicon::com::atproto::sync::SubscribeRepos;
use serde::Deserialize;
use std::io::Cursor;

#[derive(Debug, Deserialize)]
pub struct Header {
    #[serde(rename(deserialize = "t"))]
    pub type_: String,
    #[serde(rename(deserialize = "op"))]
    pub operation: u8,
}

#[derive(Debug)]
pub enum Error {
    Header(ciborium::de::Error<std::io::Error>),
    Body(serde_ipld_dagcbor::DecodeError<std::io::Error>),
}

impl From<ciborium::de::Error<std::io::Error>> for Error {
    fn from(e: ciborium::de::Error<std::io::Error>) -> Self {
        Self::Header(e)
    }
}

impl From<serde_ipld_dagcbor::DecodeError<std::io::Error>> for Error {
    fn from(e: serde_ipld_dagcbor::DecodeError<std::io::Error>) -> Self {
        Self::Body(e)
    }
}

pub const FUTURE_CURSOR: &str = "FutureCursor";

#[derive(Debug, Deserialize, PartialEq)]
pub struct ErrorFrame {
    pub error: String,
    pub message: Option<String>,
}

#[derive(Deserialize)]
struct FrameOperation {
    op: i8,
}

pub fn read_error(data: &[u8]) -> Option<ErrorFrame> {
    let mut reader = Cursor::new(data);
    let header = ciborium::de::from_reader::<FrameOperation, _>(&mut reader).ok()?;
    if header.op != -1 {
        return None;
    }
    serde_ipld_dagcbor::from_reader(&mut reader).ok()
}

pub fn seq(event: &SubscribeRepos) -> i64 {
    match event {
        SubscribeRepos::Commit(commit) => commit.seq,
        SubscribeRepos::Identity(identity) => identity.seq,
        SubscribeRepos::Account(account) => account.seq,
        SubscribeRepos::Handle(handle) => handle.seq,
        SubscribeRepos::Tombstone(tombstone) => tombstone.seq,
    }
}

pub fn read(data: &[u8]) -> Result<Option<(Header, SubscribeRepos)>> {
    let mut reader = Cursor::new(data);

    let header = ciborium::de::from_reader::<Header, _>(&mut reader)?;
    let body = match header.type_.as_str() {
        "#commit" => match serde_ipld_dagcbor::from_reader(&mut reader) {
            Ok(commit) => SubscribeRepos::Commit(commit),
            Err(e) => {
                eprintln!("Failed to decode #commit message: {:?}", e);
                return Ok(None);
            }
        },
        "#handle" => match serde_ipld_dagcbor::from_reader(&mut reader) {
            Ok(handle) => SubscribeRepos::Handle(handle),
            Err(e) => {
                eprintln!("Failed to decode #handle message: {:?}", e);
                return Ok(None);
            }
        },
        "#tombstone" => match serde_ipld_dagcbor::from_reader(&mut reader) {
            Ok(tombstone) => SubscribeRepos::Tombstone(tombstone),
            Err(e) => {
                eprintln!("Failed to decode #tombstone message: {:?}", e);
                return Ok(None);
            }
        },
        "#account" => match serde_ipld_dagcbor::from_reader(&mut reader) {
            Ok(account) => SubscribeRepos::Account(account),
            Err(e) => {
                eprintln!("Failed to decode #account message: {:?}", e);
                return Ok(None);
            }
        },
        "#identity" => match serde_ipld_dagcbor::from_reader(&mut reader) {
            Ok(identity) => SubscribeRepos::Identity(identity),
            Err(e) => {
                eprintln!("Failed to decode #identity message: {:?}", e);
                return Ok(None);
            }
        },
        "#sync" => {
            // Sync messages declare current repo state (AT Protocol Sync v1.1)
            // For now we ignore these - a full implementation would check if
            // resynchronization is needed and fetch via com.atproto.sync.getRepo
            // See: https://github.com/bluesky-social/atproto/blob/main/specs/0006-sync.md
            return Ok(None);
        }
        "#info" => {
            // Info messages are informational and can be safely ignored
            return Ok(None);
        }
        _ => {
            eprintln!("Received unknown header {:?}", header.type_.as_str());
            // Skip unknown message types instead of failing
            return Ok(None);
        }
    };

    Ok(Some((header, body)))
}

pub fn read_labels(data: &[u8]) -> Result<(Header, SubscribeLabels)> {
    let mut reader = Cursor::new(data);

    let header = ciborium::de::from_reader::<Header, _>(&mut reader)?;
    let body = match header.type_.as_str() {
        "#labels" => serde_ipld_dagcbor::from_reader(&mut reader)?,
        _ => {
            eprintln!("Received unknown header {:?}", header.type_.as_str());
            bail!(format!(
                "Received unknown header {:?}",
                header.type_.as_str()
            ))
        }
    };

    Ok((header, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_lexicon::com::atproto::sync::SubscribeReposCommit;
    use serde::Serialize;

    #[derive(Serialize)]
    struct OperationHeader {
        op: i8,
    }

    #[derive(Serialize)]
    struct EventHeader {
        op: i8,
        t: &'static str,
    }

    #[derive(Serialize)]
    struct ErrorBody {
        error: &'static str,
        message: Option<&'static str>,
    }

    #[derive(Serialize)]
    struct SeqBody {
        seq: i64,
        did: &'static str,
        time: &'static str,
        handle: &'static str,
        active: bool,
    }

    fn frame<H: Serialize, B: Serialize>(header: &H, body: &B) -> Vec<u8> {
        let mut bytes = serde_ipld_dagcbor::to_vec(header).unwrap();
        bytes.extend(serde_ipld_dagcbor::to_vec(body).unwrap());
        bytes
    }

    fn event(kind: &'static str, seq: i64) -> SubscribeRepos {
        let body = SeqBody {
            seq,
            did: "did:plc:w4xbfzo7kqfes5zb7r6qv3rw",
            time: "2026-01-12T19:45:23.307Z",
            handle: "rude1.blacksky.team",
            active: true,
        };
        let (_header, event) = read(&frame(&EventHeader { op: 1, t: kind }, &body))
            .unwrap()
            .unwrap();
        event
    }

    #[test]
    fn read_error_decodes_error_frames() {
        let body = ErrorBody {
            error: FUTURE_CURSOR,
            message: Some("Cursor in the future."),
        };
        assert_eq!(
            read_error(&frame(&OperationHeader { op: -1 }, &body)),
            Some(ErrorFrame {
                error: FUTURE_CURSOR.to_string(),
                message: Some("Cursor in the future.".to_string()),
            })
        );
    }

    #[test]
    fn read_error_ignores_everything_else() {
        let body = ErrorBody {
            error: FUTURE_CURSOR,
            message: None,
        };
        assert_eq!(read_error(&frame(&OperationHeader { op: 1 }, &body)), None);
        assert_eq!(read_error(&frame(&OperationHeader { op: -1 }, &7)), None);
        assert_eq!(read_error(&[0xff]), None);
    }

    #[test]
    fn seq_reads_every_event_kind() {
        let commit = SubscribeReposCommit {
            seq: 10,
            time: Default::default(),
            rebase: false,
            too_big: false,
            repo: "did:plc:w4xbfzo7kqfes5zb7r6qv3rw".to_string(),
            commit: Default::default(),
            prev: None,
            rev: "3n35crvprx22s".to_string(),
            since: None,
            blocks: Vec::new(),
            ops: Vec::new(),
            blobs: Vec::new(),
            prev_data: None,
        };
        assert_eq!(seq(&SubscribeRepos::Commit(commit)), 10);
        assert_eq!(seq(&event("#identity", 11)), 11);
        assert_eq!(seq(&event("#account", 12)), 12);
        assert_eq!(seq(&event("#handle", 13)), 13);
        assert_eq!(seq(&event("#tombstone", 14)), 14);
    }
}
