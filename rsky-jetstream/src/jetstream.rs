use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use rsky_lexicon::app::bsky::feed::like::Like;
use rsky_lexicon::app::bsky::feed::{Post, Repost};
use rsky_lexicon::app::bsky::graph::follow::Follow;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Header {
    #[serde(rename(deserialize = "t"))]
    pub type_: String,
    #[serde(rename(deserialize = "op"))]
    pub operation: u8,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoCommitMessage {
    pub did: String,
    pub time_us: i64,
    pub kind: String,
    pub commit: JetstreamRepoCommit,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoAccountMessage {
    pub did: String,
    pub time_us: i64,
    pub kind: String,
    pub account: JetstreamRepoAccount,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoIdentityMessage {
    pub did: String,
    pub time_us: i64,
    pub kind: String,
    pub identity: JetstreamRepoIdentity,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoCommit {
    pub rev: String,
    pub operation: String,
    pub collection: String,
    pub rkey: String,
    #[serde(rename = "record", skip_serializing_if = "Option::is_none")]
    pub record: Option<Lexicon>,
    #[serde(rename = "cid", skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
}

#[derive(Debug, serde_derive::Deserialize, serde_derive::Serialize, PartialEq)]
#[serde(tag = "$type")]
pub enum Lexicon {
    #[serde(rename(deserialize = "app.bsky.feed.post"))]
    AppBskyFeedPost(Post),
    #[serde(rename(deserialize = "app.bsky.feed.repost"))]
    AppBskyFeedRepost(Repost),
    #[serde(rename(deserialize = "app.bsky.feed.like"))]
    AppBskyFeedLike(Like),
    #[serde(rename(deserialize = "app.bsky.graph.follow"))]
    AppBskyFeedFollow(Follow),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LikeSubject {
    pub cid: String,
    pub uri: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoIdentity {
    pub did: String,
    pub handle: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JetstreamRepoAccount {
    pub active: bool,
    pub did: String,
    pub seq: i64,
    pub time: DateTime<Utc>,
}

#[derive(Debug)]
pub enum JetstreamRepoMessage {
    Commit(JetstreamRepoCommitMessage),
    Identity(JetstreamRepoIdentityMessage),
    Account(JetstreamRepoAccountMessage),
}

pub fn read(data: &str) -> Result<JetstreamRepoMessage> {
    let data_json: serde_json::Value = serde_json::from_str(&data)?;

    let binding = data_json.clone();
    let kind = binding["kind"].as_str().unwrap();

    let body = match kind {
        "commit" => JetstreamRepoMessage::Commit(serde_json::from_value(data_json)?),
        "account" => JetstreamRepoMessage::Account(serde_json::from_value(data_json)?),
        "identity" => JetstreamRepoMessage::Identity(serde_json::from_value(data_json)?),
        _ => {
            eprintln!("Received unknown kind {:?}", kind);
            bail!(format!("Received unknown kind {:?}", kind))
        }
    };

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_lexicon::com::atproto::repo::StrongRef;

    #[test]
    fn test_read_commit_create_like() {
        let data = "{\"did\":\"did:plc:uhtptnlcrj4wrxfjfcanf34q\",\"time_us\":1731539977109649,\"kind\":\"commit\",\"commit\":{\"rev\":\"3lauicnwejh2f\",\"operation\":\"create\",\"collection\":\"app.bsky.feed.like\",\"rkey\":\"3lauicnw5op2f\",\"record\":{\"$type\":\"app.bsky.feed.like\",\"createdAt\":\"2024-11-13T23:19:36.449Z\",\"subject\":{\"cid\":\"bafyreigw5ufnkavdzcczl2dusa3bcnkckhi4tscp6qsrsmg76s3ckseney\",\"uri\":\"at://did:plc:6wthaiuqiys3y7eztkpsdam2/app.bsky.feed.post/3latjcehsho2n\"}},\"cid\":\"bafyreifsdaip3s5nm3hcz4fbgkxodnils75oi3rmqhipwtom34rxw4vwdi\"}}";
        let response = read(data).unwrap();
        let expected_response = JetstreamRepoCommitMessage {
            did: "did:plc:uhtptnlcrj4wrxfjfcanf34q".to_string(),
            time_us: 1731539977109649,
            kind: "commit".to_string(),
            commit: JetstreamRepoCommit {
                rev: "3lauicnwejh2f".to_string(),
                operation: "create".to_string(),
                collection: "app.bsky.feed.like".to_string(),
                rkey: "3lauicnw5op2f".to_string(),
                record: Some(Lexicon::AppBskyFeedLike {
                    0: Like {
                        created_at: "2024-11-13T23:19:36.449Z".to_string(),
                        subject: StrongRef {
                            uri: "at://did:plc:6wthaiuqiys3y7eztkpsdam2/app.bsky.feed.post/3latjcehsho2n".to_string(),
                            cid: "bafyreigw5ufnkavdzcczl2dusa3bcnkckhi4tscp6qsrsmg76s3ckseney".to_string(),
                        },
                    },
                }),
                cid: Some("bafyreifsdaip3s5nm3hcz4fbgkxodnils75oi3rmqhipwtom34rxw4vwdi".to_string()),
            },
        };

        match response {
            JetstreamRepoMessage::Commit(commit) => {
                assert_eq!(commit, expected_response);
            }
            JetstreamRepoMessage::Identity(_) => {
                panic!()
            }
            JetstreamRepoMessage::Account(_) => {
                panic!()
            }
        }
    }

    #[test]
    fn test_read_commit_delete_like() {
        let data = "{\"did\":\"did:plc:zfr76ms7mkg6ct7qldg5c3z5\",\"time_us\":1731623029598761,\"kind\":\"commit\",\"commit\":{\"rev\":\"3lawvnsupm222\",\"operation\":\"delete\",\"collection\":\"app.bsky.graph.follow\",\"rkey\":\"3kwrdj3olqr2t\"}}";
        let response = read(data).unwrap();
        let expected_response = JetstreamRepoCommitMessage {
            did: "did:plc:zfr76ms7mkg6ct7qldg5c3z5".to_string(),
            time_us: 1731623029598761,
            kind: "commit".to_string(),
            commit: JetstreamRepoCommit {
                rev: "3lawvnsupm222".to_string(),
                operation: "delete".to_string(),
                collection: "app.bsky.graph.follow".to_string(),
                rkey: "3kwrdj3olqr2t".to_string(),
                record: None,
                cid: None,
            },
        };

        match response {
            JetstreamRepoMessage::Commit(commit) => {
                assert_eq!(commit, expected_response);
            }
            JetstreamRepoMessage::Identity(_) => {
                panic!()
            }
            JetstreamRepoMessage::Account(_) => {
                panic!()
            }
        }
    }

    #[test]
    fn test_read_account_active() {
        let data = "{\"did\":\"did:plc:pvvfw4tru5kvzrpra5dairkv\",\"time_us\":1731623029648609,\"kind\":\"account\",\"account\":{\"active\":true,\"did\":\"did:plc:pvvfw4tru5kvzrpra5dairkv\",\"seq\":3478739895,\"time\":\"2024-11-14T22:23:49.092Z\"}}";
        let response = read(data).unwrap();
        let expected_response = JetstreamRepoAccountMessage {
            did: "did:plc:pvvfw4tru5kvzrpra5dairkv".to_string(),
            time_us: 1731623029648609,
            kind: "account".to_string(),
            account: JetstreamRepoAccount {
                active: true,
                did: "did:plc:pvvfw4tru5kvzrpra5dairkv".to_string(),
                seq: 3478739895,
                time: DateTime::parse_from_str("2024-11-14T22:23:49.092Z", "%+")
                    .unwrap()
                    .to_utc(),
            },
        };

        match response {
            JetstreamRepoMessage::Commit(_) => {
                panic!()
            }
            JetstreamRepoMessage::Identity(_) => {
                panic!()
            }
            JetstreamRepoMessage::Account(account) => {
                assert_eq!(account, expected_response);
            }
        }
    }

    #[test]
    fn test_read_identity() {
        let data = "{\"did\":\"did:plc:sh5zdynqtvfavtkv6estb73d\",\"time_us\":1731623029695659,\"kind\":\"identity\",\"identity\":{\"did\":\"did:plc:sh5zdynqtvfavtkv6estb73d\",\"handle\":\"irlasajj.bsky.social\",\"seq\":3478739942,\"time\":\"2024-11-14T22:23:49.147Z\"}}";
        let response = read(data).unwrap();
        let expected_response = JetstreamRepoIdentityMessage {
            did: "did:plc:sh5zdynqtvfavtkv6estb73d".to_string(),
            time_us: 1731623029695659,
            kind: "identity".to_string(),
            identity: JetstreamRepoIdentity {
                did: "did:plc:sh5zdynqtvfavtkv6estb73d".to_string(),
                handle: "irlasajj.bsky.social".to_string(),
                seq: 3478739942,
                time: DateTime::parse_from_str("2024-11-14T22:23:49.147Z", "%+")
                    .unwrap()
                    .to_utc(),
            },
        };

        match response {
            JetstreamRepoMessage::Commit(_) => {
                panic!()
            }
            JetstreamRepoMessage::Identity(identity) => {
                assert_eq!(identity, expected_response);
            }
            JetstreamRepoMessage::Account(_) => {
                panic!()
            }
        }
    }
}
