use crate::common::encode_uri_component;
use crate::plc::operations::update_handle_op;
use crate::plc::types::{CompatibleOp, OpOrTombstone};
use crate::APP_USER_AGENT;
use anyhow::{bail, Result};
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

    async fn send_operation(&self, did: &String, op: &OpOrTombstone) -> Result<()> {
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
            Ok(_res) => Ok(()),
            Err(error) => bail!(error.to_string()),
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
        handle: &String,
    ) -> Result<()> {
        let last_op: CompatibleOp = match self.ensure_last_op(did).await? {
            CompatibleOpOrTombstone::CreateOpV1(last_op) => CompatibleOp::CreateOpV1(last_op),
            CompatibleOpOrTombstone::Operation(last_op) => CompatibleOp::Operation(last_op),
            CompatibleOpOrTombstone::Tombstone(_) => {
                panic!("ensure_last_op() didn't prevent tombstone")
            }
        };
        let op = update_handle_op(last_op, signer, handle.clone()).await?;
        self.send_operation(&did, &OpOrTombstone::Operation(op))
            .await
    }
}

pub mod operations;
pub mod types;
