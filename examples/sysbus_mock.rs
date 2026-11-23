//! Fake NetworkManager + BlueZ on a test "system" bus, for trying the Wi-Fi and Bluetooth lists:
//!   dbus-daemon --session --fork --print-address   # use it as DBUS_SYSTEM_BUS_ADDRESS
//!   DBUS_SYSTEM_BUS_ADDRESS=... cargo run --example sysbus_mock

use std::collections::HashMap;
use std::time::Duration;

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::LocalConnection;
use dbus::message::MessageType;
use dbus::{Message, Path};

fn v<T: RefArg + 'static>(x: T) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(x))
}

fn op(s: &str) -> Path<'static> {
    Path::new(s.to_string()).unwrap()
}

const APS: &[(&str, &str, u8, u32)] = &[
    ("/ap/1", "Home", 82, 0x188),
    ("/ap/2", "Café Aurora", 55, 0),
    ("/ap/3", "Office 5G", 34, 0x188),
    ("/ap/4", "Neighbour", 12, 0x188),
];

fn main() {
    let c = LocalConnection::new_system().expect("DBUS_SYSTEM_BUS_ADDRESS");
    c.request_name("org.freedesktop.NetworkManager", false, true, false)
        .unwrap();
    c.request_name("org.bluez", false, true, false).unwrap();
    let mut active = String::from("/");
    let mut bt_connected = vec!["/org/bluez/hci0/dev_AA".to_string()];
    loop {
        c.channel()
            .read_write(Some(Duration::from_millis(500)))
            .ok();
        while let Some(m) = c.channel().pop_message() {
            if m.msg_type() != MessageType::MethodCall {
                continue;
            }
            let path = m.path().map(|p| p.to_string()).unwrap_or_default();
            let member = m.member().map(|x| x.to_string()).unwrap_or_default();
            let iface = m.interface().map(|x| x.to_string()).unwrap_or_default();
            let reply = match (path.as_str(), iface.as_str(), member.as_str()) {
                (
                    "/org/freedesktop/NetworkManager",
                    "org.freedesktop.DBus.Properties",
                    "GetAll",
                ) => {
                    let mut p = PropMap::new();
                    p.insert("WirelessEnabled".into(), v(true));
                    let conn = active != "/";
                    p.insert(
                        "PrimaryConnection".into(),
                        v(op(if conn { "/active/1" } else { "/" })),
                    );
                    p.insert(
                        "PrimaryConnectionType".into(),
                        v(if conn {
                            "802-11-wireless".to_string()
                        } else {
                            String::new()
                        }),
                    );
                    m.method_return().append1(p)
                }
                ("/org/freedesktop/NetworkManager", _, "GetDevices") => {
                    m.method_return().append1(vec![op("/dev/1")])
                }
                ("/org/freedesktop/NetworkManager", _, "ActivateConnection")
                | ("/org/freedesktop/NetworkManager", _, "AddAndActivateConnection") => {
                    let mut it = m.iter_init();
                    if member == "AddAndActivateConnection" {
                        let _: HashMap<String, PropMap> = it.read().unwrap_or_default();
                    } else {
                        let _: Path = it.read().unwrap();
                    }
                    let _: Path = it.read().unwrap();
                    let ap: Path = it.read().unwrap();
                    active = ap.to_string();
                    let sig = Message::new_signal(
                        "/org/freedesktop/NetworkManager",
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                    )
                    .unwrap()
                    .append3(
                        "org.freedesktop.NetworkManager",
                        PropMap::new(),
                        Vec::<String>::new(),
                    );
                    let _ = c.channel().send(sig);
                    if member == "ActivateConnection" {
                        m.method_return().append1(op("/active/1"))
                    } else {
                        m.method_return().append2(op("/c/2"), op("/active/1"))
                    }
                }
                ("/active/1", "org.freedesktop.DBus.Properties", "Get") => {
                    let (_, prop): (String, String) = m.read2().unwrap();
                    let name = APS
                        .iter()
                        .find(|a| a.0 == active)
                        .map(|a| a.1)
                        .unwrap_or("");
                    if prop == "Id" {
                        m.method_return().append1(v(name.to_string()))
                    } else {
                        m.method_return().append1(v(op(&active)))
                    }
                }
                ("/dev/1", "org.freedesktop.DBus.Properties", "Get") => {
                    let (_, prop): (String, String) = m.read2().unwrap();
                    match prop.as_str() {
                        "DeviceType" => m.method_return().append1(v(2u32)),
                        _ => m.method_return().append1(v(op(&active))),
                    }
                }
                ("/dev/1", _, "GetAllAccessPoints") => m
                    .method_return()
                    .append1(APS.iter().map(|a| op(a.0)).collect::<Vec<_>>()),
                ("/dev/1", _, "RequestScan") | ("/dev/1", _, "Disconnect") => {
                    if member == "Disconnect" {
                        active = "/".into();
                    }
                    m.method_return()
                }
                (p, "org.freedesktop.DBus.Properties", "GetAll") if p.starts_with("/ap/") => {
                    let a = APS.iter().find(|a| a.0 == p).unwrap();
                    let mut pm = PropMap::new();
                    pm.insert("Ssid".into(), v(a.1.as_bytes().to_vec()));
                    pm.insert("Strength".into(), v(a.2));
                    pm.insert("Flags".into(), v(if a.3 != 0 { 1u32 } else { 0u32 }));
                    pm.insert("WpaFlags".into(), v(0u32));
                    pm.insert("RsnFlags".into(), v(a.3));
                    m.method_return().append1(pm)
                }
                (p, "org.freedesktop.DBus.Properties", "Get") if p.starts_with("/ap/") => {
                    let a = APS.iter().find(|a| a.0 == p).unwrap();
                    m.method_return().append1(v(a.2))
                }
                ("/org/freedesktop/NetworkManager/Settings", _, "ListConnections") => {
                    m.method_return().append1(vec![op("/c/1")])
                }
                ("/c/1", _, "GetSettings") => {
                    let mut w = PropMap::new();
                    w.insert("ssid".into(), v(b"Home".to_vec()));
                    let mut s: HashMap<String, PropMap> = HashMap::new();
                    s.insert("802-11-wireless".into(), w);
                    m.method_return().append1(s)
                }
                ("/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects") => {
                    let mut objs: HashMap<Path, HashMap<String, PropMap>> = HashMap::new();
                    let mut ad = PropMap::new();
                    ad.insert("Powered".into(), v(true));
                    ad.insert("Discovering".into(), v(false));
                    objs.insert(
                        op("/org/bluez/hci0"),
                        HashMap::from([("org.bluez.Adapter1".to_string(), ad)]),
                    );
                    for (p, name, icon, paired) in [
                        (
                            "/org/bluez/hci0/dev_AA",
                            "Pixel Buds",
                            "audio-headset",
                            true,
                        ),
                        ("/org/bluez/hci0/dev_BB", "MX Keys", "input-keyboard", true),
                        ("/org/bluez/hci0/dev_CC", "Galaxy S24", "phone", false),
                    ] {
                        let mut d = PropMap::new();
                        d.insert("Alias".into(), v(name.to_string()));
                        d.insert("Name".into(), v(name.to_string()));
                        d.insert("Icon".into(), v(icon.to_string()));
                        d.insert("Paired".into(), v(paired));
                        d.insert("Connected".into(), v(bt_connected.iter().any(|x| x == p)));
                        let mut ifs = HashMap::from([("org.bluez.Device1".to_string(), d)]);
                        if p.ends_with("AA") {
                            let mut b = PropMap::new();
                            b.insert("Percentage".into(), v(80u8));
                            ifs.insert("org.bluez.Battery1".into(), b);
                        }
                        objs.insert(op(p), ifs);
                    }
                    m.method_return().append1(objs)
                }
                (p, "org.bluez.Device1", "Connect") | (p, "org.bluez.Device1", "Pair") => {
                    bt_connected.push(p.to_string());
                    let sig = Message::new_signal(
                        p,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                    )
                    .unwrap()
                    .append3(
                        "org.bluez.Device1",
                        PropMap::new(),
                        Vec::<String>::new(),
                    );
                    let _ = c.channel().send(sig);
                    m.method_return()
                }
                (p, "org.bluez.Device1", "Disconnect") => {
                    bt_connected.retain(|x| x != p);
                    let sig = Message::new_signal(
                        p,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                    )
                    .unwrap()
                    .append3(
                        "org.bluez.Device1",
                        PropMap::new(),
                        Vec::<String>::new(),
                    );
                    let _ = c.channel().send(sig);
                    m.method_return()
                }
                _ => m.error(
                    &"org.freedesktop.DBus.Error.UnknownMethod".into(),
                    c"mock: unknown",
                ),
            };
            let _ = c.channel().send(reply);
        }
    }
}
