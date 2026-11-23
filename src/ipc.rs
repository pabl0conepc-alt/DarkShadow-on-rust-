//! `dshell <command>` talks to the running shell over a Unix socket: one line in, one line out.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::{Interest, Mode, PostAction, generic::Generic};

use crate::app::App;

pub fn socket() -> PathBuf {
    let disp = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    crate::config::runtime_dir().join(format!("dshell-{disp}.sock"))
}

pub fn serve(app: &mut App) {
    let path = socket();
    if UnixStream::connect(&path).is_ok() {
        eprintln!("dshell: another dshell is already running on this display");
        std::process::exit(1);
    }
    let _ = std::fs::remove_file(&path);
    let Ok(l) = UnixListener::bind(&path) else {
        eprintln!("dshell: cannot listen on {}", path.display());
        return;
    };
    let _ = l.set_nonblocking(true);
    let _ = app.lh.insert_source(Generic::new(l, Interest::READ, Mode::Level), |_, l, app| {
        while let Ok((mut s, _)) = l.accept() {
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
            let mut line = String::new();
            let _ = BufReader::new(&s).read_line(&mut line);
            let reply = crate::actions::command(app, line.trim());
            let _ = s.write_all(reply.as_bytes());
            let _ = s.write_all(b"\n");
        }
        Ok(PostAction::Continue)
    });
}

/// Client side: send the command, print the answer. Exit code 1 when the shell is not running.
pub fn send(cmd: &str) -> i32 {
    let Ok(mut s) = UnixStream::connect(socket()) else {
        eprintln!("dshell is not running (start it with `dshell` or `dshell daemon`)");
        return 1;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.write_all(format!("{cmd}\n").as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let out = out.trim();
    if out.starts_with("error") {
        eprintln!("{out}");
        return 2;
    }
    if !out.is_empty() && out != "ok" {
        println!("{out}");
    }
    0
}
