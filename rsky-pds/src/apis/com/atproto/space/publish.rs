//! Publishing policy state as records (spec §Publishing to readers).
//!
//! `memberspace.publishPolicy`/`publishMembers` and
//! `managedspace.publishPolicy` make the authority's PDS write
//! `com.atproto.space.policy`/`self` and `com.atproto.memberspace.member`/
//! `{did}` records into the authority's own repo in the space, where they
//! sync to readers like any other record. The config stays the source of
//! truth; the records are a copy the PDS keeps in sync.

use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::{MemberRow, SpacePolicy, SpaceStore, SpaceWrite};
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::{apply_space_writes, space_def_error};
use crate::apis::ApiError;
use crate::config::ServerConfig;
use rocket::State;
use rsky_space::space_id::SpaceId;
use serde_json::json;

/// The collections the space machinery manages in the authority's repo.
pub const POLICY_COLLECTION: &str = "com.atproto.space.policy";
pub const MEMBER_COLLECTION: &str = "com.atproto.memberspace.member";
pub const MANAGED_COLLECTIONS: [&str; 2] = [POLICY_COLLECTION, MEMBER_COLLECTION];

/// Direct CRUD writes of the managed collections into the authority's own
/// repo are rejected: only the PDS's publishing path writes them (spec
/// §Publishing to readers). Members may still write the types into their own
/// repos, where readers ignore them.
pub fn reject_managed_collections(
    space: &SpaceId,
    repo_did: &str,
    collection: &str,
) -> Result<(), ApiError> {
    if repo_did == space.authority && MANAGED_COLLECTIONS.contains(&collection) {
        Err(ApiError::InvalidRequest(format!(
            "{collection} records are managed by the space's policy"
        )))
    } else {
        Ok(())
    }
}

fn publish_policy_flag(policy: &SpacePolicy) -> bool {
    match policy {
        SpacePolicy::Personalspace(_) => false,
        SpacePolicy::Memberspace(m) => m.publish_policy.unwrap_or(false),
        SpacePolicy::Managedspace(m) => m.publish_policy.unwrap_or(false),
    }
}

fn publish_members_flag(policy: &SpacePolicy) -> bool {
    match policy {
        SpacePolicy::Memberspace(m) => m.publish_members.unwrap_or(false),
        _ => false,
    }
}

fn policy_record(policy: &SpacePolicy) -> SpaceWrite {
    SpaceWrite::Update {
        collection: POLICY_COLLECTION.to_string(),
        rkey: "self".to_string(),
        value: json!({
            "$type": POLICY_COLLECTION,
            "policy": serde_json::to_value(policy).expect("policy serializes"),
        }),
        swap_cid: None,
    }
}

fn member_record(member: &str, read: bool, write: bool) -> SpaceWrite {
    SpaceWrite::Update {
        collection: MEMBER_COLLECTION.to_string(),
        rkey: member.to_string(),
        value: json!({
            "$type": MEMBER_COLLECTION,
            "read": read,
            "write": write,
        }),
        swap_cid: None,
    }
}

async fn apply_published(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    authority_did: &str,
    space: &SpaceId,
    writes: Vec<SpaceWrite>,
) -> Result<(), ApiError> {
    if writes.is_empty() {
        return Ok(());
    }
    apply_space_writes(
        actor_store,
        blobstore_factory,
        server_config,
        authority_did,
        space,
        writes,
    )
    .await?;
    Ok(())
}

/// After `createSpace`/`updateSpace` stores the new def, reconcile the
/// published records with it: upsert the policy record when the new policy
/// publishes, drop member records for the old member list, then republish
/// the current one under `memberspace.publishMembers`.
#[allow(clippy::too_many_arguments)]
pub async fn reconcile_published(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    space_store: &SpaceStore,
    authority_did: &str,
    space: &SpaceId,
    policy: &SpacePolicy,
    dropped_members: Vec<String>,
) -> Result<(), ApiError> {
    let mut writes: Vec<SpaceWrite> = Vec::new();
    if publish_policy_flag(policy) {
        writes.push(policy_record(policy));
    } else if space_store
        .get_record(&space.uri(), POLICY_COLLECTION, "self")
        .await
        .map_err(space_def_error)?
        .is_some()
    {
        writes.push(SpaceWrite::Delete {
            collection: POLICY_COLLECTION.to_string(),
            rkey: "self".to_string(),
            swap_cid: None,
        });
    }
    // Published member records not covered by the current member list come
    // down; the drop set is advisory, existence is what counts.
    let mut published = Vec::new();
    let mut cursor = None;
    loop {
        let page = space_store
            .list_records(&space.uri(), Some(MEMBER_COLLECTION.to_string()), 500, cursor.clone())
            .await
            .map_err(space_def_error)?;
        let full_page = page.len() == 500;
        published.extend(page.into_iter().map(|row| row.rkey));
        cursor = published.last().map(|rkey| format!("{MEMBER_COLLECTION}/{rkey}"));
        if !full_page {
            break;
        }
    }
    let mut current: std::collections::BTreeSet<String> = dropped_members.into_iter().collect();
    if publish_members_flag(policy) {
        let mut cursor = None;
        loop {
            let page = space_store
                .list_members(&space.uri(), 500, cursor.clone())
                .await
                .map_err(space_def_error)?;
            let full_page = page.len() == 500;
            for MemberRow { did, read, write, .. } in &page {
                writes.push(member_record(did, *read, *write));
                current.remove(did);
            }
            cursor = page.last().map(|row| row.did.clone());
            if !full_page {
                break;
            }
        }
    }
    for did in published {
        if current.contains(&did) || !publish_members_flag(policy) {
            writes.push(SpaceWrite::Delete {
                collection: MEMBER_COLLECTION.to_string(),
                rkey: did,
                swap_cid: None,
            });
        }
    }
    apply_published(
        actor_store,
        blobstore_factory,
        server_config,
        authority_did,
        space,
        writes,
    )
    .await
    .map_err(|error| {
        tracing::warn!(%error, "failed to publish space records");
        error
    })
}

/// `putMember` keeps a published member record in sync.
#[allow(clippy::too_many_arguments)]
pub async fn publish_member(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    authority_did: &str,
    space: &SpaceId,
    policy: &SpacePolicy,
    member: &str,
    read: bool,
    write: bool,
) -> Result<(), ApiError> {
    if !publish_members_flag(policy) {
        return Ok(());
    }
    apply_published(
        actor_store,
        blobstore_factory,
        server_config,
        authority_did,
        space,
        vec![member_record(member, read, write)],
    )
    .await
    .map_err(|error| {
        tracing::warn!(%error, "failed to publish member record");
        error
    })
}

/// `removeMember` drops a published member record.
#[allow(clippy::too_many_arguments)]
pub async fn unpublish_member(
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    server_config: &State<ServerConfig>,
    authority_did: &str,
    space: &SpaceId,
    member: &str,
) -> Result<(), ApiError> {
    // The record only exists if the space publishes members; look before
    // deleting so un-published removals are a no-op.
    let space_store = actor_store
        .read(
            authority_did.to_string(),
            blobstore_factory.blobstore(authority_did.to_string()),
        )
        .await
        .map_err(|error| ApiError::BadRequest("RepoNotFound".to_string(), error.to_string()))?;
    if space_store
        .space
        .get_record(&space.uri(), MEMBER_COLLECTION, member)
        .await
        .map_err(space_def_error)?
        .is_none()
    {
        return Ok(());
    }
    apply_published(
        actor_store,
        blobstore_factory,
        server_config,
        authority_did,
        space,
        vec![SpaceWrite::Delete {
            collection: MEMBER_COLLECTION.to_string(),
            rkey: member.to_string(),
            swap_cid: None,
        }],
    )
    .await
    .map_err(|error| {
        tracing::warn!(%error, "failed to unpublish member record");
        error
    })
}

/// A memberspace's member DID list (for `dropped_members` on a policy change).
pub async fn member_dids(space_store: &SpaceStore, space_uri: &str) -> Result<Vec<String>, ApiError> {
    let mut dids = Vec::new();
    let mut cursor = None;
    loop {
        let page = space_store
            .list_members(space_uri, 500, cursor.clone())
            .await
            .map_err(space_def_error)?;
        let full_page = page.len() == 500;
        dids.extend(page.iter().map(|row| row.did.clone()));
        cursor = page.last().map(|row| row.did.clone());
        if !full_page {
            return Ok(dids);
        }
    }
}
