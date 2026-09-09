use anyhow::Result;
use lexicon_cid::Cid;
use rsky_repo::block_map::BlockMap;
use rsky_repo::mst::util::{count_prefix_len, leading_zeros_on_hash};
use rsky_repo::mst::MST;
use rsky_repo::storage::memory_blockstore::MemoryBlockstore;
use rsky_repo::storage::readable_blockstore::ReadableBlockstore;
use serde::Deserialize;
use std::collections::BTreeSet;
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
            count_prefix_len(&case.left, &case.right)?,
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

    // The commit's `blocks` are the union of the covering proof taken per write, on the post-write tree.
    let mut covering_proof = BlockMap::new();
    for key in case.adds.iter().chain(case.dels.iter()) {
        covering_proof.add_map(tree.get_covering_proof(key).await?)?;
    }
    let produced: BTreeSet<String> = covering_proof
        .cids()?
        .into_iter()
        .map(|cid| cid.to_string())
        .collect();
    let expected: BTreeSet<String> = case.blocks_in_proof.iter().cloned().collect();
    // Per-walk assertions. The union above cannot attribute a block to the walk that should
    // have produced it, so a bug swapping the sibling walks would pass. These constrain each
    // walk on its own.
    let mut by_walk = Vec::new();
    for name in ["key", "left", "right"] {
        let mut acc = BlockMap::new();
        for key in case.adds.iter().chain(case.dels.iter()) {
            let one = match name {
                "key" => tree.proof_for_key(key).await?,
                "left" => tree.proof_for_left_sib(key).await?,
                _ => tree.proof_for_right_sib(key).await?,
            };
            acc.add_map(one)?;
        }
        let s: BTreeSet<String> = acc.cids()?.into_iter().map(|c| c.to_string()).collect();
        assert!(
            s.is_subset(&expected),
            "{name} walk produced a block outside blocksInProof [{}]: {:?}",
            case.comment,
            s.difference(&expected).collect::<Vec<_>>()
        );
        by_walk.push(s);
    }
    let (from_key, from_left, from_right) = (&by_walk[0], &by_walk[1], &by_walk[2]);

    // Both sibling walks add their own node at every level, so each always reaches the root.
    let root_str = root_after.to_string();
    assert!(
        from_left.contains(&root_str) && from_right.contains(&root_str),
        "a sibling walk did not reach the root [{}]",
        case.comment
    );

    // Neither sibling walk may be dropped: each fixture below records whether omitting one
    // still yields blocksInProof. Ablation measured against the upstream vectors.
    let key_left: BTreeSet<String> = from_key.union(from_left).cloned().collect();
    let key_right: BTreeSet<String> = from_key.union(from_right).cloned().collect();
    let (left_removable, right_removable) = match case.comment.as_str() {
        "add on edge with neighbor two layers down" => (false, true),
        "merge and split in multi-op commit" => (true, true),
        _ => (false, false),
    };
    assert_eq!(
        key_right == expected,
        left_removable,
        "dropping the left-sibling walk changed the proof unexpectedly [{}]",
        case.comment
    );
    assert_eq!(
        key_left == expected,
        right_removable,
        "dropping the right-sibling walk changed the proof unexpectedly [{}]",
        case.comment
    );

    // A walk that cannot be dropped must contribute a block the other two lack. Without this,
    // a left walk that had been made a duplicate of the right one still satisfies the ablation
    // above, because that check is symmetric in the two siblings.
    if !left_removable {
        assert!(
            !from_left.is_subset(&key_right),
            "left-sibling walk contributed nothing the other walks lacked [{}]",
            case.comment
        );
    }
    if !right_removable {
        assert!(
            !from_right.is_subset(&key_left),
            "right-sibling walk contributed nothing the other walks lacked [{}]",
            case.comment
        );
    }

    // The one fixture whose neighbour sits strictly to the left: the left walk must descend to
    // it while the right walk finds nothing beyond the spine. A swap of the two fails here.
    if case.comment == "add on edge with neighbor two layers down" {
        assert!(
            from_right.len() < from_left.len() && from_right.is_subset(from_left),
            "left walk did not out-reach the right walk on a left-hand neighbour [{}]",
            case.comment
        );
    }

    assert_eq!(
        produced, expected,
        "covering proof set equality [{}]",
        case.comment
    );
    assert_eq!(
        produced.len(),
        case.blocks_in_proof.len(),
        "covering proof block count [{}]",
        case.comment
    );
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
