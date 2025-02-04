use crate::car::read_car_with_root;
use crate::repo::mst::MST;
use crate::repo::types::{Commit, RecordCidClaim};
use crate::repo::util;
use crate::storage::memory_blockstore::MemoryBlockstore;
use crate::storage::readable_blockstore::ReadableBlockstore;
use anyhow::Result;
use serde_cbor::Value as CborValue;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct VerifyProofsOutput {
    pub verified: Vec<RecordCidClaim>,
    pub unverified: Vec<RecordCidClaim>,
}

#[derive(Error, Debug)]
enum ErrorKind {
    #[error("RepoVerificationError: {0}")]
    RepoVerificationError(String),
}

pub async fn verify_proofs(
    proofs: Vec<u8>,
    claims: Vec<RecordCidClaim>,
    did: &str,
    did_key: &String,
) -> Result<VerifyProofsOutput> {
    let car = read_car_with_root(proofs).await?;
    let mut blockstore = MemoryBlockstore::new(Some(car.blocks))?;
    println!("@DEBUG: verify_proofs reading car.root from Blockstore...");
    let data: CborValue = blockstore.read_obj(
        &car.root,
        Box::new(
            |obj: &CborValue| match serde_cbor::value::from_value::<Commit>(obj.clone()) {
                Ok(_) => true,
                Err(_) => false,
            },
        ),
    )?;
    let commit: Commit = serde_cbor::value::from_value(data)?;
    if commit.did != did {
        return Err(
            ErrorKind::RepoVerificationError(format!("Invalid repo did: {}", commit.did)).into(),
        );
    }
    match util::verify_commit_sig(commit.clone(), did_key)? {
        false => {
            return Err(ErrorKind::RepoVerificationError(format!(
                "Invalid signature on commit: {}",
                car.root.to_string()
            ))
            .into());
        }
        true => {
            let mut mst = MST::load(Arc::new(RwLock::new(blockstore)), commit.data, None)?;
            let mut verified: Vec<RecordCidClaim> = Default::default();
            let mut unverified: Vec<RecordCidClaim> = Default::default();
            for claim in claims {
                let found = mst
                    .get(&util::format_data_key(
                        claim.collection.clone(),
                        claim.rkey.clone(),
                    ))
                    .await?;
                let record = match found {
                    Some(found) => {
                        let mut storage_guard = mst.storage.write().await;
                        println!("@DEBUG: verify_proofs reading found record from Blockstore...");
                        Some(
                            storage_guard.read_obj(
                                &found,
                                Box::new(|obj| matches!(obj, CborValue::Map(_))),
                            )?,
                        )
                    }
                    None => None,
                };
                match &claim.cid {
                    None => match record {
                        None => verified.push(claim),
                        Some(_) => unverified.push(claim),
                    },
                    Some(_) => match claim.cid == found {
                        true => verified.push(claim),
                        false => unverified.push(claim),
                    },
                }
            }
            Ok(VerifyProofsOutput {
                verified,
                unverified,
            })
        }
    }
}
