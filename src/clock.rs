//! Local time through libc (no date crate needed).

#[derive(Clone, Copy, Debug, Default)]
pub struct Tm {
    pub year: i32,
    /// 0..11
    pub mon: u32,
    /// 1..31
    pub mday: u32,
    /// 0 = Monday
    pub wday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
}

pub fn now() -> Tm {
    from_unix(unix())
}

pub fn unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn from_unix(t: i64) -> Tm {
    // SAFETY: localtime_r writes into the struct we pass.
    unsafe {
        let tt: libc::time_t = t as libc::time_t;
        let mut out: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tt, &mut out);
        Tm {
            year: out.tm_year + 1900,
            mon: out.tm_mon as u32,
            mday: out.tm_mday as u32,
            wday: ((out.tm_wday + 6) % 7) as u32,
            hour: out.tm_hour as u32,
            min: out.tm_min as u32,
            sec: out.tm_sec as u32,
        }
    }
}

pub fn days_in_month(year: i32, mon: u32) -> u32 {
    match mon {
        1 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        3 | 5 | 8 | 10 => 30,
        _ => 31,
    }
}

/// Weekday (0 = Monday) of the first day of the month of `t`.
pub fn first_wday(t: &Tm) -> u32 {
    (t.wday + 7 * 6 - ((t.mday - 1) % 7)) % 7
}

/// Seconds until the next whole minute (or second).
pub fn until_next(seconds: bool) -> std::time::Duration {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ms = now.as_millis() as u64;
    let period = if seconds { 1000 } else { 60_000 };
    std::time::Duration::from_millis(period - ms % period + 5)
}

pub fn greeting(hour: u32) -> &'static str {
    match hour {
        // the same hours as the original
        0..=4 => "Good Night",
        5..=11 => "Good Morning",
        12..=17 => "Good Afternoon",
        _ => "Good Evening",
    }
}
