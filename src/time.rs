//! Temporal quantification

use core::{convert::TryFrom, ops, time::Duration};
use nc::timespec_t;
use std::cmp::Ordering;

/// A measurement of a monotonically nondecreasing clock. Opaque and useful only with `Duration`
#[derive(Clone, Copy)]
pub struct Instant {
    ts: timespec_t,
}

impl PartialEq for Instant {
    fn eq(&self, other: &Self) -> bool {
        self.ts.tv_sec == other.ts.tv_sec && self.ts.tv_nsec == other.ts.tv_nsec
    }
}

impl Eq for Instant {}

impl PartialOrd for Instant {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.ts.tv_sec < other.ts.tv_sec {
            Some(Ordering::Less)
        } else if self.ts.tv_sec > other.ts.tv_sec {
            Some(Ordering::Greater)
        } else {
            if self.ts.tv_nsec < other.ts.tv_nsec {
                Some(Ordering::Less)
            } else if self.ts.tv_nsec > other.ts.tv_nsec {
                Some(Ordering::Greater)
            } else {
                Some(Ordering::Equal)
            }
        }
    }
}

impl Ord for Instant {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

fn clock_gettime(clk_id: nc::clockid_t) -> timespec_t {
    let mut ts = timespec_t::default();
    nc::clock_gettime(clk_id, &mut ts).expect("Failed to get time");
    ts
}

impl Instant {
    /// Returns an instant corresponding to "now".
    pub fn now() -> Self {
        Self {
            ts: clock_gettime(nc::CLOCK_MONOTONIC),
        }
    }

    /// Returns `Some(t)` where t is the time `self + duration` if t can be represented as `Instant`
    /// (which means it's inside the bounds of the underlying data structure), `None` otherwise.
    pub fn checked_add(&self, dur: Duration) -> Option<Instant> {
        const NANOS_IN_ONE_SEC: isize = 1_000_000_000;

        let mut secs = self
            .ts
            .tv_sec
            .checked_add(isize::try_from(dur.as_secs()).ok()?)?;
        let mut nanos = self.ts.tv_nsec.wrapping_add(dur.subsec_nanos() as isize);

        if nanos > NANOS_IN_ONE_SEC {
            nanos -= NANOS_IN_ONE_SEC;
            secs = secs.checked_add(1)?;
        }

        Some(Instant {
            ts: timespec_t {
                tv_sec: secs,
                tv_nsec: nanos,
            },
        })
    }

    /// Returns the amount of time elapsed from another instant to this one, or `None` if that
    /// instant is earlier than this one.
    pub fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        if self < &earlier {
            None
        } else {
            let (sec, nsec) = if self.ts.tv_nsec >= earlier.ts.tv_nsec {
                (
                    self.ts.tv_sec - earlier.ts.tv_sec,
                    self.ts.tv_nsec - earlier.ts.tv_nsec,
                )
            } else {
                (
                    self.ts.tv_sec - 1 - earlier.ts.tv_sec,
                    self.ts.tv_nsec + 1_000_000_000 - earlier.ts.tv_nsec,
                )
            };

            // NOTE `nsec` is always less than `1_000_000_000`
            // NOTE `sec` is always positive
            Some(Duration::new(sec as u64, nsec as u32))
        }
    }

    /// Returns the amount of time elapsed from another instant to this one, or zero duration if
    /// that instant is earlier than this one.
    pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
        self.checked_duration_since(earlier)
            .unwrap_or(Duration::new(0, 0))
    }
}

impl ops::Add<Duration> for Instant {
    type Output = Self;

    fn add(self, dur: Duration) -> Self {
        self.checked_add(dur).unwrap()
    }
}

impl From<Instant> for timespec_t {
    fn from(i: Instant) -> timespec_t {
        i.ts
    }
}
