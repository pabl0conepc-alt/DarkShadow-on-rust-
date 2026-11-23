//! Two fake tray items (StatusNotifierItem) for testing the bar's tray:
//! one sends its picture as pixels, the other names a theme icon. Clicks are written to stdout.

use std::time::Duration;

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::LocalConnection;
use dbus::message::MessageType;

fn v<T: RefArg + 'static>(x: T) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(x))
}

fn pixels() -> Vec<(i32, i32, Vec<u8>)> {
    let s = 22;
    let mut d = Vec::new();
    for y in 0..s {
        for x in 0..s {
            let inside = (x - 11) * (x - 11) + (y - 11) * (y - 11) < 100;
            // ARGB, network byte order
            if inside { d.extend_from_slice(&[255, 255, 140, (x * 10) as u8]) } else { d.extend_from_slice(&[0, 0, 0, 0]) }
        }
    }
    vec![(s, s, d)]
}

fn main() {
    let c = LocalConnection::new_session().unwrap();
    let names = ["org.kde.StatusNotifierItem-mock-1", "org.kde.StatusNotifierItem-mock-2"];
    for n in names {
        c.request_name(n, false, true, false).unwrap();
    }
    let w = c.with_proxy("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", Duration::from_secs(2));
    for n in names {
        let r: Result<(), _> = w.method_call("org.kde.StatusNotifierWatcher", "RegisterStatusNotifierItem", (n,));
        println!("register {n}: {:?}", r.is_ok());
    }
    loop {
        c.channel().read_write(Some(Duration::from_millis(500))).ok();
        while let Some(m) = c.channel().pop_message() {
            if m.msg_type() != MessageType::MethodCall {
                continue;
            }
            let dest = m.destination().map(|d| d.to_string()).unwrap_or_default();
            let member = m.member().map(|x| x.to_string()).unwrap_or_default();
            let first = dest.ends_with('1');
            let reply = match member.as_str() {
                "GetAll" => {
                    let mut p = PropMap::new();
                    p.insert("Id".into(), v(if first { "chat".to_string() } else { "calc".to_string() }));
                    p.insert("Title".into(), v(if first { "Chat".to_string() } else { "Calc".to_string() }));
                    p.insert("Status".into(), v("Active".to_string()));
                    if first {
                        p.insert("IconName".into(), v(String::new()));
                        p.insert("IconPixmap".into(), v(pixels()));
                    } else {
                        p.insert("IconName".into(), v("libreoffice-calc".to_string()));
                    }
                    m.method_return().append1(p)
                }
                "Activate" | "ContextMenu" | "SecondaryActivate" => {
                    println!("{member} on {dest}");
                    m.method_return()
                }
                _ => m.method_return(),
            };
            let _ = c.channel().send(reply);
        }
    }
}
