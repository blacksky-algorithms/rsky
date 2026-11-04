use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(50);

/// Runs `on_expire` once `deadline` elapses unless `done` is set first.
/// Returns whether `on_expire` ran.
pub fn spawn_deadline<F>(
    deadline: Duration,
    done: Arc<AtomicBool>,
    on_expire: F,
) -> JoinHandle<bool>
where
    F: FnOnce() + Send + 'static,
{
    thread::spawn(move || {
        let start = Instant::now();
        loop {
            if done.load(Ordering::Relaxed) {
                return false;
            }
            if start.elapsed() >= deadline {
                on_expire();
                return true;
            }
            thread::sleep(POLL);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setter(flag: &Arc<AtomicBool>) -> impl FnOnce() + Send + 'static {
        let flag = Arc::clone(flag);
        move || assert!(!flag.swap(true, Ordering::SeqCst))
    }

    #[test]
    fn completing_before_the_deadline_skips_the_callback() {
        let done = Arc::new(AtomicBool::new(true));
        let fired = Arc::new(AtomicBool::new(false));
        let handle = spawn_deadline(Duration::from_secs(5), done, setter(&fired));
        assert!(!handle.join().unwrap());
        assert!(!fired.load(Ordering::SeqCst));
    }

    #[test]
    fn missing_the_deadline_runs_the_callback_once() {
        let done = Arc::new(AtomicBool::new(false));
        let fired = Arc::new(AtomicBool::new(false));
        let handle = spawn_deadline(Duration::from_millis(100), done, setter(&fired));
        assert!(handle.join().unwrap());
        assert!(fired.load(Ordering::SeqCst));
    }
}
