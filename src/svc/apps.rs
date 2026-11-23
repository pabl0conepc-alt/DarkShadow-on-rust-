//! Installed apps (.desktop files) and icon lookup in the icon theme.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub exec: String,
    pub icon: String,
    pub keywords: String,
    pub terminal: bool,
    /// lowercase name + keywords for searching
    pub hay: String,
    /// StartupWMClass, lowercase (to match windows to apps)
    pub wmclass: String,
}

#[derive(Default)]
pub struct Apps {
    pub list: Vec<Entry>,
    loaded: Option<Instant>,
    icon_paths: HashMap<String, Option<PathBuf>>,
    theme_dirs: Vec<PathBuf>,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = crate::config::home();
    v.push(
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share")),
    );
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    for d in sys.split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d));
    }
    for d in [
        "/usr/local/share",
        "/usr/share",
        "/run/current-system/sw/share",
        "/var/lib/flatpak/exports/share",
    ] {
        v.push(PathBuf::from(d));
    }
    v.push(home.join(".nix-profile/share"));
    v.push(home.join(".local/share/flatpak/exports/share"));
    if let Ok(u) = std::env::var("USER") {
        v.push(PathBuf::from(format!("/etc/profiles/per-user/{u}/share")));
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| seen.insert(p.clone()));
    v
}

fn parse(path: &Path, id: &str) -> Option<Entry> {
    let text = std::fs::read_to_string(path).ok()?;
    let lang = std::env::var("LANG").unwrap_or_default();
    let lang = lang.split(['.', '@']).next().unwrap_or("").to_string();
    let short = lang.split('_').next().unwrap_or("").to_string();
    let mut in_main = false;
    let mut f: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let k = k.trim();
            let v = v.trim().to_string();
            // localized names win over the plain ones
            if let Some(base) = k
                .strip_suffix(&format!("[{lang}]"))
                .or_else(|| k.strip_suffix(&format!("[{short}]")))
            {
                f.insert(format!("{base}#l"), v);
            } else if !k.contains('[') {
                f.entry(k.to_string()).or_insert(v);
            }
        }
    }
    let get = |k: &str| {
        f.get(&format!("{k}#l"))
            .or_else(|| f.get(k))
            .cloned()
            .unwrap_or_default()
    };
    if get("Type") != "Application" || get("NoDisplay") == "true" || get("Hidden") == "true" {
        return None;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let only = get("OnlyShowIn");
    if !only.is_empty()
        && !only
            .split(';')
            .any(|d| !d.is_empty() && desktop.split(':').any(|x| x == d))
    {
        return None;
    }
    let name = get("Name");
    let exec = get("Exec");
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    let generic = get("GenericName");
    let keywords = get("Keywords");
    let hay = format!(
        "{} {} {} {}",
        name,
        generic,
        keywords.replace(';', " "),
        id.trim_end_matches(".desktop")
    )
    .to_lowercase();
    Some(Entry {
        id: id.to_string(),
        name,
        generic,
        comment: get("Comment"),
        exec,
        icon: get("Icon"),
        keywords,
        terminal: get("Terminal") == "true",
        hay,
        wmclass: get("StartupWMClass").to_lowercase(),
    })
}

impl Apps {
    /// (Re)read the apps if it has been a while; cheap enough to do every time the launcher opens.
    pub fn ensure(&mut self) {
        if let Some(t) = self.loaded
            && t.elapsed() < Duration::from_secs(20)
        {
            return;
        }
        let mut map: HashMap<String, Entry> = HashMap::new();
        for d in data_dirs() {
            let dir = d.join("applications");
            scan(&dir, &dir, &mut map);
        }
        map.retain(|_, e| !e.id.is_empty());
        let mut list: Vec<Entry> = map.into_values().collect();
        list.sort_by_key(|a| a.name.to_lowercase());
        self.list = list;
        self.loaded = Some(Instant::now());
    }

    fn theme_dirs(&mut self, theme: &str) -> Vec<PathBuf> {
        if !self.theme_dirs.is_empty() {
            return self.theme_dirs.clone();
        }
        let mut themes: Vec<String> = Vec::new();
        let push = |t: &str, themes: &mut Vec<String>| {
            if !t.is_empty() && !themes.iter().any(|x| x == t) {
                themes.push(t.to_string());
            }
        };
        push(theme, &mut themes);
        let gtk = crate::config::home().join(".config/gtk-3.0/settings.ini");
        if let Ok(s) = std::fs::read_to_string(gtk) {
            for l in s.lines() {
                if let Some(v) = l.trim().strip_prefix("gtk-icon-theme-name") {
                    push(v.trim_start_matches([' ', '=']).trim(), &mut themes);
                }
            }
        }
        let bases: Vec<PathBuf> = {
            let mut b = vec![crate::config::home().join(".icons")];
            b.extend(data_dirs().into_iter().map(|d| d.join("icons")));
            b
        };
        // follow Inherits= one level
        let mut i = 0;
        while i < themes.len() && i < 6 {
            let t = themes[i].clone();
            for b in &bases {
                if let Ok(s) = std::fs::read_to_string(b.join(&t).join("index.theme")) {
                    for l in s.lines() {
                        if let Some(v) = l.strip_prefix("Inherits=") {
                            for x in v.split(',') {
                                push(x.trim(), &mut themes);
                            }
                        }
                    }
                    break;
                }
            }
            i += 1;
        }
        for t in [
            "Papirus-Dark",
            "Papirus",
            "Adwaita",
            "breeze-dark",
            "breeze",
            "hicolor",
        ] {
            push(t, &mut themes);
        }
        let mut dirs = Vec::new();
        for t in &themes {
            for b in &bases {
                let p = b.join(t);
                if p.is_dir() {
                    dirs.push(p);
                }
            }
        }
        self.theme_dirs = dirs.clone();
        dirs
    }

    /// The file for an icon name (svg or png), preferring scalable / large sizes.
    pub fn find_icon(&mut self, name: &str, theme: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        if name.starts_with('/') {
            return Path::new(name).is_file().then(|| PathBuf::from(name));
        }
        if let Some(p) = self.icon_paths.get(name) {
            return p.clone();
        }
        let dirs = self.theme_dirs(theme);
        let subs = [
            "scalable/apps",
            "apps/scalable",
            "128x128/apps",
            "96x96/apps",
            "64x64/apps",
            "apps/64",
            "48x48/apps",
            "apps/48",
            "256x256/apps",
            "512x512/apps",
            "32x32/apps",
            "scalable/devices",
            "scalable/status",
            "48x48/status",
            "scalable/places",
            "48x48/places",
        ];
        let mut found = None;
        'outer: for d in &dirs {
            for s in subs {
                for ext in ["svg", "png"] {
                    let p = d.join(s).join(format!("{name}.{ext}"));
                    if p.is_file() {
                        found = Some(p);
                        break 'outer;
                    }
                }
            }
        }
        if found.is_none() {
            for d in data_dirs() {
                for ext in ["svg", "png", "xpm"] {
                    let p = d.join("pixmaps").join(format!("{name}.{ext}"));
                    if ext != "xpm" && p.is_file() {
                        found = Some(p);
                    }
                }
            }
        }
        self.icon_paths.insert(name.to_string(), found.clone());
        found
    }
}

fn scan(root: &Path, dir: &Path, map: &mut HashMap<String, Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(root, &p, map);
            continue;
        }
        if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
            continue;
        }
        let id = p
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('/', "-"))
            .unwrap_or_default();
        if map.contains_key(&id) {
            continue; // earlier data dirs win (the user's own first)
        }
        if let Some(en) = parse(&p, &id) {
            map.insert(id, en);
        } else {
            // a hidden entry still shadows the system one
            map.insert(
                id.clone(),
                Entry {
                    id: String::new(),
                    name: String::new(),
                    generic: String::new(),
                    comment: String::new(),
                    exec: String::new(),
                    icon: String::new(),
                    keywords: String::new(),
                    terminal: false,
                    hay: String::new(),
                    wmclass: String::new(),
                },
            );
        }
    }
}

/// Score how well `q` matches an app (0 = no match).
pub fn score(e: &Entry, q: &str) -> i32 {
    if q.is_empty() {
        return 1;
    }
    let name = e.name.to_lowercase();
    if name == q {
        return 1000;
    }
    if name.starts_with(q) {
        return 800 - name.len() as i32;
    }
    if name.split([' ', '-', '_']).any(|w| w.starts_with(q)) {
        return 600 - name.len() as i32;
    }
    if e.hay.contains(q) {
        return 400 - e.hay.find(q).unwrap_or(0) as i32;
    }
    // letters in order
    let mut it = e.hay.chars();
    let mut gaps = 0;
    for c in q.chars() {
        let mut found = false;
        for h in it.by_ref() {
            if h == c {
                found = true;
                break;
            }
            gaps += 1;
        }
        if !found {
            return 0;
        }
    }
    (200 - gaps).max(1)
}

/// The command line of an entry without the %f %u ... field codes.
pub fn command(e: &Entry, terminal: &str) -> String {
    let mut out = String::new();
    let mut chars = e.exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(&format!("'{}'", e.name.replace('\'', ""))),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    let cmd = out.trim().to_string();
    if e.terminal {
        format!("{terminal} -e {cmd}")
    } else {
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, exec: &str) -> Entry {
        Entry {
            id: format!("{name}.desktop"),
            name: name.into(),
            generic: String::new(),
            comment: String::new(),
            exec: exec.into(),
            icon: String::new(),
            keywords: String::new(),
            terminal: false,
            hay: name.to_lowercase(),
            wmclass: String::new(),
        }
    }

    #[test]
    fn scoring_prefers_prefix_then_word_then_substring() {
        let ff = e("Firefox", "firefox %u");
        let fm = e("File Manager", "nautilus");
        assert!(score(&ff, "fire") > score(&fm, "fi"));
        assert!(score(&fm, "man") > 0);
        assert_eq!(score(&ff, "zzz"), 0);
    }

    #[test]
    fn field_codes_are_removed() {
        assert_eq!(command(&e("Firefox", "firefox %u"), "kitty"), "firefox");
        assert_eq!(
            command(&e("Files", "nautilus --new-window %U"), "kitty"),
            "nautilus --new-window"
        );
    }
}

/// The app a window belongs to (by its class / app id).
pub fn for_window<'a>(list: &'a [Entry], class: &str) -> Option<(usize, &'a Entry)> {
    let c = class.to_lowercase();
    if c.is_empty() {
        return None;
    }
    let stem = |e: &Entry| e.id.trim_end_matches(".desktop").to_lowercase();
    list.iter()
        .enumerate()
        .find(|(_, e)| e.wmclass == c)
        .or_else(|| list.iter().enumerate().find(|(_, e)| stem(e) == c))
        .or_else(|| {
            list.iter()
                .enumerate()
                .find(|(_, e)| stem(e).rsplit('.').next() == Some(c.as_str()))
        })
        .or_else(|| {
            list.iter()
                .enumerate()
                .find(|(_, e)| e.name.to_lowercase() == c)
        })
        .or_else(|| {
            list.iter().enumerate().find(|(_, e)| {
                e.exec
                    .split_whitespace()
                    .next()
                    .map(|x| x.rsplit('/').next().unwrap_or(x).to_lowercase())
                    == Some(c.clone())
            })
        })
}
