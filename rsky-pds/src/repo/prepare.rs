use crate::lexicon::LEXICONS;
use anyhow::bail;
use lazy_static::lazy_static;
use lexicon_cid::Cid;
use rsky_common::ipld::cid_for_cbor;
use rsky_common::tid::Ticker;
use rsky_lexicon::blob_refs::{BlobRef, JsonBlobRef};
use rsky_repo::storage::Ipld;
use rsky_repo::types::{
    BlobConstraint, Ids, Lex, PreparedBlobRef, PreparedCreateOrUpdate, PreparedDelete, RepoRecord,
    WriteOpAction,
};
use rsky_repo::util::{cbor_to_lex, lex_to_ipld};
use rsky_syntax::aturi::AtUri;
use serde_json::{json, Value as JsonValue};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FoundBlobRef {
    pub r#ref: BlobRef,
    pub path: Vec<String>,
}

pub struct PrepareCreateOpts {
    pub did: String,
    pub collection: String,
    pub rkey: Option<String>,
    pub swap_cid: Option<Cid>,
    pub record: RepoRecord,
    pub validate: Option<bool>,
}

pub struct PrepareUpdateOpts {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub swap_cid: Option<Cid>,
    pub record: RepoRecord,
    pub validate: Option<bool>,
}

pub struct PrepareDeleteOpts {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub swap_cid: Option<Cid>,
}

pub fn blobs_for_write(record: RepoRecord, validate: bool) -> anyhow::Result<Vec<PreparedBlobRef>> {
    let refs = find_blob_refs(Lex::Map(record.clone()), None, None);
    let record_type = match record.get("$type") {
        Some(Lex::Ipld(Ipld::String(t))) => Some(t),
        _ => None,
    };
    for r#ref in refs.clone() {
        if matches!(r#ref.r#ref.original, JsonBlobRef::Untyped(_)) {
            bail!("Legacy blob ref at `{}`", r#ref.path.join("/"))
        }
    }
    refs.into_iter()
        .map(|FoundBlobRef { r#ref, path }| {
            let constraints: BlobConstraint = match (validate, record_type) {
                (true, Some(record_type)) => {
                    let properties: crate::lexicon::lexicons::Image2 = serde_json::from_value(
                        crate::repo::prepare::CONSTRAINTS[record_type.as_str()][path.join("/")]
                            .clone(),
                    )?;
                    BlobConstraint {
                        max_size: Some(properties.max_size as usize),
                        accept: Some(properties.accept),
                    }
                }
                (_, _) => BlobConstraint {
                    max_size: None,
                    accept: None,
                },
            };

            Ok(PreparedBlobRef {
                cid: r#ref.get_cid()?,
                mime_type: r#ref.get_mime_type().to_string(),
                constraints,
            })
        })
        .collect::<anyhow::Result<Vec<PreparedBlobRef>>>()
}

pub fn find_blob_refs(val: Lex, path: Option<Vec<String>>, layer: Option<u8>) -> Vec<FoundBlobRef> {
    let layer = layer.unwrap_or(0);
    let path = path.unwrap_or_default();
    if layer > 32 {
        return vec![];
    }
    // walk arrays
    match val {
        Lex::List(list) => list
            .into_iter()
            .flat_map(|item| find_blob_refs(item, Some(path.clone()), Some(layer + 1)))
            .collect::<Vec<FoundBlobRef>>(),
        Lex::Blob(blob) => vec![FoundBlobRef { r#ref: blob, path }],
        Lex::Ipld(Ipld::Json(JsonValue::Array(list))) => list
            .into_iter()
            .flat_map(|item| match serde_json::from_value::<RepoRecord>(item) {
                Ok(item) => find_blob_refs(Lex::Map(item), Some(path.clone()), Some(layer + 1)),
                Err(_) => vec![],
            })
            .collect::<Vec<FoundBlobRef>>(),
        Lex::Ipld(Ipld::Json(json)) => match serde_json::from_value::<JsonBlobRef>(json.clone()) {
            Ok(blob) => vec![FoundBlobRef {
                r#ref: BlobRef { original: blob },
                path,
            }],
            Err(_) => match serde_json::from_value::<RepoRecord>(json) {
                Ok(record) => record
                    .into_iter()
                    .flat_map(|(key, item)| {
                        find_blob_refs(
                            item,
                            Some([path.as_slice(), [key].as_slice()].concat()),
                            Some(layer + 1),
                        )
                    })
                    .collect::<Vec<FoundBlobRef>>(),
                Err(_) => vec![],
            },
        },
        Lex::Ipld(Ipld::Map(map)) => match serde_json::to_value(Ipld::Map(map.clone()))
            .ok()
            .and_then(|json| serde_json::from_value::<JsonBlobRef>(json).ok())
        {
            Some(blob) => vec![FoundBlobRef {
                r#ref: BlobRef { original: blob },
                path,
            }],
            None => map
                .into_iter()
                .flat_map(|(key, item)| {
                    find_blob_refs(
                        Lex::Ipld(item),
                        Some([path.as_slice(), [key].as_slice()].concat()),
                        Some(layer + 1),
                    )
                })
                .collect::<Vec<FoundBlobRef>>(),
        },
        Lex::Ipld(Ipld::List(list)) => list
            .into_iter()
            .flat_map(|item| find_blob_refs(Lex::Ipld(item), Some(path.clone()), Some(layer + 1)))
            .collect::<Vec<FoundBlobRef>>(),
        Lex::Ipld(_) => vec![],
        Lex::Map(map) => map
            .into_iter()
            .flat_map(|(key, item)| {
                find_blob_refs(
                    item,
                    Some([path.as_slice(), [key].as_slice()].concat()),
                    Some(layer + 1),
                )
            })
            .collect::<Vec<FoundBlobRef>>(),
    }
}

pub fn assert_valid_record(record: &RepoRecord) -> anyhow::Result<()> {
    match record.get("$type") {
        Some(Lex::Ipld(Ipld::String(_))) => Ok(()),
        _ => bail!("No $type provided"),
    }
}

pub fn set_collection_name(
    collection: &String,
    mut record: RepoRecord,
    validate: bool,
) -> anyhow::Result<RepoRecord> {
    if !record.contains_key("$type") {
        record.insert(
            "$type".to_string(),
            Lex::Ipld(Ipld::Json(JsonValue::String(collection.clone()))),
        );
    }
    if let Some(Lex::Ipld(Ipld::Json(JsonValue::String(record_type)))) = record.get("$type") {
        if validate && record_type != collection {
            bail!("Invalid $type: expected {collection}, got {record_type}")
        }
    }
    Ok(record)
}

pub async fn cid_for_safe_record(record: RepoRecord) -> anyhow::Result<Cid> {
    let lex = lex_to_ipld(Lex::Map(record));
    let block = serde_ipld_dagcbor::to_vec(&lex)?;
    // Confirm whether Block properly transforms between lex and cbor
    let _ = cbor_to_lex(block)?;
    cid_for_cbor(&lex)
}

pub async fn prepare_create(opts: PrepareCreateOpts) -> anyhow::Result<PreparedCreateOrUpdate> {
    let PrepareCreateOpts {
        did,
        collection,
        rkey,
        swap_cid,
        validate,
        ..
    } = opts;
    let validate = validate.unwrap_or(true);

    let record = set_collection_name(&collection, opts.record, validate)?;
    if validate {
        assert_valid_record(&record)?;
    }

    // assert_no_explicit_slurs(rkey, record).await?;
    let next_rkey = Ticker::new().next(None);
    let rkey = rkey.unwrap_or(next_rkey.to_string());
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedCreateOrUpdate {
        action: WriteOpAction::Create,
        uri: uri.to_string(),
        cid: cid_for_safe_record(record.clone()).await?,
        swap_cid,
        record: record.clone(),
        blobs: blobs_for_write(record, validate)?,
    })
}

pub async fn prepare_update(opts: PrepareUpdateOpts) -> anyhow::Result<PreparedCreateOrUpdate> {
    let PrepareUpdateOpts {
        did,
        collection,
        rkey,
        swap_cid,
        validate,
        ..
    } = opts;
    let validate = validate.unwrap_or(true);

    let record = set_collection_name(&collection, opts.record, validate)?;
    if validate {
        assert_valid_record(&record)?;
    }
    // assert_no_explicit_slurs(rkey, record).await?;
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedCreateOrUpdate {
        action: WriteOpAction::Update,
        uri: uri.to_string(),
        cid: cid_for_safe_record(record.clone()).await?,
        swap_cid,
        record: record.clone(),
        blobs: blobs_for_write(record, validate)?,
    })
}

pub fn prepare_delete(opts: PrepareDeleteOpts) -> anyhow::Result<PreparedDelete> {
    let PrepareDeleteOpts {
        did,
        collection,
        rkey,
        swap_cid,
    } = opts;
    let uri = AtUri::make(did, Some(collection), Some(rkey))?;
    Ok(PreparedDelete {
        action: WriteOpAction::Delete,
        uri: uri.to_string(),
        swap_cid,
    })
}

lazy_static! {
    static ref CONSTRAINTS: JsonValue = {
        json!({
            Ids::AppBskyActorProfile.as_str(): {
                "avatar": LEXICONS.app_bsky_actor_profile.defs.main.record.properties.avatar,
                "banner": LEXICONS.app_bsky_actor_profile.defs.main.record.properties.banner
            },
            Ids::AppBskyFeedGenerator.as_str(): {
                "avatar": LEXICONS.app_bsky_feed_generator.defs.main.record.properties.avatar
            },
            Ids::AppBskyGraphList.as_str(): {
                "avatar": LEXICONS.app_bsky_graph_list.defs.main.record.properties.avatar
            },
            Ids::AppBskyFeedPost.as_str(): {
                "embed/images/image": LEXICONS.app_bsky_embed_images.defs.image.properties.image,
                "embed/external/thumb": LEXICONS.app_bsky_embed_external.defs.external.properties.thumb,
                "embed/video": LEXICONS.app_bsky_embed_video.defs.main.properties.video,
                "embed/captions/file": LEXICONS.app_bsky_embed_video.defs.caption.properties.file,
                "embed/media/images/image": LEXICONS.app_bsky_embed_images.defs.image.properties.image,
                "embed/media/external/thumb": LEXICONS.app_bsky_embed_external.defs.external.properties.thumb,
                "embed/media/video": LEXICONS.app_bsky_embed_video.defs.main.properties.video,
                "embed/media/captions/file": LEXICONS.app_bsky_embed_video.defs.caption.properties.file
            }
        })
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO_CID: &str = "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku";
    const VTT_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

    fn blob_json(cid: &str, mime_type: &str, size: i64) -> JsonValue {
        json!({
            "$type": "blob",
            "ref": { "$link": cid },
            "mimeType": mime_type,
            "size": size,
        })
    }

    fn video_post_record() -> RepoRecord {
        serde_json::from_value(json!({
            "$type": "app.bsky.feed.post",
            "text": "video post",
            "createdAt": "2026-01-01T00:00:00.000Z",
            "embed": {
                "$type": "app.bsky.embed.video",
                "video": blob_json(VIDEO_CID, "video/mp4", 1_048_576),
                "captions": [
                    { "lang": "en", "file": blob_json(VTT_CID, "text/vtt", 1024) }
                ],
            },
        }))
        .expect("record deserializes")
    }

    #[test]
    fn finds_blob_refs_in_video_embed() {
        let refs = find_blob_refs(Lex::Map(video_post_record()), None, None);
        let mut paths = refs
            .iter()
            .map(|r| r.path.join("/"))
            .collect::<Vec<String>>();
        paths.sort();
        assert_eq!(paths, vec!["embed/captions/file", "embed/video"]);
    }

    #[test]
    fn blobs_for_write_applies_video_constraints() {
        let prepared = blobs_for_write(video_post_record(), true).expect("blobs prepared");
        let video = prepared
            .iter()
            .find(|blob| blob.mime_type == "video/mp4")
            .expect("video blob present");
        assert_eq!(video.cid.to_string(), VIDEO_CID);
        assert_eq!(
            video.constraints.accept,
            Some(vec!["video/mp4".to_string()])
        );
        assert_eq!(video.constraints.max_size, Some(100_000_000));
        let captions = prepared
            .iter()
            .find(|blob| blob.mime_type == "text/vtt")
            .expect("captions blob present");
        assert_eq!(captions.cid.to_string(), VTT_CID);
        assert_eq!(
            captions.constraints.accept,
            Some(vec!["text/vtt".to_string()])
        );
        assert_eq!(captions.constraints.max_size, Some(20_000));
    }

    #[test]
    fn finds_blob_refs_in_image_embed() {
        let record: RepoRecord = serde_json::from_value(json!({
            "$type": "app.bsky.feed.post",
            "text": "image post",
            "createdAt": "2026-01-01T00:00:00.000Z",
            "embed": {
                "$type": "app.bsky.embed.images",
                "images": [
                    { "image": blob_json(VIDEO_CID, "image/png", 512), "alt": "" }
                ],
            },
        }))
        .expect("record deserializes");
        let refs = find_blob_refs(Lex::Map(record), None, None);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path.join("/"), "embed/images/image");
    }
}
