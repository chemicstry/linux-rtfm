use core::cmp::Ordering;

use crate::time::Instant;
use heapless::{binary_heap::Min, ArrayLength, BinaryHeap};
use nc::{itimerspec_t, pid_t, timer_t, timespec_t, SIGRTMIN, TIMER_ABSTIME};

pub struct TimerQueue<T, N>(pub BinaryHeap<NotReady<T>, N, Min>)
where
    T: Copy,
    N: ArrayLength<NotReady<T>>;

impl<T, N> TimerQueue<T, N>
where
    T: Copy,
    N: ArrayLength<NotReady<T>>,
{
    pub unsafe fn enqueue_unchecked(
        &mut self,
        nr: NotReady<T>,
        tgid_tid: Option<(pid_t, pid_t)>,
        signo: u8,
    ) {
        if self
            .0
            .peek()
            .map(|head| nr.instant < head.instant)
            .unwrap_or(true)
        {
            // new entry has earlier deadline; signal the timer queue
            if let Some((tgid, tid)) = tgid_tid {
                // multi-core application
                nc::tgkill(tgid, tid, SIGRTMIN + i32::from(signo)).expect("Sending signal failed");
            } else {
                // single core application
                nc::kill(0, SIGRTMIN + i32::from(signo)).expect("Sending signal failed");
            }
        }

        self.0.push_unchecked(nr);
    }

    pub fn dequeue(&mut self, timer_id: timer_t) -> Option<(T, u8)> {
        if let Some(instant) = self.0.peek().map(|p| p.instant) {
            let now = Instant::now();
            if now >= instant {
                // task became ready
                let nr = unsafe { self.0.pop_unchecked() };

                Some((nr.task, nr.index))
            } else {
                // set a new timeout
                nc::timer_settime(
                    timer_id,
                    TIMER_ABSTIME,
                    &itimerspec_t {
                        it_interval: timespec_t {
                            tv_sec: 0,
                            tv_nsec: 0,
                        },
                        it_value: instant.into(),
                    },
                    None,
                )
                .expect("Failed to set timer");

                None
            }
        } else {
            // the queue is empty
            None
        }
    }
}

pub struct NotReady<T>
where
    T: Copy,
{
    pub index: u8,
    pub instant: Instant,
    pub task: T,
}

impl<T> Eq for NotReady<T> where T: Copy {}

impl<T> Ord for NotReady<T>
where
    T: Copy,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.instant.cmp(&other.instant)
    }
}

impl<T> PartialEq for NotReady<T>
where
    T: Copy,
{
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant
    }
}

impl<T> PartialOrd for NotReady<T>
where
    T: Copy,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}
