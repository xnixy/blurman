//! When a panel tries again to make its backdrop after an attempt made none. Pure (the clock is
//! passed in), so it is tested without a desktop.
//!
//! Two reasons an attempt makes nothing:
//! - the thread's rate bucket (`guard::Bucket`) has no token left: the panel waits until it has
//!   one and tries again, for up to [`WAIT_FOR_RATE`] (so a refresh that came during a burst of
//!   attaches is not lost, and a panel whose first attach was refused still gets glass), never
//!   faster than the bucket allows;
//! - Windows refused: tried again [`Retry::MAX_FAILURES`] times, [`super::REBUILD_GAP`] apart.
//!
//! A new rebuild asked for starts the count over, so one unlucky refresh does not leave a panel
//! with a single attempt on every later one.

use std::time::{Duration, Instant};

/// The longest a panel keeps waiting for the rate bucket.
pub const WAIT_FOR_RATE: Duration = Duration::from_secs(10);
/// The least time between two attempts while waiting for the bucket (never a busy loop).
pub const MIN_WAIT: Duration = Duration::from_millis(100);

/// What an attempt to make a backdrop came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Made,
    /// The rate bucket had no token; it will have one in this long.
    RateLimited(Duration),
    /// Windows would not make it.
    Failed,
}

/// What to do next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    /// Nothing: it was made.
    Done,
    /// Try again after this long.
    RetryIn(Duration),
    /// Stop until a rebuild is asked for again.
    GiveUp,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Retry {
    failures: u8,
    /// When the first attempt refused for the rate (since the last ask) was.
    waiting_since: Option<Instant>,
}

impl Retry {
    pub const MAX_FAILURES: u8 = 3;

    /// A new rebuild was asked for: the count starts over.
    pub fn asked(&mut self) {
        *self = Self::default();
    }

    /// The attempt at `now` came to `outcome`.
    pub fn after(&mut self, outcome: Outcome, now: Instant, gap: Duration) -> Next {
        match outcome {
            Outcome::Made => {
                *self = Self::default();
                Next::Done
            }
            Outcome::Failed => {
                self.failures = self.failures.saturating_add(1);
                if self.failures <= Self::MAX_FAILURES {
                    Next::RetryIn(gap)
                } else {
                    Next::GiveUp
                }
            }
            Outcome::RateLimited(ready_in) => {
                let since = *self.waiting_since.get_or_insert(now);
                if now.saturating_duration_since(since) >= WAIT_FOR_RATE {
                    Next::GiveUp
                } else {
                    Next::RetryIn(ready_in.max(MIN_WAIT))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::panel::guard::Bucket;

    const GAP: Duration = Duration::from_millis(500);

    #[test]
    fn a_refusal_by_windows_is_tried_again_a_few_times() {
        let t = Instant::now();
        let mut r = Retry::default();
        for _ in 0..Retry::MAX_FAILURES {
            assert_eq!(r.after(Outcome::Failed, t, GAP), Next::RetryIn(GAP));
        }
        assert_eq!(r.after(Outcome::Failed, t, GAP), Next::GiveUp);
        // A later refresh gets every attempt again, not one.
        r.asked();
        for _ in 0..Retry::MAX_FAILURES {
            assert_eq!(r.after(Outcome::Failed, t, GAP), Next::RetryIn(GAP));
        }
    }

    #[test]
    fn success_starts_the_count_over() {
        let t = Instant::now();
        let mut r = Retry::default();
        r.after(Outcome::Failed, t, GAP);
        r.after(Outcome::Failed, t, GAP);
        assert_eq!(r.after(Outcome::Made, t, GAP), Next::Done);
        for _ in 0..Retry::MAX_FAILURES {
            assert_eq!(r.after(Outcome::Failed, t, GAP), Next::RetryIn(GAP));
        }
    }

    /// A panel against an empty bucket (a fake clock): it waits for a token, as often as the
    /// bucket refills and never faster, takes one as soon as there is one, and stops after 10 s.
    #[test]
    fn an_empty_bucket_is_waited_for_not_given_up_on() {
        let t0 = Instant::now();
        let mut bucket = Bucket::default();
        while bucket.take(t0) {}
        let mut r = Retry::default();
        let mut now = t0;
        let mut attempts = 0;
        loop {
            attempts += 1;
            let outcome = if bucket.take(now) {
                Outcome::Made
            } else {
                Outcome::RateLimited(bucket.ready_in(now))
            };
            match r.after(outcome, now, GAP) {
                Next::Done => break,
                Next::RetryIn(d) => {
                    assert!(d >= MIN_WAIT, "no busy loop");
                    now += d;
                }
                Next::GiveUp => panic!("gave up while the bucket refills"),
            }
        }
        // A token comes back in a third of a second: the first retry gets it.
        assert_eq!(attempts, 2);
        assert!(now - t0 <= Duration::from_millis(400));

        // Another caller keeps taking every token as it comes: the panel waits 10 s, then stops.
        let mut r = Retry::default();
        let start = now;
        let mut tries = 0;
        let gave_up = loop {
            tries += 1;
            let ready = bucket.ready_in(now);
            match r.after(Outcome::RateLimited(ready), now, GAP) {
                Next::RetryIn(d) => now += d,
                Next::GiveUp => break now,
                Next::Done => unreachable!(),
            }
            assert!(tries < 1000, "bounded");
        };
        assert!(
            gave_up - start >= WAIT_FOR_RATE
                && gave_up - start < WAIT_FOR_RATE + Duration::from_secs(1)
        );
        // At most one attempt per MIN_WAIT.
        assert!(tries as u128 <= WAIT_FOR_RATE.as_millis() / MIN_WAIT.as_millis() + 2);
    }
}
