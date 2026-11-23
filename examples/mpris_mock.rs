//! A fake MPRIS player for testing the shell without a real one:
//! `cargo run --example mpris_mock [cover.png]` — it "plays" a 3:30 track and answers PlayPause/Next/Seek.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::LocalConnection;
use dbus::message::MessageType;
use dbus::{Message, Path};

const P: &str = "org.mpris.MediaPlayer2.Player";

struct St {
    playing: bool,
    track: u32,
    pos_at: Instant,
    pos_us: i64,
    art: String,
}

fn meta(s: &St) -> PropMap {
    let mut m: PropMap = HashMap::new();
    let titles = ["SON6FORMYD34TH - Slowed", "Midnight Drive", "Glass Garden"];
    let artists = ["STAKILLAZ", "Night Cat", "Aurora Fields"];
    let i = (s.track % 3) as usize;
    m.insert("xesam:title".into(), Variant(Box::new(titles[i].to_string())));
    m.insert("xesam:artist".into(), Variant(Box::new(vec![artists[i].to_string()])));
    m.insert("mpris:length".into(), Variant(Box::new(210_000_000i64)));
    m.insert("mpris:trackid".into(), Variant(Box::new(Path::new(format!("/track/{}", s.track)).unwrap())));
    if !s.art.is_empty() {
        m.insert("mpris:artUrl".into(), Variant(Box::new(format!("file://{}", s.art))));
    }
    m
}

fn pos(s: &St) -> i64 {
    s.pos_us + if s.playing { s.pos_at.elapsed().as_micros() as i64 } else { 0 }
}

fn props(s: &St) -> PropMap {
    let mut m: PropMap = HashMap::new();
    m.insert("PlaybackStatus".into(), Variant(Box::new(if s.playing { "Playing" } else { "Paused" }.to_string())));
    m.insert("Metadata".into(), Variant(Box::new(meta(s)) as Box<dyn RefArg>));
    m.insert("Position".into(), Variant(Box::new(pos(s))));
    m.insert("CanSeek".into(), Variant(Box::new(true)));
    m.insert("CanGoNext".into(), Variant(Box::new(true)));
    m
}

fn changed(c: &LocalConnection, s: &St) {
    let mut m = props(s);
    m.remove("Position");
    let sig = Message::new_signal("/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties", "PropertiesChanged")
        .unwrap()
        .append3(P, m, Vec::<String>::new());
    let _ = c.channel().send(sig);
}

fn main() {
    let c = LocalConnection::new_session().expect("session bus");
    c.request_name("org.mpris.MediaPlayer2.mock", false, true, false).unwrap();
    let art = std::env::args().nth(1).unwrap_or_default();
    let mut s = St { playing: true, track: 0, pos_at: Instant::now(), pos_us: 42_000_000, art };
    changed(&c, &s);
    loop {
        c.channel().read_write(Some(Duration::from_millis(500))).ok();
        while let Some(m) = c.channel().pop_message() {
            if m.msg_type() != MessageType::MethodCall {
                continue;
            }
            let member = m.member().map(|x| x.to_string()).unwrap_or_default();
            let reply = match member.as_str() {
                "GetAll" => {
                    let iface: String = m.read1().unwrap_or_default();
                    if iface == P {
                        m.method_return().append1(props(&s))
                    } else {
                        let mut r: PropMap = HashMap::new();
                        r.insert("Identity".into(), Variant(Box::new("Mock Player".to_string())));
                        m.method_return().append1(r)
                    }
                }
                "Get" => {
                    let (_iface, name): (String, String) = m.read2().unwrap_or_default();
                    let all = props(&s);
                    match name.as_str() {
                        "Identity" => m.method_return().append1(Variant(Box::new("Mock Player".to_string()) as Box<dyn RefArg>)),
                        _ => match all.get(&name) {
                            Some(v) => m.method_return().append1(Variant(v.0.box_clone())),
                            None => m.error(&"org.freedesktop.DBus.Error.InvalidArgs".into(), c"no such property"),
                        },
                    }
                }
                "PlayPause" => {
                    s.pos_us = pos(&s);
                    s.pos_at = Instant::now();
                    s.playing = !s.playing;
                    changed(&c, &s);
                    m.method_return()
                }
                "Next" | "Previous" => {
                    s.track = if member == "Next" { s.track + 1 } else { s.track + 2 };
                    s.pos_us = 0;
                    s.pos_at = Instant::now();
                    changed(&c, &s);
                    m.method_return()
                }
                "SetPosition" => {
                    let (_t, p): (Path, i64) = m.read2().unwrap_or((Path::new("/").unwrap(), 0));
                    s.pos_us = p;
                    s.pos_at = Instant::now();
                    let sig = Message::new_signal("/org/mpris/MediaPlayer2", P, "Seeked").unwrap().append1(p);
                    let _ = c.channel().send(sig);
                    m.method_return()
                }
                _ => m.method_return(),
            };
            let _ = c.channel().send(reply);
        }
    }
}
