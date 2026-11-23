//! dshell — DarkShadow's Shell in one small Rust binary.

mod actions;
mod anim;
mod app;
mod clock;
mod color;
mod config;
mod gfx;
mod icons_data;
mod ipc;
mod paths;
mod proc;
mod raster;
mod settings;
mod shell;
mod state;
mod svc;
mod text;
mod theme;
mod ui;

const HELP: &str = "dshell — DarkShadow's Shell (Rust)

  dshell                       start the shell (same as `dshell daemon`)
  dshell toggle <panel>        launcher | control | settings | clipboard | power | picker | today | system | player
  dshell open|close <panel>    the same, one way; `close all` closes every panel
  dshell volume up|down|mute|set <0-100>     (shows in the island)
  dshell mic mute
  dshell brightness up|down|set <0-100>
  dshell media play-pause|next|prev
  dshell wallpaper set <file>|next|random
  dshell scheme <auto|grey|navy|mono|slate|...|violet|#rrggbb>
  dshell notifs clear|dnd|test
  dshell ws <n>|next|prev     go to a workspace (Hyprland) / bookmark (driftwm)
  dshell shot region|full|window          screenshot (saved and copied)
  dshell record region|full|full-sound|stop   screen recording (again to stop)
  dshell edit                 move the desktop widgets with the mouse (again to finish)
  dshell gaming               gaming mode: no motion, no blur, no visualizer
  dshell focus toggle|reset|skip
  dshell config               write the example config to ~/.config/dshell/config.toml
  dshell reload | quit | status
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str).unwrap_or("daemon");
    match first {
        "daemon" | "run" => {
            if let Err(e) = app::run() {
                eprintln!("dshell: {e}");
                std::process::exit(1);
            }
        }
        "-h" | "--help" | "help" => print!("{HELP}"),
        "-V" | "--version" | "version" => println!("dshell {}", env!("CARGO_PKG_VERSION")),
        "config" => {
            let p = config::path();
            if p.exists() && args.get(1).map(String::as_str) != Some("--force") {
                eprintln!(
                    "{} exists (use `dshell config --force` to overwrite)",
                    p.display()
                );
                std::process::exit(1);
            }
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            match std::fs::write(&p, config::DEFAULT_TOML) {
                Ok(_) => println!("wrote {}", p.display()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1)
                }
            }
        }
        _ => std::process::exit(ipc::send(&args.join(" "))),
    }
}
