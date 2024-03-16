use crate::repo::mst::{NodeEntry, MST};
use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WalkerStatusDone(bool);

#[derive(Clone)]
pub struct WalkerStatusProgress<'a> {
    pub done: bool,
    pub curr: NodeEntry<'a>,
    pub walking: Option<MST<'a>>, // walking set to null if `curr` is the root of the tree
    pub index: usize,
}

#[derive(Clone)]
pub enum WalkerStatus<'a> {
    WalkerStatusDone(WalkerStatusDone),
    WalkerStatusProgress(WalkerStatusProgress<'a>),
}

#[derive(Clone)]
pub struct MstWalker<'a> {
    pub stack: Vec<WalkerStatus<'a>>,
    pub status: WalkerStatus<'a>,
}

impl<'a> MstWalker<'a> {
    pub fn new(root: MST) -> Self {
        MstWalker {
            stack: Vec::new(),
            status: WalkerStatus::WalkerStatusProgress(WalkerStatusProgress {
                done: false,
                curr: NodeEntry::MST(root),
                walking: None,
                index: 0,
            }),
        }
    }

    /// return the current layer of the node you are walking
    pub fn layer(&mut self) -> Result<usize> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => bail!("Walk is done"),
            WalkerStatus::WalkerStatusProgress(ref p) => {
                if let Some(ref mst) = p.walking {
                    return Ok(mst.layer.unwrap_or(0) as usize);
                } else {
                    // if curr is the root of the tree, add 1
                    if let NodeEntry::MST(ref mst) = p.curr {
                        return Ok((mst.layer.unwrap_or(0) + 1) as usize);
                    }
                }
            }
        }
        bail!("Could not identify layer of walk")
    }

    /// move to the next node in the subtree, skipping over the subtree
    pub fn step_over(&mut self) -> Result<()> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let Some(ref mut mst) = p.walking {
                    let entries = mst.get_entries()?;
                    p.index += 1;
                    let next = entries.get(p.index);
                    if let Some(next) = next {
                        p.curr = next.clone();
                    } else {
                        let popped = self.stack.pop();
                        if let Some(popped) = popped {
                            self.status = popped;
                            self.step_over()?;
                        } else {
                            self.status = WalkerStatus::WalkerStatusDone(WalkerStatusDone(true));
                        }
                    }
                } else {
                    // if stepping over the root of the node, we're done
                    self.status = WalkerStatus::WalkerStatusDone(WalkerStatusDone(true));
                }
            }
        }
        Ok(())
    }

    /// step into a subtree, throws if currently pointed at a leaf
    pub fn step_into(&mut self) -> Result<()> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let Some(ref mut mst) = p.walking {
                    if let NodeEntry::MST(ref mut mst) = p.curr {
                        let next = mst.at_index(0)?;
                        if let Some(next) = next {
                            self.stack
                                .push(WalkerStatus::WalkerStatusProgress(p.clone()));
                            p.walking = Some(mst.clone());
                            p.curr = next.clone();
                            p.index = 0;
                        } else {
                            bail!("Tried to step into a node with 0 entries which is invalid");
                        }
                    } else {
                        bail!("No tree at pointer, cannot step into");
                    }
                } else {
                    if let NodeEntry::MST(ref mut mst) = p.curr {
                        let next = mst.at_index(0)?;
                        if let Some(next) = next {
                            self.status =
                                WalkerStatus::WalkerStatusProgress(WalkerStatusProgress {
                                    done: false,
                                    walking: Some(mst.clone()),
                                    curr: next.clone(),
                                    index: 0,
                                });
                        } else {
                            self.status = WalkerStatus::WalkerStatusDone(WalkerStatusDone(true));
                        }
                    } else {
                        bail!("The root of the tree cannot be a leaf");
                    }
                }
            }
        }
        Ok(())
    }

    /// advance the pointer to the next node in the tree,
    /// stepping into the current node if necessary
    pub fn advance(&mut self) -> Result<()> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let NodeEntry::Leaf(_) = p.curr {
                    self.step_over()?;
                } else {
                    self.step_into()?;
                }
            }
        }
        Ok(())
    }
}
