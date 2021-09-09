use core::{
    cell::Cell,
    ops::Range,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
use std::mem::size_of;

use heapless::spsc::SingleCore;
pub use heapless::{
    consts,
    i::{BinaryHeap as iBinaryHeap, Queue as iQueue},
    spsc::Queue,
    BinaryHeap,
};
pub use nc::{exit, getpid, pid_t, sched_yield, siginfo_t, timer_t, SI_QUEUE};
use nc::{
    mmap, rt_sigaction, rt_sigprocmask, sched_param_t, sched_setaffinity, sched_setscheduler,
    sigaction_t, sigev_un_t, sigevent_t, sighandler_t, sigset_t, sigval_t, SCHED_FIFO, SIGRTMIN,
    SIG_BLOCK,
};

pub use crate::tq::{NotReady, TimerQueue};

pub struct Barrier {
    inner: AtomicBool,
}

impl Barrier {
    pub const fn new() -> Self {
        Self {
            inner: AtomicBool::new(false),
        }
    }

    pub fn release(&self) {
        self.inner.store(true, Ordering::Release)
    }

    pub fn wait(&self) {
        while !self.inner.load(Ordering::Acquire) {}
    }
}

pub struct Pid {
    inner: AtomicI32,
}

impl Pid {
    pub const fn uninit() -> Self {
        Self {
            inner: AtomicI32::new(0),
        }
    }

    pub fn get(&self) -> pid_t {
        self.inner.load(Ordering::Relaxed)
    }

    pub fn init(&self, pid: pid_t) {
        self.inner.store(pid, Ordering::Relaxed)
    }

    pub fn wait(&self) -> pid_t {
        loop {
            let pid = self.inner.load(Ordering::Relaxed);

            if pid == 0 {
                sched_yield().expect("Yield failed");
            } else {
                break pid;
            }
        }
    }
}

pub struct Timer {
    inner: AtomicI32,
}

impl Timer {
    pub const fn uninit() -> Self {
        Self {
            inner: AtomicI32::new(0),
        }
    }

    pub fn get(&self) -> timer_t {
        self.inner.load(Ordering::Relaxed)
    }

    pub fn init(&self, timer: timer_t) {
        self.inner.store(timer, Ordering::Relaxed)
    }
}

pub type FreeQueue<N> = Queue<u8, N, u8, SingleCore>;

// The PID `0` represents the current process
const OURSELVES: pid_t = 0;

pub unsafe fn init_runtime(signo_max: Option<u8>) {
    // NOTE all threads spawned (`sys_clone`) from this one will inherit these settings

    // start by running all threads on a single core
    set_affinity(OURSELVES, 0);

    // raise the priority to the minimal real-time priority
    sched_setscheduler(OURSELVES, SCHED_FIFO, &sched_param_t { sched_priority: 1 }).expect(
        "error: couldn't change scheduling policy; \
    run `sudo setcap cap_sys_nice+ep $binary` first",
    );

    // block all the used real-time signals; this is equivalent to `interrupt::disable`
    if let Some(signo) = signo_max {
        let mask = ((1 << (signo + 1)) - 1) << (SIGRTMIN - 1);
        let mask = sigset_t { sig: [mask] };
        rt_sigprocmask(
            SIG_BLOCK,
            &mask,
            &mut sigset_t::default(),
            size_of::<sigset_t>(),
        )
        .expect("error: couldn't change the signal mask");
    }
}

pub unsafe fn spawn(_child: extern "C" fn() -> !) -> pid_t {
    const PAGE_SIZE: usize = 4 * 1024; // 4 KiB (output of `getconf PAGESIZE`)
    const STACK_SIZE: usize = 2 * 1024 * PAGE_SIZE; // 8 MiB (output of `ulimit -s`)

    let stack_low = mmap(
        0,          // address; 0 means any page-aligned address
        STACK_SIZE, // length of mapping
        nc::PROT_READ | // read access
        nc::PROT_WRITE, // write access
        nc::MAP_ANONYMOUS | // mapping is not backed by any file
        nc::MAP_PRIVATE | // mapping is private to other threads / processes
        nc::MAP_GROWSDOWN, // mapping suitable for stacks
        -1,         // file descriptor; needs to be `-1` because of MAP_ANONYMOUS
        0,          // offset; ignored because of MAP_ANONYMOUS
    )
    .expect("MMAP failed in process spawn");

    let stack_high = stack_low + STACK_SIZE;

    // spin a new thread
    nc::clone(
        nc::CLONE_VM | // new thread shares memory with the parent
        nc::CLONE_THREAD | // share thread group
        nc::CLONE_SIGHAND, // shared signal handlers; required by `CLONE_THREAD`
        stack_high,
        &mut 0,
        &mut 0,
        0,
    )
    .expect("Process clone failed")
}

pub unsafe fn set_affinity(tid: pid_t, core: u8) {
    sched_setaffinity(tid, 1, &[1 << core]).expect("error: couldn't change CPU affinity");
}

pub unsafe fn timer_create(tid: Option<pid_t>, signo: u8) -> timer_t {
    let (sigev_notify, sigev_un) = if let Some(tid) = tid {
        // multi-core application
        (nc::SIGEV_THREAD_ID, sigev_un_t { tid })
    } else {
        // single-core application
        (nc::SIGEV_SIGNAL, sigev_un_t::default())
    };

    let mut tid = 0;
    nc::timer_create(
        nc::CLOCK_MONOTONIC,
        Some(&mut sigevent_t {
            sigev_value: sigval_t { sival_int: 0 },
            sigev_signo: SIGRTMIN + i32::from(signo),
            sigev_notify,
            sigev_un,
        }),
        &mut tid,
    )
    .expect("error: couldn't create a timer");

    tid
}

pub unsafe fn lock<T, R>(
    ptr: *mut T,
    priority: &Priority,
    ceiling: u8,
    range: Range<u8>,
    f: impl FnOnce(&mut T) -> R,
) -> R {
    let current = priority.get();

    if current < ceiling {
        priority.set(ceiling);
        mask(range.clone(), current, ceiling, true);
        let r = f(&mut *ptr);
        mask(range, current, ceiling, false);
        priority.set(current);
        r
    } else {
        f(&mut *ptr)
    }
}

pub unsafe fn mask(Range { start, end }: Range<u8>, current: u8, ceiling: u8, block: bool) {
    let len = end.wrapping_sub(start);
    let mask =
        ((1 << (ceiling - current)) - 1) << (SIGRTMIN - 1 + i32::from(start + len - ceiling));
    let mask = sigset_t { sig: [mask] };
    rt_sigprocmask(
        if block {
            nc::SIG_BLOCK
        } else {
            nc::SIG_UNBLOCK
        },
        &mask,
        &mut sigset_t::default(),
        size_of::<sigset_t>(),
    )
    .expect("error: couldn't change the signal mask");
}

pub unsafe fn enqueue(tgid: i32, tid: Option<i32>, signo: u8, task: u8, index: u8) {
    let mut si = siginfo_t::default();
    si.siginfo.si_code = nc::SI_QUEUE;
    si.siginfo.sifields.rt.sigval.sival_ptr = (usize::from(task) << 8) + usize::from(index);

    if let Some(tid) = tid {
        nc::rt_tgsigqueueinfo(tgid, tid, SIGRTMIN + i32::from(signo), &mut si)
            .expect("error: couldn't enqueue signal\n");
    } else {
        nc::rt_sigqueueinfo(tgid, SIGRTMIN + i32::from(signo), &mut si)
            .expect("error: couldn't enqueue signal\n");
    }
}

pub unsafe fn register(
    Range { start, end }: Range<u8>,
    priority: u8,
    sigaction: extern "C" fn(i32, &mut siginfo_t, usize),
) {
    extern "C" {
        fn __restorer() -> !;
    }

    let len = end.wrapping_sub(start);
    let mask = (1 << len) - 1;
    let mask = (mask ^ (mask >> (priority - 1))) << (i32::from(start) + SIGRTMIN - 1);
    let mask = sigset_t { sig: [mask] };

    rt_sigaction(
        SIGRTMIN + i32::from(end.wrapping_sub(priority)),
        &sigaction_t {
            sa_handler: sigaction as sighandler_t,
            sa_flags: nc::SA_SIGINFO,
            sa_mask: mask,
        },
        &mut sigaction_t::default(),
        size_of::<sigset_t>(),
    )
    .expect("error: couldn't register signal handler");
}

// Newtype over `Cell` that forbids mutation through a shared reference
pub struct Priority {
    inner: Cell<u8>,
}

impl Priority {
    #[inline(always)]
    pub unsafe fn new(value: u8) -> Self {
        Priority {
            inner: Cell::new(value),
        }
    }

    // these two methods are used by `lock` (see below) but can't be used from the RTFM application
    #[inline(always)]
    fn set(&self, value: u8) {
        self.inner.set(value)
    }

    #[inline(always)]
    fn get(&self) -> u8 {
        self.inner.get()
    }
}

pub fn pause() {
    // ppoll(0, 0, 0, 0) in C.
    #[cfg(target_arch = "aarch64")]
    nc::rt_sigsuspend(&mut sigset_t { sig: [255] }, size_of::<sigset_t>()).ok();

    #[cfg(not(target_arch = "aarch64"))]
    nc::pause().expect("pause failed");
}

pub fn assert_send<T>()
where
    T: Send,
{
}

pub fn assert_sync<T>()
where
    T: Sync,
{
}
