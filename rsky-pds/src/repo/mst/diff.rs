use crate::repo::data_diff::DataDiff;
use crate::repo::mst::walker::{MstWalker, WalkerStatus};
use crate::repo::mst::{NodeEntry, MST};
use crate::storage::readable_blockstore::ReadableBlockstore;
use anyhow::{bail, Result};
use std::fmt::Debug;

pub async fn null_diff<B: ReadableBlockstore + Clone + Debug>(tree: MST<B>) -> Result<DataDiff> {
    let mut diff = DataDiff::new();
    for entry in tree.walk() {
        diff.node_add(entry.clone()).await?;
    }
    Ok(diff)
}

pub async fn mst_diff<B: ReadableBlockstore + Clone + Debug>(
    curr: &mut MST<B>,
    prev: Option<&mut MST<B>>,
) -> Result<DataDiff> {
    curr.get_pointer()?;
    return if let Some(prev) = prev {
        prev.get_pointer()?;
        let mut diff = DataDiff::new();

        let mut left_walker = MstWalker::new(prev.clone());
        let mut right_walker = MstWalker::new(curr.clone());
        while !matches!(&left_walker.status, WalkerStatus::WalkerStatusDone(_))
            || !matches!(&right_walker.status, WalkerStatus::WalkerStatusDone(_))
        {
            // if one walker is finished, continue walking the other & logging all nodes
            match (&left_walker.status, &right_walker.status) {
                (WalkerStatus::WalkerStatusDone(_), WalkerStatus::WalkerStatusProgress(ref r)) => {
                    diff.node_add(r.curr.clone()).await?;
                    right_walker.advance().await?;
                    continue;
                }
                (WalkerStatus::WalkerStatusProgress(ref l), WalkerStatus::WalkerStatusDone(_)) => {
                    diff.node_delete(l.curr.clone()).await?;
                    left_walker.advance().await?;
                    continue;
                }
                _ => (),
            }
            match (&left_walker.status, &right_walker.status) {
                (WalkerStatus::WalkerStatusDone(_), _) => break,
                (_, WalkerStatus::WalkerStatusDone(_)) => break,
                (
                    WalkerStatus::WalkerStatusProgress(ref l),
                    WalkerStatus::WalkerStatusProgress(ref r),
                ) => {
                    let mut left = l.curr.clone();
                    let mut right = r.curr.clone();

                    // if both pointers are leaves, record an update & advance both or record
                    // the lowest key and advance that pointer
                    if let (NodeEntry::Leaf(left_leaf), NodeEntry::Leaf(right_leaf)) =
                        (&left, &right)
                    {
                        if left_leaf.key == right_leaf.key {
                            if !left_leaf.value.eq(&right_leaf.value) {
                                diff.leaf_update(&left_leaf.key, left_leaf.value, right_leaf.value);
                            }
                            left_walker.advance().await?;
                            right_walker.advance().await?;
                        } else if left_leaf.key < right_leaf.key {
                            diff.leaf_delete(&left_leaf.key, left_leaf.value);
                            left_walker.advance().await?;
                        } else {
                            diff.leaf_add(&right_leaf.key, right_leaf.value);
                            right_walker.advance().await?;
                        }
                        continue;
                    }
                    // next, ensure that we're on the same layer
                    // if one walker is at a higher layer than the other, we need to do
                    // one of two things
                    // if the higher walker is pointed at a tree, step into that tree to
                    // try to catch up with the lower
                    // if the higher walker is pointed at a leaf, then advance the lower walker
                    // to try to catch up the higher
                    if left_walker.layer()? > right_walker.layer()? {
                        if left.is_leaf() {
                            diff.node_add(right).await?;
                            right_walker.advance().await?;
                        } else {
                            diff.node_delete(left).await?;
                            left_walker.step_into().await?;
                        }
                        continue;
                    } else if left_walker.layer()? < right_walker.layer()? {
                        if right.is_leaf() {
                            diff.node_delete(left).await?;
                            left_walker.advance().await?;
                        } else {
                            diff.node_add(right).await?;
                            right_walker.step_into().await?;
                        }
                        continue;
                    }

                    // if we're on the same level, and both pointers are trees, do a comparison
                    // if they're the same, step over. if they're different, step in to
                    // find the subdiff
                    if let (NodeEntry::MST(left_tree), NodeEntry::MST(right_tree)) =
                        (&mut left, &mut right)
                    {
                        if left_tree.get_pointer()?.eq(&right_tree.get_pointer()?) {
                            left_walker.step_over().await?;
                            right_walker.step_over().await?;
                        } else {
                            diff.node_add(right).await?;
                            diff.node_delete(left).await?;
                            left_walker.step_into().await?;
                            right_walker.step_into().await?;
                        }
                        continue;
                    }

                    // finally, if one pointer is a tree and the other is a leaf,
                    // simply step into the tree
                    if let (NodeEntry::Leaf(_), NodeEntry::MST(_)) = (&left, &right) {
                        diff.node_add(right).await?;
                        right_walker.step_into().await?;
                        continue;
                    } else if let (NodeEntry::MST(_), NodeEntry::Leaf(_)) = (&left, &right) {
                        diff.node_delete(left).await?;
                        left_walker.step_into().await?;
                        continue;
                    }

                    bail!("Unidentifiable case in diff walk");
                }
            }
        }
        Ok(diff)
    } else {
        Ok(null_diff(curr.clone()).await?)
    };
}
