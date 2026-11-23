//! Child processes without blocking the loop: fire-and-forget commands, one-shot captures and line streams.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use smithay_client_toolkit::reexports::calloop::{
    Interest, Mode, PostAction, RegistrationToken, generic::Generic,
};

use crate::app::App;

pub fn set_nonblock(fd: i32) {
    // SAFETY: plain fcntl on a descriptor we own.
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
}

/// Start a program detached from the shell (its own session, no pipes).
pub fn spawn(argv: &[&str]) {
    if argv.is_empty() {
        return;
    }
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    if let Ok(mut ch) = c.spawn() {
        // reap it in the background so it never stays a zombie
        std::thread::spawn(move || {
            let _ = ch.wait();
        });
    }
}

pub fn sh(cmd: &str) {
    if !cmd.trim().is_empty() {
        spawn(&["sh", "-c", cmd]);
    }
}

pub fn which(name: &str) -> bool {
    if name.contains('/') {
        return std::path::Path::new(name).exists();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

/// Run a program and hand its whole output to `done` once it exits (never blocks the loop).
pub fn capture(app: &mut App, argv: &[&str], done: impl FnOnce(&mut App, String) + 'static) {
    let Some(mut child) = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
    else {
        done(app, String::new());
        return;
    };
    let out = child.stdout.take().unwrap();
    set_nonblock(out.as_raw_fd());
    let mut buf = Vec::new();
    let mut done = Some(done);
    let mut child = Some(child);
    let src = Generic::new(out, Interest::READ, Mode::Level);
    let r = app.lh.insert_source(src, move |_, f, app| {
        let mut tmp = [0u8; 8192];
        loop {
            // SAFETY: the pipe is not dropped here.
            match unsafe { f.get_mut() }.read(&mut tmp) {
                Ok(0) => {
                    if let Some(mut c) = child.take() {
                        let _ = c.wait();
                    }
                    if let Some(d) = done.take() {
                        d(app, String::from_utf8_lossy(&buf).into_owned());
                    }
                    return Ok(PostAction::Remove);
                }
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(PostAction::Continue);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    if let Some(d) = done.take() {
                        d(app, String::from_utf8_lossy(&buf).into_owned());
                    }
                    return Ok(PostAction::Remove);
                }
            }
        }
    });
    if r.is_err() {
        // nothing to do: the closure (and the child) are dropped
    }
}

/// A long-running program whose output arrives line by line (pactl subscribe, cava).
pub struct Stream {
    pub child: Child,
    pub token: RegistrationToken,
}

impl Stream {
    pub fn stop(mut self, app: &mut App) {
        app.lh.remove(self.token);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn stream(
    app: &mut App,
    argv: &[&str],
    mut on_line: impl FnMut(&mut App, &str) + 'static,
    on_end: impl FnOnce(&mut App) + 'static,
) -> Option<Stream> {
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: prctl is async-signal-safe; the child dies with the shell.
    unsafe {
        c.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = c.spawn().ok()?;
    let out = child.stdout.take()?;
    set_nonblock(out.as_raw_fd());
    let mut acc: Vec<u8> = Vec::new();
    let mut on_end = Some(on_end);
    let token = app
        .lh
        .insert_source(
            Generic::new(out, Interest::READ, Mode::Level),
            move |_, f, app| {
                let mut tmp = [0u8; 16384];
                loop {
                    // SAFETY: the pipe is not dropped here.
                    match unsafe { f.get_mut() }.read(&mut tmp) {
                        Ok(0) => {
                            if let Some(e) = on_end.take() {
                                e(app);
                            }
                            return Ok(PostAction::Remove);
                        }
                        Ok(n) => {
                            acc.extend_from_slice(&tmp[..n]);
                            while let Some(p) = acc.iter().position(|&b| b == b'\n') {
                                let line: Vec<u8> = acc.drain(..=p).collect();
                                let s = String::from_utf8_lossy(&line[..line.len() - 1]);
                                on_line(app, &s);
                            }
                            if acc.len() > 1 << 20 {
                                acc.clear();
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(PostAction::Continue);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            if let Some(e) = on_end.take() {
                                e(app);
                            }
                            return Ok(PostAction::Remove);
                        }
                    }
                }
            },
        )
        .ok()?;
    Some(Stream { child, token })
}

/// Run something on a worker thread and get the result back on the loop (image decoding).
pub fn background<T: Send + 'static>(
    app: &mut App,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut App, T) + 'static,
) {
    use std::any::Any;
    if app.svc.bg_tx.is_none() {
        let (tx, rx) = smithay_client_toolkit::reexports::calloop::channel::channel::<(
            u64,
            Box<dyn Any + Send>,
        )>();
        let _ = app.lh.insert_source(rx, |ev, _, app| {
            if let smithay_client_toolkit::reexports::calloop::channel::Event::Msg((id, v)) = ev
                && let Some(d) = app.svc.bg_done.remove(&id)
            {
                d(app, v);
            }
        });
        app.svc.bg_tx = Some(tx);
    }
    let id = app.svc.bg_next;
    app.svc.bg_next += 1;
    app.svc.bg_done.insert(
        id,
        Box::new(move |app: &mut App, v: Box<dyn Any + Send>| {
            if let Ok(v) = v.downcast::<T>() {
                done(app, *v);
            }
        }),
    );
    let tx = app.svc.bg_tx.clone().unwrap();
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let _ = tx.send((id, Box::new(work()) as Box<dyn Any + Send>));
        })
        .ok();
}

/// A helper program that lives as long as the shell (it gets SIGTERM when the shell goes).
pub fn bound(argv: &[&str]) -> Option<Child> {
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: prctl is async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    c.spawn().ok()
}

/// Is a program with this command line already running (for this user)?
pub fn running(cmdline: &str) -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return false;
    };
    let me = unsafe { libc::getuid() };
    for e in rd.flatten() {
        let p = e.path();
        if !e
            .file_name()
            .to_string_lossy()
            .chars()
            .all(|c| c.is_ascii_digit())
        {
            continue;
        }
        use std::os::unix::fs::MetadataExt;
        if std::fs::metadata(&p).map(|m| m.uid() != me).unwrap_or(true) {
            continue;
        }
        if let Ok(cl) = std::fs::read(p.join("cmdline")) {
            let s: String = cl
                .iter()
                .map(|&b| if b == 0 { ' ' } else { b as char })
                .collect();
            if s.trim() == cmdline || s.contains(cmdline) {
                return true;
            }
        }
    }
    false
}
