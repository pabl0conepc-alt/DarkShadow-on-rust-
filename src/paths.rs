//! Where the shipped files are (wallpapers, example config): $DSHELL_SHARE, next to the binary, or the repo.

use std::path::PathBuf;

pub fn share() -> PathBuf {
    if let Some(p) = std::env::var_os("DSHELL_SHARE") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        let s = bin.join("../share/dshell");
        if s.is_dir() {
            return s;
        }
        // cargo run: target/release/dshell -> the repository
        let repo = bin.join("../..");
        if repo.join("assets").is_dir() {
            return repo.join("assets");
        }
    }
    PathBuf::from("/usr/share/dshell")
}
