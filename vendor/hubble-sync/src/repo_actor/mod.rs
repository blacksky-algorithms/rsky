//! Repository actor, RepoState, RepoMessage

mod actor;
mod evictable_state;
mod identity_initial;
mod identity_refresh;
mod repo_registry;
mod task_processor;
mod working_gauge;

pub use actor::RepoMessage;
pub use repo_registry::{RepoRegistry, RepoSendError, RepoSender};
pub use task_processor::{ModerateOutcome, RepoContext, ResyncContext};

use actor::Task;
use evictable_state::EvictableState;
use identity_initial::{InitialResolve, InitialResolveOutcome};
use identity_refresh::{IdentityRefresh, IdentityRefreshOutcome};
use task_processor::{ProcessError, TaskProcessor};
use working_gauge::WorkingGauge;
