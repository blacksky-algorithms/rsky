//! Runtime discovery of the spaces a daemon should sync.

use async_trait::async_trait;
use rsky_space::space_id::SpaceId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use crate::error::{DaemonError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceTarget {
    pub generation: i64,
    pub state: String,
}

#[async_trait]
pub trait SpaceSource: Send + Sync {
    async fn spaces(&self) -> Result<BTreeMap<String, SpaceTarget>>;
}

pub struct StaticSpaces(pub BTreeSet<String>);

impl StaticSpaces {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(spaces: I) -> Self {
        Self(spaces.into_iter().map(Into::into).collect())
    }
}

#[async_trait]
impl SpaceSource for StaticSpaces {
    async fn spaces(&self) -> Result<BTreeMap<String, SpaceTarget>> {
        Ok(self
            .0
            .iter()
            .cloned()
            .map(|space| {
                (
                    space,
                    SpaceTarget {
                        generation: 1,
                        state: "active".into(),
                    },
                )
            })
            .collect())
    }
}

pub struct HttpSpaceSource {
    url: String,
    api_key: String,
    authority_did: Option<String>,
    space_type: String,
    http: reqwest::Client,
}

impl HttpSpaceSource {
    pub fn new(
        url: impl Into<String>,
        api_key: impl Into<String>,
        authority_did: Option<String>,
        space_type: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into().trim_end_matches('/').into(),
            api_key: api_key.into(),
            authority_did,
            space_type: space_type.into(),
            http: reqwest::Client::new(),
        }
    }
}

#[derive(serde::Deserialize)]
struct SpacesResponse {
    spaces: Vec<SyncableSpace>,
}
#[derive(serde::Deserialize)]
struct SyncableSpace {
    space: String,
    generation: i64,
    state: String,
}

#[async_trait]
impl SpaceSource for HttpSpaceSource {
    async fn spaces(&self) -> Result<BTreeMap<String, SpaceTarget>> {
        let mut request = self
            .http
            .get(format!("{}/admin/sync-spaces", self.url))
            .header("X-RSKY-KEY", &self.api_key);
        if let Some(authority) = &self.authority_did {
            request = request.query(&[("authority", authority)]);
        }
        let response = request
            .send()
            .await
            .map_err(|e| DaemonError::Xrpc(e.to_string()))?;
        if !response.status().is_success() {
            return Err(DaemonError::Xrpc(format!(
                "space list returned {}",
                response.status()
            )));
        }
        let body: SpacesResponse = response
            .json()
            .await
            .map_err(|e| DaemonError::Xrpc(e.to_string()))?;
        Ok(body
            .spaces
            .into_iter()
            .filter_map(|entry| {
                let space = SpaceId::parse(&entry.space).ok()?;
                (entry.generation > 0
                    && matches!(
                        entry.state.as_str(),
                        "host_registered" | "active" | "deleting"
                    )
                    && self
                        .authority_did
                        .as_deref()
                        .is_none_or(|authority| space.authority == authority)
                    && space.space_type == self.space_type)
                    .then_some((
                        space.uri(),
                        SpaceTarget {
                            generation: entry.generation,
                            state: entry.state,
                        },
                    ))
            })
            .collect())
    }
}

pub struct CombinedSource(pub Vec<Box<dyn SpaceSource>>);

#[async_trait]
impl SpaceSource for CombinedSource {
    async fn spaces(&self) -> Result<BTreeMap<String, SpaceTarget>> {
        let mut all = BTreeMap::new();
        let mut last_error = None;
        for source in &self.0 {
            match source.spaces().await {
                Ok(spaces) => {
                    for (space, target) in spaces {
                        all.entry(space)
                            .and_modify(|current: &mut SpaceTarget| {
                                if target.generation >= current.generation {
                                    *current = target.clone();
                                }
                            })
                            .or_insert(target);
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "a space source failed");
                    last_error = Some(error);
                }
            }
        }
        match last_error {
            Some(error) if all.is_empty() => Err(error),
            _ => Ok(all),
        }
    }
}

#[derive(Clone, Default)]
pub struct SpaceRegistry(Arc<RwLock<BTreeSet<String>>>);
impl SpaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn contains(&self, space: &str) -> bool {
        self.0.read().expect("space registry").contains(space)
    }
    pub fn snapshot(&self) -> BTreeSet<String> {
        self.0.read().expect("space registry").clone()
    }
    pub fn replace(&self, spaces: BTreeSet<String>) {
        *self.0.write().expect("space registry") = spaces;
    }
    pub fn insert(&self, space: impl Into<String>) {
        self.0.write().expect("space registry").insert(space.into());
    }
    pub fn remove(&self, space: &str) {
        self.0.write().expect("space registry").remove(space);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    const A: &str = "at://did:plc:c/space/community.blacksky.feed/a";
    const B: &str = "at://did:plc:c/space/community.blacksky.feed/b";
    fn source(url: String) -> HttpSpaceSource {
        HttpSpaceSource::new(
            url,
            "key",
            Some("did:plc:c".to_string()),
            "community.blacksky.feed",
        )
    }
    #[tokio::test]
    async fn filters_and_reads_managing_app_spaces() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/admin/sync-spaces")).and(query_param("authority", "did:plc:c")).and(header("X-RSKY-KEY", "key")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"spaces":[{"space":A,"generation":2,"state":"active"},{"space":B,"generation":3,"state":"deleting"},{"space":"at://did:plc:other/space/community.blacksky.feed/x","generation":1,"state":"active"}]}))).mount(&server).await;
        assert_eq!(source(server.uri()).spaces().await.unwrap().len(), 2);
    }
    #[tokio::test]
    async fn without_an_authority_filter_all_authorities_are_discovered() {
        use wiremock::matchers::query_param_is_missing;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/admin/sync-spaces"))
            .and(query_param_is_missing("authority"))
            .and(header("X-RSKY-KEY", "key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "spaces": [
                    {"space": A, "generation": 2, "state": "active"},
                    {"space": "at://did:plc:other/space/community.blacksky.feed/x", "generation": 1, "state": "active"},
                    {"space": "at://did:plc:other/space/wrong.type/y", "generation": 1, "state": "active"},
                    {"space": "at://did:plc:third/space/community.blacksky.feed/z", "generation": 1, "state": "retired"},
                ]
            })))
            .mount(&server)
            .await;
        let spaces = HttpSpaceSource::new(server.uri(), "key", None, "community.blacksky.feed")
            .spaces()
            .await
            .unwrap();
        assert_eq!(
            spaces.keys().collect::<Vec<_>>(),
            vec![A, "at://did:plc:other/space/community.blacksky.feed/x"]
        );
    }

    #[tokio::test]
    async fn pin_survives_source_outage() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let spaces = CombinedSource(vec![
            Box::new(StaticSpaces::new([A])),
            Box::new(source(server.uri())),
        ])
        .spaces()
        .await
        .unwrap();
        assert!(spaces.contains_key(A));
    }
    #[tokio::test]
    async fn every_source_failure_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(CombinedSource(vec![Box::new(source(server.uri()))])
            .spaces()
            .await
            .is_err());
    }
    #[test]
    fn registry_tracks_spaces() {
        let registry = SpaceRegistry::new();
        registry.insert(A);
        assert!(registry.contains(A));
        registry.replace(BTreeSet::from([B.into()]));
        assert!(!registry.contains(A) && registry.contains(B));
        registry.remove(B);
        assert!(registry.snapshot().is_empty());
    }
}
