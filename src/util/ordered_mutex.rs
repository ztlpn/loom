use crate::rt;

use std::sync::atomic::Ordering;

/// Estabish mutual exclusion with a specific memory ordering.
#[derive(Debug)]
pub struct OrderedMutex {
    object: rt::Mutex,
}

impl OrderedMutex {
    /// Creates a new mutex in an unlocked state ready for use.
    pub fn new(ordering: Ordering) -> OrderedMutex {
        OrderedMutex {
            object: rt::Mutex::new(ordering),
        }
    }
}

impl OrderedMutex {
    /// Acquires a mutex, blocking the current thread until it is able to do so.
    pub fn lock(&self) {
        self.object.acquire_lock();
    }

    /// Attempts to acquire this lock.
    ///
    /// Returns `true` if the lock is acquired.
    pub fn try_lock(&self) -> bool {
        self.object.try_acquire_lock()
    }

    /// Release the lock.
    pub fn release(&self) {
        self.object.release_lock();
    }
}
