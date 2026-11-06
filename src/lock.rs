use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

pub struct Lock(File);

impl Lock {
    pub fn acquire(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn second_acquire_waits_for_first_to_drop() {
        let path = std::env::temp_dir().join(format!("herdr-tabshell-{}.lock", std::process::id()));
        let first = Lock::acquire(&path).unwrap();
        let (sender, receiver) = mpsc::channel();
        let (starting, started) = mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            starting.send(()).unwrap();
            let _second = Lock::acquire(worker_path).unwrap();
            sender.send(()).unwrap();
        });

        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        let _ = std::fs::remove_file(path);
    }
}
