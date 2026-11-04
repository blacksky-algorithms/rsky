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

/// Lifts the soft open-file limit to the hard one. A container's default
/// soft limit is 1024, and every proxied request holds a client socket and
/// an upstream socket, so a few hundred concurrent requests exhaust it and
/// every new upstream connect fails with "Too many open files".
pub fn raise_open_file_limit() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is plain data that outlives both calls.
    let read = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } == 0;
    let before = limit.rlim_cur;
    limit.rlim_cur = limit.rlim_max;
    let raised = read && unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } == 0;
    tracing::info!(before, hard = limit.rlim_max, raised, "open file limit");
}

#[cfg(test)]
mod open_file_limit_tests {
    use super::raise_open_file_limit;

    fn current() -> libc::rlimit {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is plain data that outlives the call.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0
        );
        limit
    }

    #[test]
    fn the_soft_open_file_limit_is_lifted_to_the_hard_one() {
        let before = current();
        raise_open_file_limit();
        let after = current();
        assert!(after.rlim_cur >= before.rlim_cur);
        assert_eq!(after.rlim_max, before.rlim_max);
    }
}
