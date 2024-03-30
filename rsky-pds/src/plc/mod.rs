use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use types::DocumentData;
use crate::common::encode_uri_component;

pub struct Client {
    pub url: String
}

impl Client {
    pub fn new(url: String) -> Self {
        Self {
            url
        }
    }

    async fn make_get_req<T: DeserializeOwned>(
        &self,
        url: String,
        params: Option<Vec<(&str, String)>>
    ) -> Result<T> {
        let client = reqwest::Client::new();
        let mut builder = client
            .get(url)
            .header("Connection", "Keep-Alive")
            .header("Keep-Alive", "timeout=5, max=1000");
        if let Some(params) = params {
            builder = builder.query(&params);
        }
        let res = builder
            .send()
            .await?;
        Ok(res.json().await?)
    }

    pub async fn get_document_data(&self, did: &String) -> Result<DocumentData> {
        match self.make_get_req(format!(
            "{0}/{1}/data",
            self.url,
            encode_uri_component(did)
        ), None).await {
            Ok(res) => Ok(res),
            Err(error) => bail!(error.to_string())
        }
    }
}

pub mod types;