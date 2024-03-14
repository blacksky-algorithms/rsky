use libipld::Cid;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataStoreError {
    #[error("missing block `{0}`")]
    MissingBlock(String),
    #[error("missing `{0}` blocks: `{1:?}`")]
    MissingBlocks(String, Vec<Cid>),
    #[error("unexpected object at `{0}`")]
    UnexpectedObject(Cid),
    #[error("unknown data store error")]
    Unknown,
}
