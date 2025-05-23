use crate::mst::{NodeEntry, MST};
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WalkerStatusDone(bool);

#[derive(Clone, Debug)]
pub struct WalkerStatusProgress {
    pub done: bool,
    pub curr: NodeEntry,
    pub walking: Option<MST>, // walking set to null if `curr` is the root of the tree
    pub index: usize,
}

#[derive(Clone, Debug)]
pub enum WalkerStatus {
    WalkerStatusDone(WalkerStatusDone),
    WalkerStatusProgress(WalkerStatusProgress),
}

#[derive(Clone, Debug)]
pub struct MstWalker {
    pub stack: Vec<WalkerStatus>,
    pub status: WalkerStatus,
}

impl MstWalker {
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
    #[async_recursion(Sync)]
    pub async fn step_over(&mut self) -> Result<()> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let Some(ref mut mst) = p.walking {
                    let entries = mst.get_entries().await?;
                    p.index += 1;
                    let next = entries.into_iter().nth(p.index);
                    if let Some(next) = next {
                        p.curr = next.clone();
                    } else {
                        let popped = self.stack.pop();
                        if let Some(popped) = popped {
                            self.status = popped;
                            self.step_over().await?;
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
    pub async fn step_into(&mut self) -> Result<()> {
        let clone_of_current_status = self.status.clone();
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let Some(_) = p.walking {
                    if let NodeEntry::MST(ref mut curr) = p.curr {
                        let next = curr.at_index(0).await?;
                        if let Some(next) = next {
                            self.stack.push(clone_of_current_status);
                            p.walking = Some(curr.clone()); // Changes walking to be parent tree
                            p.curr = next.clone(); // Changes current to be this node
                            p.index = 0;
                        } else {
                            bail!("Tried to step into a node with 0 entries which is invalid");
                        }
                    } else {
                        bail!("No tree at pointer, cannot step into");
                    }
                } else {
                    if let NodeEntry::MST(ref mut mst) = p.curr {
                        let next = mst.at_index(0).await?;
                        if let Some(next) = next {
                            self.status =
                                WalkerStatus::WalkerStatusProgress(WalkerStatusProgress {
                                    done: false,
                                    walking: Some(mst.clone()),
                                    curr: next,
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
    pub async fn advance(&mut self) -> Result<()> {
        match self.status {
            WalkerStatus::WalkerStatusDone(_) => return Ok(()),
            WalkerStatus::WalkerStatusProgress(ref mut p) => {
                if let NodeEntry::Leaf(_) = p.curr {
                    self.step_over().await?;
                } else {
                    self.step_into().await?;
                }
            }
        }
        Ok(())
    }
}
