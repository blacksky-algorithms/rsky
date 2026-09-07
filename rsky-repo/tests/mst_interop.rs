use anyhow::Result;
use lexicon_cid::Cid;
use rsky_repo::mst::util::{count_prefix_len, leading_zeros_on_hash};
use rsky_repo::mst::MST;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

const COMMON_PREFIX_VECTORS: &str = include_str!("interop/common_prefix.json");
const KEY_HEIGHT_VECTORS: &str = include_str!("interop/key_heights.json");
const COMMIT_PROOF_VECTORS: &str = include_str!("interop/commit-proof-fixtures.json");

#[derive(Deserialize)]
struct CommonPrefixCase {
    left: String,
    right: String,
    len: usize,
}

#[derive(Deserialize)]
struct KeyHeightCase {
    key: String,
    height: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitProofCase {
    comment: String,
    leaf_value: String,
    keys: Vec<String>,
    adds: Vec<String>,
    dels: Vec<String>,
    root_before_commit: String,
    root_after_commit: String,
    blocks_in_proof: Vec<String>,
}

#[test]
fn mst_common_prefix_vectors() -> Result<()> {
    let cases: Vec<CommonPrefixCase> = serde_json::from_str(COMMON_PREFIX_VECTORS)?;
    assert_eq!(cases.len(), 13, "common_prefix.json case count");

    for case in &cases {
        assert_eq!(
            count_prefix_len(case.left.clone(), case.right.clone())?,
            case.len,
            "left={:?} right={:?}",
            case.left,
            case.right
        );
    }
    Ok(())
}

#[test]
fn mst_key_height_vectors() -> Result<()> {
    let cases: Vec<KeyHeightCase> = serde_json::from_str(KEY_HEIGHT_VECTORS)?;
    assert_eq!(cases.len(), 9, "key_heights.json case count");

    for case in &cases {
        assert_eq!(
            leading_zeros_on_hash(case.key.as_bytes())?,
            case.height,
            "key={:?}",
            case.key
        );
    }
    Ok(())
}

/// One inverted operation: `true` re-adds a deleted key, `false` removes an added key.
type Inversion = (String, bool);

fn permutations(items: &[Inversion]) -> Vec<Vec<Inversion>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut perm in permutations(&rest) {
            perm.insert(0, head.clone());
            out.push(perm);
        }
    }
    out
}

async fn run_commit_proof_case(case: &CommitProofCase) -> Result<()> {
    let leaf = Cid::try_from(case.leaf_value.as_str())?;
    let storage = Arc::new(RwLock::new(MemoryBlockstore::default()));
    let mut tree = MST::create(storage.clone(), None, None).await?;

    for key in &case.keys {
        tree = tree.add(key, leaf, None).await?;
    }
    assert_eq!(
        tree.get_pointer().await?.to_string(),
        case.root_before_commit,
        "rootBeforeCommit [{}]",
        case.comment
    );

    for key in &case.adds {
        tree = tree.add(key, leaf, None).await?;
    }
    for key in &case.dels {
        tree = tree.delete(key).await?;
    }
    let root_after = tree.get_pointer().await?;
    assert_eq!(
        root_after.to_string(),
        case.root_after_commit,
        "rootAfterCommit [{}]",
        case.comment
    );
    tree.save_mst().await?;

    let proof_cids = case
        .blocks_in_proof
        .iter()
        .map(|cid| Cid::try_from(cid.as_str()))
        .collect::<Result<Vec<Cid>, _>>()?;
    let proof_blocks = {
        let storage_guard = storage.read().await;
        storage_guard.get_blocks(proof_cids.clone()).await?
    };
    assert!(
        proof_blocks.missing.is_empty(),
        "blocksInProof not produced by the committed tree [{}]: {:?}",
        case.comment,
        proof_blocks.missing
    );
    assert_eq!(
        proof_blocks.blocks.size(),
        proof_cids.len(),
        "blocksInProof block count [{}]",
        case.comment
    );

    let proof_store = MemoryBlockstore::new(Some(proof_blocks.blocks)).await?;
    let proof_store: Arc<RwLock<MemoryBlockstore>> = Arc::new(RwLock::new(proof_store));

    let mut inversions: Vec<Inversion> = case.adds.iter().map(|k| (k.clone(), false)).collect();
    inversions.extend(case.dels.iter().map(|k| (k.clone(), true)));

    // The proof must let a consumer invert the commit in any order, holding no other blocks.
    for order in permutations(&inversions) {
        let mut inverted = MST::load(proof_store.clone(), root_after, None)?;
        for (key, re_add) in order {
            inverted = if re_add {
                inverted.add(&key, leaf, None).await?
            } else {
                inverted.delete(&key).await?
            };
        }
        assert_eq!(
            inverted.get_pointer().await?.to_string(),
            case.root_before_commit,
            "inverted root [{}]",
            case.comment
        );
    }

    Ok(())
}

#[tokio::test]
async fn firehose_commit_proof_vectors() -> Result<()> {
    let cases: Vec<CommitProofCase> = serde_json::from_str(COMMIT_PROOF_VECTORS)?;
    assert_eq!(cases.len(), 6, "commit-proof-fixtures.json case count");

    for case in &cases {
        run_commit_proof_case(case).await?;
    }
    Ok(())
}
