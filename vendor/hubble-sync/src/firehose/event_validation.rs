use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use dasl::drisl;
use jacquard_api::com_atproto::sync::subscribe_repos::{
    Commit as JacquardCommit, RepoOpAction, Sync as JacquardSync,
};
use jacquard_common::types::cid::IpldCid;
use jacquard_repo::RepoError;
use jacquard_repo::car::{parse_car_bytes, reader::ParsedCar};
use jacquard_repo::commit::firehose::to_invertible_op;
use jacquard_repo::mst::{Mst, tree::VerifiedWriteOp};
use jacquard_repo::storage::MemoryBlockStore;
use metrics::histogram;
use repo_stream::{CidMismatch, Commit as RSCommit, verify_block_cid};

use crate::commit::{CommitConvertError, CommitObject, Op, OpKind};
use crate::metrics::COMMIT_PREVALIDATE_SECONDS;
use crate::{DaslCid, Tid};

const CAR_SIZE_LIMIT: usize = 2 * 2_usize.pow(20); // 2MB
const RECORD_SIZE_LIMIT: usize = 2_usize.pow(20); // 1MB

#[derive(Debug, thiserror::Error)]
pub enum FirehoseValidationError {
    #[error("CAR too big for firehose limit of {limit}: {got}")]
    CarTooBig { limit: usize, got: usize },
    #[error("Record too big for firehose limit of {limit}: {got}")]
    RecordTooBig { limit: usize, got: usize },
    #[error("Missing record link for op: {action} {path:?}")]
    MissingRecordLink { action: RepoOpAction, path: String },
    #[error("Missing record block for {action} op: {cid:?}")]
    MissingRecordBlock { action: RepoOpAction, cid: String },
    #[error("failed to invert an op: {action} {path:?}")]
    FailedToInvert { action: RepoOpAction, path: String },
    #[error("prevData mismatch after operation inversion")]
    PrevDataMismatch {
        claimed_prev_data: String,
        computed: String,
    },
    #[error("DID mismatch")]
    DidMismatch { claimed: String, in_commit: String },
    #[error("Tid mismatch")]
    TidMismatch { claimed: String, in_commit: String },
    #[error("CID mismatch")]
    CidMismatch(#[from] Box<CidMismatch>),
    #[error("commit convert: {0}")]
    CommitConvert(#[from] CommitConvertError),
    #[error("missing commit block at {0}")]
    MissingCommitBlock(String),
    #[error("bad comit block data: {0}")]
    BadCommitBlock(String),
    #[error("missing prevData field from firehose commit")]
    MissingPrevData,
    #[error("jacquard repo error: {0}")]
    RepoError(#[from] RepoError),
    #[error("invalid CID at {0}")]
    BadCid(&'static str),
    #[error("invalid Tid: {0}")]
    BadTid(String),
}

#[derive(Debug)]
pub struct FirehoseSync {
    pub commit: CommitObject,
    pub rev: Tid,
}

#[derive(Debug)]
pub struct FirehoseCommit {
    pub commit: CommitObject,
    pub data: DaslCid,
    pub prev: DaslCid,
    pub rev: Tid,
    pub ops: Vec<Op>,
    pub since: Option<Tid>,
    pub blocks: HashMap<DaslCid, Bytes>,
}

impl FirehoseSync {
    /// Apply firehose sync event valiation, which isn't in the spec (yet?)
    ///
    /// The only notable thing missing from the spec is asserting a size limit
    /// on the CAR slice. Since the CAR is supposed to only contain a Commit
    /// object, it would be very unusual for it to be larger than a few hundred
    /// bytes, but we allow up to the firehose commit CAR limit.
    ///
    /// We also parse out the commit object, rejecting early if it's not valid.
    pub async fn prevalidate(s: JacquardSync) -> Result<Self, FirehoseValidationError> {
        if s.blocks.len() > CAR_SIZE_LIMIT {
            return Err(FirehoseValidationError::CarTooBig {
                limit: CAR_SIZE_LIMIT,
                got: s.blocks.len(),
            });
        }

        let rs_commit = repo_stream::DriverBuilder::new()
            .load_commit(&*s.blocks)
            .await
            .map_err(|e| FirehoseValidationError::BadCommitBlock(e.to_string()))?;
        let commit: CommitObject = (&rs_commit).try_into()?;

        // jacquard makes this a string, by mistake i think
        let rev = s
            .rev
            .parse::<Tid>()
            .map_err(|e| FirehoseValidationError::BadTid(e.to_string()))?;

        // bonus checks (not described in spec even for commit events)

        // - make sure the commit object's DID matches the event's
        if commit.did != (&s.did).into() {
            return Err(FirehoseValidationError::DidMismatch {
                claimed: s.did.to_string(),
                in_commit: commit.did.to_string(),
            });
        }
        // - and that the commit object's rev matches the event's
        if commit.rev != rev {
            return Err(FirehoseValidationError::TidMismatch {
                claimed: s.rev.to_string(),
                in_commit: commit.rev.to_string(),
            });
        }

        Ok(Self { commit, rev })
    }
}

impl FirehoseCommit {
    /// Apply firehose commit validation steps 1–3
    ///
    /// https://www.ietf.org/archive/id/draft-holmgren-at-synchronization-00.html#name-commit-validation
    ///
    /// These steps can be statelessly checked to reject garbage running any i/o
    /// like identity resolution or state loading on actor wake.
    ///
    /// steps 4, 5, 6. stateful validations continue in the repo actor, see
    /// [`crate::repo_actor::task_processor::TaskProcessor::process_commit`].
    pub async fn prevalidate(c: JacquardCommit) -> Result<Self, FirehoseValidationError> {
        // TODO: non-sync1.1 handling (+put a flag to say this is sync1.1)

        // 1. verify wire-level fields. jacquard catches some of this, but we
        // add size-limit checks.

        // the maximum car size allowed is 2MB
        if c.blocks.len() > CAR_SIZE_LIMIT {
            return Err(FirehoseValidationError::CarTooBig {
                limit: CAR_SIZE_LIMIT,
                got: c.blocks.len(),
            });
        }

        // 2. parse repo diff car and mst blocks, required records present

        // (keep an eye on the time this takes bc all commits serialize through)
        let invert_start = Instant::now();

        // jacquard's parse only verifies the actual raw car structure
        let ParsedCar { root, blocks } = parse_car_bytes(&c.blocks).await?;

        // aditional integrity checks: assert size, CIDs against block bytes
        for (cid, block) in &blocks {
            // max record size over firehose is 1MB
            if block.len() > RECORD_SIZE_LIMIT {
                return Err(FirehoseValidationError::RecordTooBig {
                    limit: RECORD_SIZE_LIMIT,
                    got: block.len(),
                });
            }
            // jacquard doesn't assert CIDs against actual block bytes (and
            // iroh-car underneath does not either)
            verify_block_cid(cid, block)?;
        }

        // parse the Commit object to ensure its structure is valid.
        // NOTE: we're not currently validating MST node structures. they should
        // surface in step 3.
        let commit_block = blocks
            .get(&root)
            .ok_or(FirehoseValidationError::MissingCommitBlock(
                root.to_string(),
            ))?;
        // this is a bit silly -- we should derive Deserialize on our CommitObject
        let rs_commit = drisl::from_slice::<RSCommit>(commit_block)
            .map_err(|e| FirehoseValidationError::BadCommitBlock(e.to_string()))?;
        let commit: CommitObject = (&rs_commit).try_into()?;

        // make sure all record blocks for created/updated ops are present
        for op in c
            .ops
            .iter()
            .filter(|o| matches!(o.action, RepoOpAction::Create | RepoOpAction::Update))
        {
            let link = op
                .cid
                .as_ref()
                .ok_or(FirehoseValidationError::MissingRecordLink {
                    action: op.action.clone(),
                    path: op.path.to_string(),
                })?
                .to_ipld()
                .map_err(|_| FirehoseValidationError::BadCid("#commit.ops[].cid"))?;
            if !blocks.contains_key(&link) {
                return Err(FirehoseValidationError::MissingRecordBlock {
                    action: op.action.clone(),
                    cid: format!("{:?}", link),
                });
            }
        }

        // 2.5 bonus checks:
        // - avoid confusion between CAR root link and firehose-commit event's
        //   `commit` link: they must be the same.
        let commit_link = c
            .commit
            .to_ipld()
            .map_err(|_| FirehoseValidationError::BadCid("#commit.commit"))?;
        if root != commit_link {
            return Err(FirehoseValidationError::CidMismatch(Box::new(
                CidMismatch {
                    claimed: commit_link,
                    computed: root,
                },
            )));
        }
        // - make sure the commit object's DID matches the event's
        if commit.did != (&c.repo).into() {
            return Err(FirehoseValidationError::DidMismatch {
                claimed: c.repo.to_string(),
                in_commit: commit.did.to_string(),
            });
        }
        // - and that the commit object's rev matches the event's
        if commit.rev != c.rev.clone().into() {
            return Err(FirehoseValidationError::TidMismatch {
                claimed: c.rev.to_string(),
                in_commit: commit.rev.to_string(),
            });
        }

        // 3. apply operation inversion, matching against claimed `prevData`

        // claimed prevData setup
        let claimed_prev_data = c
            .prev_data
            .as_ref()
            .ok_or(FirehoseValidationError::MissingPrevData)?
            .to_ipld()
            .map_err(|_| FirehoseValidationError::BadCid("#commit.prevData"))?;

        // stash a hubble-sync style copy of the map for later
        let view_blocks: HashMap<DaslCid, Bytes> = blocks
            .iter()
            .map(|(ipld, b)| Ok((to_dasl_cid(*ipld, "blocks")?, b.clone())))
            .collect::<Result<_, FirehoseValidationError>>()?;

        // for now we use jacquard's memory store impl (CAR is size-bounded)
        let store = Arc::new(MemoryBlockStore::new_from_blocks(blocks));
        let mut mst = Mst::load(store, commit.data_ipld(), None);

        // stash for hubble-sync ops for the commit view later
        let mut view_ops = Vec::with_capacity(c.ops.len());

        for op in c.ops.iter() {
            let invertible = to_invertible_op(op)?;

            // we convert from VerifiedWriteOps bc we can do that infallibly
            let view_op = Op::try_from(&invertible)?;

            if !mst.invert_op(invertible).await? {
                return Err(FirehoseValidationError::FailedToInvert {
                    action: op.action.clone(),
                    path: op.path.to_string(),
                });
            }

            view_ops.push(view_op);
        }
        let computed_prev_root = mst.get_pointer().await?;
        if computed_prev_root != claimed_prev_data {
            return Err(FirehoseValidationError::PrevDataMismatch {
                claimed_prev_data: format!("{:?}", claimed_prev_data),
                computed: format!("{}", computed_prev_root),
            });
        }

        histogram!(COMMIT_PREVALIDATE_SECONDS).record(invert_start.elapsed().as_secs_f64());

        // 4, 5, 6. stateful validations continue in the repo actor, see
        // [`crate::repo_actor::task_processor`].

        let data = commit.data;

        let prev = to_dasl_cid(claimed_prev_data, "prevData")?;

        Ok(Self {
            commit,
            data,
            prev,
            rev: c.rev.into(),
            ops: view_ops,
            since: c.since.map(Into::into),
            blocks: view_blocks,
        })
    }
}

fn to_dasl_cid(ipld: IpldCid, source: &'static str) -> Result<DaslCid, FirehoseValidationError> {
    DaslCid::from_bytes_raw(&ipld.to_bytes()).map_err(|_| FirehoseValidationError::BadCid(source))
}

impl TryFrom<&VerifiedWriteOp> for Op {
    type Error = FirehoseValidationError;
    fn try_from(r: &VerifiedWriteOp) -> Result<Op, Self::Error> {
        Ok(match r {
            VerifiedWriteOp::Create { key, cid } => Op {
                path: key.as_str().into(),
                kind: OpKind::Create {
                    cid: to_dasl_cid(*cid, "ops")?,
                },
            },
            VerifiedWriteOp::Update { key, cid, prev } => Op {
                path: key.as_str().into(),
                kind: OpKind::Update {
                    cid: to_dasl_cid(*cid, "ops")?,
                    prev: to_dasl_cid(*prev, "ops")?,
                },
            },
            VerifiedWriteOp::Delete { key, prev } => Op {
                path: key.as_str().into(),
                kind: OpKind::Delete {
                    prev: to_dasl_cid(*prev, "ops")?,
                },
            },
        })
    }
}
