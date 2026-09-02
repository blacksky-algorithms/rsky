use crate::plc::operations::{update_atproto_key_op, update_handle_op};
use crate::plc::types::{CompatibleOp, OpOrTombstone};
use crate::APP_USER_AGENT;
use anyhow::{bail, Result};
use rsky_common::encode_uri_component;
use secp256k1::SecretKey;
use serde::de::DeserializeOwned;
use types::{CompatibleOpOrTombstone, DocumentData};

pub struct Client {
    pub url: String,
}

impl Client {
    pub fn new(url: String) -> Self {
        Self { url }
    }

    pub fn post_op_url(&self, did: &String) -> String {
        format!("{0}/{1}", self.url, encode_uri_component(did))
    }

    // @TODO: Add better failure mode here
    async fn make_get_req<T: DeserializeOwned>(
        &self,
        url: String,
        params: Option<Vec<(&str, String)>>,
    ) -> Result<T> {
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build()?;
        let mut builder = client
            .get(url)
            .header("Connection", "Keep-Alive")
            .header("Keep-Alive", "timeout=5, max=1000");
        if let Some(params) = params {
            builder = builder.query(&params);
        }
        let res = builder.send().await?;
        Ok(res.json().await?)
    }

    pub async fn send_operation(&self, did: &String, op: &OpOrTombstone) -> Result<()> {
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build()?;
        let response = client
            .post(self.post_op_url(did))
            .json(op)
            .header("Connection", "Keep-Alive")
            .header("Keep-Alive", "timeout=5, max=1000")
            .send()
            .await?;
        let res = &response;
        match res.error_for_status_ref() {
            Ok(_) => Ok(()),
            Err(_) => bail!(response.text().await?),
        }
    }

    pub async fn get_document_data(&self, did: &String) -> Result<DocumentData> {
        match self
            .make_get_req(
                format!("{0}/{1}/data", self.url, encode_uri_component(did)),
                None,
            )
            .await
        {
            Ok(res) => Ok(res),
            Err(error) => bail!(error.to_string()),
        }
    }

    pub async fn get_last_op(&self, did: &String) -> Result<CompatibleOpOrTombstone> {
        match self
            .make_get_req(
                format!("{0}/{1}/log/last", self.url, encode_uri_component(did)),
                None,
            )
            .await
        {
            Ok(res) => Ok(res),
            Err(error) => bail!(error.to_string()),
        }
    }

    pub async fn ensure_last_op(&self, did: &String) -> Result<CompatibleOpOrTombstone> {
        let last_op: CompatibleOpOrTombstone = self.get_last_op(did).await?;
        match last_op {
            CompatibleOpOrTombstone::Tombstone(_) => bail!("Cannot apply op to tombstone"),
            _ => Ok(last_op),
        }
    }

    pub async fn update_handle(
        &self,
        did: &String,
        signer: &SecretKey,
        handle: &str,
    ) -> Result<()> {
        let last_op: CompatibleOp = match self.ensure_last_op(did).await? {
            CompatibleOpOrTombstone::CreateOpV1(last_op) => CompatibleOp::CreateOpV1(last_op),
            CompatibleOpOrTombstone::Operation(last_op) => CompatibleOp::Operation(last_op),
            CompatibleOpOrTombstone::Tombstone(_) => {
                panic!("ensure_last_op() didn't prevent tombstone")
            }
        };
        let op = update_handle_op(last_op, signer, handle.to_owned()).await?;
        self.send_operation(did, &OpOrTombstone::Operation(op))
            .await
    }

    pub async fn update_atproto_key(
        &self,
        did: &String,
        signer: &SecretKey,
        signing_key: &str,
    ) -> Result<()> {
        let last_op: CompatibleOp = match self.ensure_last_op(did).await? {
            CompatibleOpOrTombstone::CreateOpV1(last_op) => CompatibleOp::CreateOpV1(last_op),
            CompatibleOpOrTombstone::Operation(last_op) => CompatibleOp::Operation(last_op),
            CompatibleOpOrTombstone::Tombstone(_) => {
                panic!("ensure_last_op() didn't prevent tombstone")
            }
        };
        let op = update_atproto_key_op(last_op, signer, signing_key.to_owned()).await?;
        self.send_operation(did, &OpOrTombstone::Operation(op))
            .await
    }
}

pub mod operations;
#[cfg(test)]
pub(crate) mod test_support;
pub mod types;

#[cfg(test)]
mod tests {
    use super::test_support::MockPlc;
    use super::*;
    use crate::plc::types::Operation;
    use rsky_crypto::utils::encode_did_key;
    use secp256k1::{Keypair, Secp256k1};
    use std::collections::BTreeMap;

    fn keypair(byte: u8) -> Keypair {
        let secret = secp256k1::SecretKey::from_slice(&[byte; 32]).unwrap();
        Keypair::from_secret_key(&Secp256k1::new(), &secret)
    }

    #[tokio::test]
    async fn update_atproto_key_publishes_the_new_verification_method() {
        let rotation = keypair(0x11);
        let old_key = encode_did_key(&keypair(0x22).public_key());
        let new_key = encode_did_key(&keypair(0x33).public_key());
        let did = "did:plc:alice".to_owned();
        let plc = MockPlc::start(
            &encode_did_key(&rotation.public_key()),
            BTreeMap::from([(did.clone(), old_key.clone())]),
        );
        let client = Client::new(plc.url.clone());

        client
            .update_atproto_key(&did, &rotation.secret_key(), &new_key)
            .await
            .unwrap();

        let posted = plc.posted();
        assert_eq!(posted.len(), 1);
        let op: Operation = serde_json::from_value(posted[0].clone()).unwrap();
        assert_eq!(op.verification_methods.get("atproto"), Some(&new_key));
        assert!(op.prev.is_some());
        assert!(op.sig.is_some());
        assert_eq!(plc.published_key(&did), Some(new_key));
    }

    #[tokio::test]
    async fn update_atproto_key_propagates_a_directory_error() {
        let rotation = keypair(0x11);
        let plc = MockPlc::start(&encode_did_key(&rotation.public_key()), BTreeMap::new());
        let client = Client::new(format!("{}/missing", plc.url));
        let error = client
            .update_atproto_key(
                &"did:plc:gone".to_owned(),
                &rotation.secret_key(),
                "did:key:whatever",
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.is_empty());
    }
}
