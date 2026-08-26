//! A CARv1 block reader, enough to compare what `getRepo` carries.
//!
//! The served commit embeds fresh random key material, so the two sides' commit
//! blocks never match by construction. The record blocks must, so the gate
//! compares the block sets and expects the difference to be exactly the one
//! commit block on each side.

use anyhow::{bail, Result};
use std::collections::BTreeSet;

fn varint(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let Some(byte) = bytes.get(*at) else {
            bail!("truncated varint");
        };
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            bail!("varint too long");
        }
    }
}

/// Advance past one binary CIDv1 and return its bytes.
fn cid<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a [u8]> {
    let start = *at;
    let version = varint(bytes, at)?;
    if version != 1 {
        bail!("expected CIDv1, got version {version}");
    }
    let _codec = varint(bytes, at)?;
    let _hash = varint(bytes, at)?;
    let length = varint(bytes, at)? as usize;
    *at += length;
    if *at > bytes.len() {
        bail!("truncated cid");
    }
    Ok(&bytes[start..*at])
}

/// The `(cid, data)` pairs a CARv1 payload carries, roots excluded from the
/// return because the header is not what is being compared.
pub fn blocks(car: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut at = 0usize;
    let header = varint(car, &mut at)? as usize;
    at += header;
    if at > car.len() {
        bail!("truncated car header");
    }
    let mut out = Vec::new();
    while at < car.len() {
        let length = varint(car, &mut at)? as usize;
        let end = at + length;
        if end > car.len() {
            bail!("truncated car block");
        }
        let mut cursor = at;
        let key = cid(car, &mut cursor)?.to_vec();
        out.push((key, car[cursor..end].to_vec()));
        at = end;
    }
    Ok(out)
}

pub struct CarDiff {
    pub shared: usize,
    pub only_shim: Vec<String>,
    pub only_pds: Vec<String>,
}

/// Compare two CAR payloads by block CID.
pub fn diff(shim: &[u8], pds: &[u8]) -> Result<CarDiff> {
    let left: BTreeSet<Vec<u8>> = blocks(shim)?.into_iter().map(|(cid, _)| cid).collect();
    let right: BTreeSet<Vec<u8>> = blocks(pds)?.into_iter().map(|(cid, _)| cid).collect();
    let hex = |bytes: &Vec<u8>| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(CarDiff {
        shared: left.intersection(&right).count(),
        only_shim: left.difference(&right).map(hex).collect(),
        only_pds: right.difference(&left).map(hex).collect(),
    })
}
