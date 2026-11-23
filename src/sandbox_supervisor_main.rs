//! Standalone, single-threaded Linux invocation owner. Never linked into the
//! application's process: waitpid(-1) here can only reap this invocation's tree.
use std::{
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::FileExt, process::ExitStatusExt},
    },
    process::{Command, ExitStatus},
    time::Duration,
};
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(signal: i32, handler: usize) -> usize;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}
fn run() -> io::Result<ExitStatus> {
    let mut args = std::env::args_os().skip(1);
    let control: i32 = args.next().unwrap().to_str().unwrap().parse().unwrap();
    let lease: i32 = args.next().unwrap().to_str().unwrap().parse().unwrap();
    let program = args.next().unwrap();
    if !cfg!(supervisor_fixture) && program != "/usr/bin/bwrap" {
        return Err(io::Error::other("only Bubblewrap is supported"));
    }
    let mut control = unsafe { File::from_raw_fd(control) };
    let lease = unsafe { File::from_raw_fd(lease) };
    // Set up ownership before any child can escape through failed namespace setup.
    if unsafe { prctl(36, 1i32, 0i32, 0i32, 0i32) } < 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in [control.as_raw_fd(), lease.as_raw_fd()] {
        if unsafe { fcntl(fd, 2, 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let flags = unsafe { fcntl(control.as_raw_fd(), 3) };
    if flags < 0 || unsafe { fcntl(control.as_raw_fd(), 4, flags | 0o4000) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let child = match Command::new(program).args(args).spawn() {
        Ok(child) => child,
        Err(error) => {
            lease.write_all_at(b"complete", 0)?;
            return Err(error);
        }
    };
    let primary = child.id() as i32;
    // Diagnostic identities only; recovery never kills a PID from this record.
    let _ = lease.write_all_at(
        format!(
            " supervisor_pid={} primary_pid={primary}\n",
            std::process::id()
        )
        .as_bytes(),
        8,
    );
    let mut result: Option<ExitStatus> = None;
    let mut cleaning = false;
    loop {
        let mut byte = [0];
        match control.read(&mut byte) {
            Ok(_) => cleaning = true, // EOF means cancellation, drop, or parent exit.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => cleaning = true,
        }
        if cleaning {
            // Kill before reaping: a direct child's unreaped PID cannot be reused.
            // Killing its ancestors adopts even setsid grandchildren on the next pass.
            // Some kernels omit /proc/PID/task/TID/children. Read PPid from
            // stat instead; our single thread never reaps between this check
            // and kill, so these owned child PIDs cannot be recycled.
            for entry in std::fs::read_dir("/proc")? {
                let entry = entry?;
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                    continue;
                };
                let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                    Ok(stat) => stat,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                        ) =>
                    {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                let parent = stat
                    .rsplit_once(')')
                    .and_then(|(_, fields)| fields.split_whitespace().nth(1))
                    .and_then(|p| p.parse::<u32>().ok());
                if parent == Some(std::process::id()) {
                    unsafe {
                        kill(pid, 9);
                    }
                }
            }
        }
        loop {
            let mut status = 0;
            let pid = unsafe { waitpid(-1, &mut status, 1) };
            if pid > 0 {
                if pid == primary {
                    result = Some(ExitStatus::from_raw(status));
                    cleaning = true;
                }
            } else if pid == 0 {
                break;
            } else {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(10) {
                    // ECHILD: positively confirmed clean.
                    lease.write_all_at(b"complete", 0)?;
                    lease.sync_data()?;
                    return Ok(result.unwrap_or(ExitStatus::from_raw(125 << 8)));
                }
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn main() {
    match run() {
        Ok(status) => {
            if let Some(sig) = status.signal() {
                unsafe {
                    signal(sig, 0);
                    kill(std::process::id() as i32, sig);
                }
            }
            std::process::exit(status.code().unwrap_or(125));
        }
        Err(error) => {
            eprintln!("sandbox supervisor failed: {error}; cleanup may be incomplete");
            std::process::exit(125);
        }
    }
}
