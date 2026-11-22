//! The weather from wttr.in (what the original uses), through curl, every half hour.

use std::time::{Duration, Instant};

use serde_json::Value;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::app::App;
use crate::state::topic;

pub fn icon(code: u32, night: bool) -> &'static str {
    match code {
        113 => {
            if night {
                "clear_night"
            } else {
                "sunny"
            }
        }
        116 => {
            if night {
                "partly_cloudy_night"
            } else {
                "partly_cloudy_day"
            }
        }
        119 | 122 => "cloud",
        143 | 248 | 260 => "foggy",
        200 | 386 | 389 | 392 | 395 => "thunderstorm",
        179 | 182 | 185 | 227 | 230 | 281 | 284 | 317 | 320 | 323 | 326 | 329 | 332 | 335 | 338 | 350 | 362 | 365 | 368
        | 371 | 374 | 377 => "weather_snowy",
        176 | 263 | 266 | 293 | 296 | 299 | 302 | 305 | 308 | 311 | 314 | 353 | 356 | 359 => "rainy",
        _ => "cloud",
    }
}

pub fn start(app: &mut App) {
    if !app.cfg.weather.enabled {
        return;
    }
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_secs(3)), |_, _, app: &mut App| {
        fetch(app);
        TimeoutAction::ToDuration(Duration::from_secs(60 * app.cfg.weather.minutes.max(10) as u64))
    });
}

pub fn fetch(app: &mut App) {
    if app.svc.weather_busy || !app.cfg.weather.enabled {
        return;
    }
    if let Some(t) = app.svc.weather_at
        && t.elapsed() < Duration::from_secs(300)
        && app.st.weather.ok
    {
        return;
    }
    app.svc.weather_busy = true;
    let place = app.cfg.weather.location.trim().replace(' ', "+");
    let url = format!("https://wttr.in/{place}?format=j1&lang=en");
    crate::proc::capture(app, &["curl", "-fsS", "-m", "15", &url], |app, out| {
        app.svc.weather_busy = false;
        let Ok(v) = serde_json::from_str::<Value>(&out) else { return };
        app.svc.weather_at = Some(Instant::now());
        let f = app.cfg.weather.units.eq_ignore_ascii_case("f");
        let num = |x: &Value| x.as_str().and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let cur = &v["current_condition"][0];
        let tm = crate::clock::now();
        let night = !(6..19).contains(&tm.hour);
        let w = &mut app.st.weather;
        w.ok = true;
        w.unit = if f { 'F' } else { 'C' };
        w.temp = num(&cur[if f { "temp_F" } else { "temp_C" }]);
        w.feels = num(&cur[if f { "FeelsLikeF" } else { "FeelsLikeC" }]);
        let code = num(&cur["weatherCode"]) as u32;
        w.icon = icon(code, night);
        w.desc = cur["weatherDesc"][0]["value"].as_str().unwrap_or("").trim().to_string();
        w.humidity = num(&cur["humidity"]) as u32;
        w.wind = num(&cur["windspeedKmph"]);
        w.city = v["nearest_area"][0]["areaName"][0]["value"].as_str().unwrap_or("").to_string();
        let day = &v["weather"][0];
        w.min = num(&day[if f { "mintempF" } else { "mintempC" }]);
        w.max = num(&day[if f { "maxtempF" } else { "maxtempC" }]);
        w.hours.clear();
        if let Some(hs) = day["hourly"].as_array() {
            for h in hs {
                let t = num(&h["time"]) as u32 / 100;
                let c = num(&h["weatherCode"]) as u32;
                w.hours.push((format!("{t:02}:00"), num(&h[if f { "tempF" } else { "tempC" }]), icon(c, !(6..19).contains(&t))));
            }
        }
        app.mark(topic::WEATHER);
    });
}
