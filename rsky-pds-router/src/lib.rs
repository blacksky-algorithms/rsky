//! A stateless router in front of two PDS implementations that share one
//! data directory: reads go to whichever implementation the policy pins
//! for the account, mutations go to the single writer of the account, and
//! every forwarded mutation is journaled before its first byte leaves.

pub mod classify;
pub mod config;
pub mod inventory;
pub mod journal;
pub mod lookup;
pub mod metrics;
pub mod policy;
pub mod proxy;
pub mod server;
