//! Release advisory locks explicitly, including fork-inherited descriptor copies.
use std::fs::{File, TryLockError};

pub(crate) struct LockGuard(File);

impl LockGuard {
    pub(crate) fn acquire(file: File) -> Result<Self, TryLockError> {
        file.try_lock()?;
        Ok(Self(file))
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // A different thread can fork while this descriptor is open. Closing
        // just our copy can leave its lock alive until the child execs/exits.
        // Explicit unlock releases it at the end of the protected operation.
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_copy_cannot_keep_a_finished_operation_locked() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("lock");
        let file = File::create(&path).unwrap();
        let inherited = file.try_clone().unwrap();
        let guard = LockGuard::acquire(file).unwrap();
        assert!(LockGuard::acquire(File::open(&path).unwrap()).is_err());
        drop(guard);
        let next = LockGuard::acquire(File::open(&path).unwrap()).unwrap();
        drop(inherited);
        drop(next);
    }
}
