mod connection;
mod manager;
mod types;
mod worker;

pub use manager::{Manager, ManagerError};
pub use types::{SubscribeRepos, SubscribeReposSender};
