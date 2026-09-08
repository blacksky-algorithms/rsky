use crate::commit::CommitObject;
use crate::identity::{SignatureError, SigningKey};

/// Apply remaining commit event validations
///
/// We are picking up at step 4:
/// https://www.ietf.org/archive/id/draft-holmgren-at-synchronization-00.html#section-4.5-3.4.1
///
/// See [`crate::firehose::event_validation::FirehoseCommit::prevalidate`] for
/// validation steps 1–3.
pub fn validate_commit_signature(
    key: &SigningKey,
    commit: &CommitObject,
) -> Result<(), SignatureError> {
    // 4. verify the commit signature
    commit.verify_signature(key)
}
