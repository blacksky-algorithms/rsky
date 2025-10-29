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

pub fn read(data: &[u8]) -> Result<Option<(Header, SubscribeRepos)>> {
    let mut reader = Cursor::new(data);

    let header = ciborium::de::from_reader::<Header, _>(&mut reader)?;
    let body = match header.type_.as_str() {
        "#commit" => SubscribeRepos::Commit(serde_ipld_dagcbor::from_reader(&mut reader)?),
        "#handle" => SubscribeRepos::Handle(serde_ipld_dagcbor::from_reader(&mut reader)?),
        "#tombstone" => SubscribeRepos::Tombstone(serde_ipld_dagcbor::from_reader(&mut reader)?),
        "#account" => SubscribeRepos::Account(serde_ipld_dagcbor::from_reader(&mut reader)?),
        "#identity" => SubscribeRepos::Identity(serde_ipld_dagcbor::from_reader(&mut reader)?),
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
