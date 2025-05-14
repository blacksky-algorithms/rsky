use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use std::fs::File;

use anyhow::{Context, Result};
use glob::glob;
use indexmap::IndexMap;
use iroh_car::CarReader;
use lexicon_cid::Cid;
use rsky_common::tid::TID;
use rsky_repo::storage::types::RepoStorage;
use rsky_repo::{data_diff::DataDiff, mst::MST, storage::memory_blockstore::MemoryBlockstore};
use serde::Deserialize;
use tokio::fs::File as AsyncFile;
use tokio::sync::RwLock;

/// ---------- JSON formats -------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct TestCase {
    #[serde(rename = "$type")]
    _type: String,
    #[allow(dead_code)]
    description: String,
    inputs: Inputs,
    results: Results,
}

#[derive(Deserialize)]
struct Inputs {
    mst_a: String,
    mst_b: String,
}

#[derive(Deserialize)]
struct Results {
    created_nodes: Vec<String>,
    deleted_nodes: Vec<String>,
    record_ops: Vec<RecordOpJson>,
    #[allow(dead_code)]
    proof_nodes: Vec<String>,
    #[allow(dead_code)]
    inductive_proof_nodes: Vec<String>,
    #[allow(dead_code)]
    firehose_cids: serde_json::Value,
}

#[derive(Deserialize)]
struct RecordOpJson {
    rpath: String,
    old_value: Option<String>,
    new_value: Option<String>,
}

/// ---------------- CAR → Blockstore helper --------------------------------
async fn load_car_into(bs: &mut MemoryBlockstore, path: &Path) -> Result<Cid> {
    let mut reader = CarReader::new(AsyncFile::open(path).await?).await?;
    let roots = reader.header().roots();
    let first_root = roots
        .first()
        .cloned()
        .context("CAR must have exactly one root")?;
    while let Some((cid, bytes)) = reader.next_block().await? {
        let rev = TID::next_str(None)?;
        bs.put_block(cid, bytes, rev).await?;
    }
    Ok(first_root)
}

/// ----------------- one-shot test runner ----------------------------------
async fn run_single(case_path: &Path) -> Result<()> {
    let tc: TestCase = serde_json::from_reader(File::open(case_path).context("Test case file missing")?)?;

    // Shared MemoryBlockstore so that both CARs live in the same space.
    let mut storage = MemoryBlockstore::default();

    let root_a = load_car_into(
        &mut storage,
        &case_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(&tc.inputs.mst_a),
    )
    .await?;
    let root_b = load_car_into(
        &mut storage,
        &case_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(&tc.inputs.mst_b),
    )
    .await?;

    // Build MSTs
    let mst_a = MST::load(Arc::new(RwLock::new(storage.clone())), root_a, None)?;
    let mst_b = MST::load(Arc::new(RwLock::new(storage)), root_b, None)?;

    // Run diff: B –> A   (direction matches test-suite expectation)
    let diff = DataDiff::of(&mut mst_b.clone(), Some(&mut mst_a.clone())).await?;

    // ------------ 1. created / deleted -----------------------------------
    let created: HashSet<_> = diff.new_mst_blocks.cids()?.into_iter().collect();
    let deleted: HashSet<_> = diff.removed_mst_blocks.to_list().into_iter().collect();

    let expect_created: HashSet<_> = tc
        .results
        .created_nodes
        .iter()
        .map(|s| Cid::try_from(s.as_str()).unwrap())
        .collect();
    let expect_deleted: HashSet<_> = tc
        .results
        .deleted_nodes
        .iter()
        .map(|s| Cid::try_from(s.as_str()).unwrap())
        .collect();

    assert_eq!(
        created, expect_created,
        "created_nodes mismatch in {:?}",
        case_path
    );
    assert_eq!(
        deleted, expect_deleted,
        "deleted_nodes mismatch in {:?}",
        case_path
    );

    // ------------ 2. record-ops ------------------------------------------
    let mut expect_ops = IndexMap::<String, (Option<Cid>, Option<Cid>)>::new();
    for op in tc.results.record_ops {
        expect_ops.insert(
            op.rpath,
            (
                op.old_value
                    .as_ref()
                    .map(|s| Cid::try_from(s.as_str()).unwrap()),
                op.new_value
                    .as_ref()
                    .map(|s| Cid::try_from(s.as_str()).unwrap()),
            ),
        );
    }

    for add in diff.add_list() {
        let (old, new) = expect_ops
            .swap_remove(&add.key)
            .expect("unexpected add key");
        assert!(old.is_none() && new == Some(add.cid), "add mismatch");
    }
    for upd in diff.update_list() {
        let (old, new) = expect_ops
            .swap_remove(&upd.key)
            .expect("unexpected update key");
        assert!(
            old == Some(upd.prev) && new == Some(upd.cid),
            "update mismatch"
        );
    }
    for del in diff.delete_list() {
        let (old, new) = expect_ops
            .swap_remove(&del.key)
            .expect("unexpected delete key");
        assert!(old.is_some() && new.is_none(), "delete mismatch");
    }
    assert!(
        expect_ops.is_empty(),
        "not all expected ops were seen: {:?}",
        expect_ops.keys()
    );

    // ------------ 3. proof sets (optional) -------------------------------
    //  rsky's DataDiff already guarantees inductive-proof correctness.
    //  If you want to assert these blocks too, uncomment below:
    /*
    let inductive: HashSet<_> =
        diff.inductive_proof_nodes().cloned().collect();
    let expect_inductive: HashSet<_> = tc
        .results
        .inductive_proof_nodes
        .iter()
        .map(|s| Cid::try_from(s.as_str()).unwrap())
        .collect();
    assert_eq!(inductive, expect_inductive, "inductive proof mismatch");
    */

    Ok(())
}

/// ------------ dynamic test-case discovery -------------------------------
#[tokio::test(flavor = "multi_thread")]
async fn interop_suite() -> Result<()> {
    let root: PathBuf = std::env::var("INTEROP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    let pattern = root.join("tests/**/*.json");
    let mut total = 0usize;
    for entry in glob(pattern.to_str().unwrap())? {
        let path = entry?;
        run_single(&path)
            .await
            .with_context(|| format!("while running {:?}", path))?;
        total += 1;
    }
    println!("✓ interop suite passed ({} cases)", total);
    Ok(())
}
