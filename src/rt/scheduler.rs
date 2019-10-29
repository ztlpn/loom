#![allow(deprecated)]

use crate::rt::{thread, Execution};

use context::{Context, Transfer};
use context::stack::ProtectedFixedSizeStack;
use scoped_tls::scoped_thread_local;
use std::cell::RefCell;
use std::fmt;

pub(crate) struct Scheduler {
    /// Threads
    threads: Vec<Thread>,

    // Pending functions
    pending: Vec<Option<Box<dyn FnOnce()>>>,

    next_thread: usize,
}

// type Thread = Generator<'static, Option<Box<dyn FnOnce()>>, ()>;
struct Thread {
    _stack: ProtectedFixedSizeStack,
    transfer: Option<Transfer>,
}

scoped_thread_local! {
    static STATE: RefCell<State<'_>>
}

struct State<'a> {
    // Execution
    execution: &'a mut Execution,

    // queued_spawn: &'a mut VecDeque<Box<dyn FnOnce()>>,
    transfer: Option<Transfer>,

    pending: &'a mut [Option<Box<dyn FnOnce()>>],

    // Next thread
    next_thread: &'a mut usize,
}

impl Scheduler {
    /// Create an execution
    pub(crate) fn new(capacity: usize) -> Scheduler {
        let threads = spawn_threads(capacity);
        let pending = (0..capacity).map(|_| None).collect();

        Scheduler {
            threads,
            pending,
            next_thread: 1,
        }
    }

    /// Access the execution
    pub(crate) fn with_execution<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Execution) -> R,
    {
        STATE.with(|state| f(&mut state.borrow_mut().execution))
    }

    /// Perform a context switch
    pub(crate) fn switch() {
        let transfer = STATE.with(|state| {
            state.borrow_mut().transfer.take().unwrap()
        });

        let transfer = unsafe { transfer.context.resume(0) };

        STATE.with(|state| {
            state.borrow_mut().transfer = Some(transfer);
        });
    }

    pub(crate) fn spawn(f: Box<dyn FnOnce()>) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();

            let thread_id = *state.next_thread;
            *state.next_thread += 1;

            state.pending[thread_id] = Some(f);
        });
    }

    pub(crate) fn run<F>(&mut self, execution: &mut Execution, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.pending[0] = Some(Box::new(f));
        self.next_thread = 1;

        loop {
            if !execution.threads.is_active() {
                return;
            }

            let active = execution.threads.active_id();
            self.tick(active, execution);
        }
    }

    fn tick(&mut self, thread: thread::Id, execution: &mut Execution) {
        let state = RefCell::new(State {
            execution: execution,
            transfer: None,
            pending: &mut self.pending,
            next_thread: &mut self.next_thread,
        });

        let threads = &mut self.threads;

        STATE.set(unsafe { transmute_lt(&state) }, || {
            let thread_id = thread.as_usize();

            let transfer = threads[thread_id].transfer.take().unwrap();
            let transfer = unsafe { transfer.context.resume(thread_id) };

            if transfer.data == 1 {
                panic!();
            }

            threads[thread_id].transfer = Some(transfer);
        });
    }
}

impl fmt::Debug for Scheduler {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Schedule")
            .finish()
    }
}

fn spawn_threads(n: usize) -> Vec<Thread> {
    use std::panic;

    extern "C" fn context_fn(mut t: Transfer) -> ! {
        let thread_id = t.data;

        loop {
            let f = STATE.with(|state| {
                let mut state = state.borrow_mut();

                state.transfer = Some(t);
                state.pending[thread_id].take().unwrap()
            });

            let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                f();
            }));

            t = STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.transfer.take().unwrap()
            });

            if res.is_err() {
                unsafe { t.context.resume(1) };
                break;
            } else {
                t = unsafe { t.context.resume(0) };
            }
        }

        unreachable!();
    }

    (0..n)
        .map(|i| unsafe {
            let stack = ProtectedFixedSizeStack::default();
            let transfer = Transfer::new(Context::new(&stack, context_fn), i);

            Thread {
                _stack: stack,
                transfer: Some(transfer),
            }
        })
        .collect()
}

unsafe fn transmute_lt<'a, 'b>(state: &'a RefCell<State<'b>>) -> &'a RefCell<State<'static>> {
    ::std::mem::transmute(state)
}
