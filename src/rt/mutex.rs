use crate::rt::object::{self, Object};
use crate::rt::{thread, Access, Synchronize};

use std::sync::atomic::Ordering;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) struct Mutex {
    obj: Object,
}

#[derive(Debug)]
pub(super) struct State {
    /// Ordering established by the mutex. Options are:
    ///
    /// - Relaxed
    /// - AcqRel
    /// - SeqCst
    ordering: Ordering,

    /// `Some` when the mutex is in the locked state. The `thread::Id`
    /// references the thread that currently holds the mutex.
    lock: Option<thread::Id>,

    /// Tracks access to the mutex
    last_access: Option<Access>,

    /// Causality transfers between threads
    synchronize: Synchronize,
}

impl Mutex {
    pub(crate) fn new(ordering: Ordering) -> Mutex {
        match ordering {
            Ordering::Relaxed |
                Ordering::AcqRel |
                Ordering::SeqCst => {}
            _ => panic!("invalid mutex ordering; {:?}", ordering),
        }

        super::execution(|execution| {
            let obj = execution.objects.insert_mutex(State {
                ordering,
                lock: None,
                last_access: None,
                synchronize: Synchronize::new(execution.max_threads),
            });

            Mutex { obj }
        })
    }

    pub(crate) fn acquire_lock(&self) {
        self.obj.branch_acquire(self.is_locked());
        assert!(self.post_acquire(), "expected to be able to acquire lock");
    }

    pub(crate) fn try_acquire_lock(&self) -> bool {
        self.obj.branch_opaque();
        self.post_acquire()
    }

    pub(crate) fn release_lock(&self) {
        super::execution(|execution| {
            let state = self.get_state(&mut execution.objects);

            let thread_id = execution.threads.active_id();

            // Release the lock flag
            state.lock = None;

            state
                .synchronize
                .sync_store(&mut execution.threads, state.release_ordering());

            if state.is_seq_cst() {
                // Establish sequential consistency between the lock's operations.
                execution.threads.seq_cst();
            }

            for (id, thread) in execution.threads.iter_mut() {
                if id == thread_id {
                    continue;
                }

                let obj = thread
                    .operation
                    .as_ref()
                    .map(|operation| operation.object());

                if obj == Some(self.obj) {
                    thread.set_runnable();
                }
            }
        });
    }

    fn post_acquire(&self) -> bool {
        super::execution(|execution| {
            let state = self.get_state(&mut execution.objects);
            let thread_id = execution.threads.active_id();

            if state.lock.is_some() {
                return false;
            }

            // Set the lock to the current thread
            state.lock = Some(thread_id);

            state.synchronize.sync_load(&mut execution.threads, state.acquire_ordering());

            if state.is_seq_cst() {
                // Establish sequential consistency between locks
                execution.threads.seq_cst();
            }

            // Block all **other** threads attempting to acquire the mutex
            for (id, thread) in execution.threads.iter_mut() {
                if id == thread_id {
                    continue;
                }

                let obj = thread
                    .operation
                    .as_ref()
                    .map(|operation| operation.object());

                if obj == Some(self.obj) {
                    thread.set_blocked();
                }
            }

            true
        })
    }

    /// Returns `true` if the mutex is currently locked
    fn is_locked(&self) -> bool {
        super::execution(|execution| self.get_state(&mut execution.objects).lock.is_some())
    }

    fn get_state<'a>(&self, objects: &'a mut object::Store) -> &'a mut State {
        self.obj.mutex_mut(objects).unwrap()
    }
}

impl State {
    pub(crate) fn last_dependent_accesses<'a>(&'a self) -> Box<dyn Iterator<Item = &Access> + 'a> {
        Box::new(self.last_access.iter())
    }

    pub(crate) fn set_last_access(&mut self, access: Access) {
        self.last_access = Some(access);
    }

    fn is_seq_cst(&self) -> bool {
        match self.ordering {
            Ordering::SeqCst => true,
            _ => false,
        }
    }

    fn acquire_ordering(&self) -> Ordering {
        match self.ordering {
            Ordering::Relaxed => Ordering::Relaxed,
            _ => Ordering::Acquire,
        }
    }

    fn release_ordering(&self) -> Ordering {
        match self.ordering {
            Ordering::Relaxed => Ordering::Relaxed,
            _ => Ordering::Release,
        }
    }
}
