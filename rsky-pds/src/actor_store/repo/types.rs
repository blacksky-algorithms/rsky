use lexicon_cid::Cid;
use rsky_repo::block_map::BlockMap;
pub struct SyncEvtData {
    pub cid: Cid,
    pub rev: String,
    pub blocks: BlockMap,
}
