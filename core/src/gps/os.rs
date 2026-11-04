//! Operating-system location (2.1 spec §3.5): macOS Core Location, Windows Geolocator, Linux GeoClue2.
//! The OS may answer from Wi-Fi, cell or a stored position, so fixes are labelled `os_position` unless
//! the OS itself says the position came from satellites (Windows reports this). Permission is only
//! requested when the operator presses Capture/Test — never when a logbook opens.

use super::{GpsOutcome, GpsRequest, age_s, maidenhead};
use crate::models::{Location, LocationSource};
use chrono::{DateTime, Utc};
use std::sync::atomic::AtomicBool;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::atomic::Ordering;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::{Duration, Instant};

/// Runs a closure on the app's main thread (required by Core Location on macOS).
pub type MainThread<'a> = &'a dyn Fn(Box<dyn FnOnce() + Send>);

pub const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux"));
pub const LABEL: &str = if cfg!(target_os = "macos") {
    "macOS Location Services"
} else if cfg!(target_os = "windows") {
    "Windows location"
} else {
    "System location (GeoClue)"
};

/// A position reported by the OS, before freshness is judged.
#[derive(Debug, Clone)]
pub struct OsFix {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    /// Horizontal accuracy in metres as reported by the OS (it computes it; we never derive one).
    pub accuracy_m: Option<f64>,
    pub altitude_m: Option<f64>,
    pub source: LocationSource,
    pub provider: String,
}

pub fn outcome(f: OsFix, max_age_s: u64) -> GpsOutcome {
    if !(-90.0..=90.0).contains(&f.lat) || !(-180.0..=180.0).contains(&f.lon) || !f.lat.is_finite() || !f.lon.is_finite() {
        return GpsOutcome::fail("invalid_sentence", "The OS returned an invalid position");
    }
    let age = age_s(f.time, Utc::now());
    let stale = age > max_age_s as f64;
    GpsOutcome {
        status: if stale { "stale_fix" } else { "fix" }.into(),
        message: if stale { format!("The OS position is {age:.0} s old (limit {max_age_s} s).") } else { "Position received".into() },
        location: Some(Location {
            latitude_deg: Some(f.lat),
            longitude_deg: Some(f.lon),
            label: String::new(),
            maidenhead: maidenhead(f.lat, f.lon),
            source: f.source,
            provider_id: f.provider,
            captured_at_utc: Some(f.time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            accuracy_m: f.accuracy_m.filter(|a| a.is_finite() && *a >= 0.0),
            altitude_m: f.altitude_m.filter(|a| a.is_finite() && a.abs() < 100_000.0),
            satellites: None,
            notes: String::new(),
        }),
        fix_age_s: Some(age),
        baud: None,
        sentences: 0,
        rejected: 0,
    }
}

// ---------- macOS: Core Location ----------

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
    use std::cell::RefCell;
    use std::sync::mpsc;

    thread_local! {
        // Lives on the main thread only; one manager for the app.
        static MANAGER: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
    }

    enum Poll {
        Waiting,
        Denied,
        Disabled,
        Fix(OsFix),
    }

    fn on_main<T: Send + 'static>(main: MainThread, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        main(Box::new(move || {
            let _ = tx.send(f());
        }));
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }

    pub fn acquire(req: &GpsRequest, cancel: &AtomicBool, main: MainThread) -> GpsOutcome {
        let started = on_main(main, || unsafe {
            if !CLLocationManager::locationServicesEnabled_class() {
                return false;
            }
            MANAGER.with(|m| {
                let mut m = m.borrow_mut();
                let mgr = m.get_or_insert_with(|| CLLocationManager::new());
                mgr.requestWhenInUseAuthorization();
                mgr.startUpdatingLocation();
            });
            true
        });
        match started {
            None => return GpsOutcome::fail("unavailable", "Location Services did not respond"),
            Some(false) => {
                return GpsOutcome::fail(
                    "permission_denied",
                    "Location Services are turned off (System Settings → Privacy & Security → Location Services)",
                );
            }
            Some(true) => {}
        }
        let deadline = Instant::now() + Duration::from_secs(req.timeout_s.clamp(5, 600));
        let since = Utc::now() - chrono::Duration::seconds(req.max_age_s as i64);
        let result = loop {
            if cancel.load(Ordering::SeqCst) {
                break GpsOutcome::fail("cancelled", "Cancelled");
            }
            if Instant::now() >= deadline {
                break GpsOutcome::fail("timed_out", "macOS did not provide a position before the timeout");
            }
            let poll = on_main(main, || unsafe {
                MANAGER.with(|m| {
                    let m = m.borrow();
                    let Some(mgr) = m.as_ref() else { return Poll::Waiting };
                    let st = mgr.authorizationStatus();
                    if st == CLAuthorizationStatus::Denied || st == CLAuthorizationStatus::Restricted {
                        return Poll::Denied;
                    }
                    if !CLLocationManager::locationServicesEnabled_class() {
                        return Poll::Disabled;
                    }
                    match mgr.location() {
                        Some(l) => {
                            let c = l.coordinate();
                            let secs = l.timestamp().timeIntervalSince1970();
                            let time = DateTime::from_timestamp_millis((secs * 1000.0) as i64).unwrap_or_else(Utc::now);
                            let h = l.horizontalAccuracy();
                            let v = l.verticalAccuracy();
                            Poll::Fix(OsFix {
                                lat: c.latitude,
                                lon: c.longitude,
                                time,
                                accuracy_m: (h >= 0.0).then_some(h),
                                altitude_m: (v >= 0.0).then_some(l.altitude()),
                                source: LocationSource::OsPosition,
                                provider: "macOS Core Location".into(),
                            })
                        }
                        None => Poll::Waiting,
                    }
                })
            });
            match poll {
                Some(Poll::Denied) => {
                    break GpsOutcome::fail(
                        "permission_denied",
                        "EMCOMM LOG is not allowed to use your location (System Settings → Privacy & Security → Location Services)",
                    );
                }
                Some(Poll::Disabled) => break GpsOutcome::fail("permission_denied", "Location Services are turned off"),
                // Prefer a position newer than the start of this request; an older cached one is only
                // returned (as stale) if nothing fresher arrives.
                Some(Poll::Fix(f)) if f.time >= since => break outcome(f, req.max_age_s),
                Some(Poll::Fix(f)) if Instant::now() + Duration::from_millis(300) >= deadline => break outcome(f, req.max_age_s),
                _ => std::thread::sleep(Duration::from_millis(250)),
            }
        };
        on_main(main, || unsafe {
            MANAGER.with(|m| {
                if let Some(mgr) = m.borrow().as_ref() {
                    mgr.stopUpdatingLocation();
                }
            })
        });
        result
    }
}

// ---------- Windows: Windows.Devices.Geolocation ----------

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy, PositionSource};
    use windows::Foundation::TimeSpan;

    fn ts(secs: u64) -> TimeSpan {
        TimeSpan { Duration: (secs as i64) * 10_000_000 }
    }

    pub fn acquire(req: &GpsRequest) -> GpsOutcome {
        let run = || -> windows::core::Result<GpsOutcome> {
            if Geolocator::RequestAccessAsync()?.get()? != GeolocationAccessStatus::Allowed {
                return Ok(GpsOutcome::fail(
                    "permission_denied",
                    "Location access is off for this app (Settings → Privacy & security → Location)",
                ));
            }
            let g = Geolocator::new()?;
            g.SetDesiredAccuracy(PositionAccuracy::High)?;
            let pos = g.GetGeopositionAsyncWithAgeAndTimeout(ts(req.max_age_s), ts(req.timeout_s.clamp(5, 600)))?.get()?;
            let c = pos.Coordinate()?;
            let p = c.Point()?.Position()?;
            // FILETIME ticks (100 ns since 1601-01-01) -> Unix time.
            let ticks = c.Timestamp()?.UniversalTime;
            let unix_ms = (ticks - 116_444_736_000_000_000) / 10_000;
            let time = DateTime::from_timestamp_millis(unix_ms).unwrap_or_else(Utc::now);
            let src = c.PositionSource()?;
            let (source, how) = if src == PositionSource::Satellite {
                (LocationSource::InternalGnss, "satellite")
            } else if src == PositionSource::WiFi {
                (LocationSource::OsPosition, "Wi-Fi")
            } else if src == PositionSource::Cellular {
                (LocationSource::OsPosition, "cellular")
            } else if src == PositionSource::IPAddress {
                (LocationSource::OsPosition, "IP address")
            } else if src == PositionSource::Default {
                (LocationSource::OsPosition, "default location")
            } else {
                (LocationSource::OsPosition, "unknown")
            };
            Ok(outcome(
                OsFix {
                    lat: p.Latitude,
                    lon: p.Longitude,
                    time,
                    accuracy_m: c.Accuracy().ok(),
                    altitude_m: None,
                    source,
                    provider: format!("Windows location ({how})"),
                },
                req.max_age_s,
            ))
        };
        match run() {
            Ok(o) => o,
            Err(e) if e.code().0 as u32 == 0x8000_0013 || e.code().0 as u32 == 0x8007_05B4 => {
                GpsOutcome::fail("timed_out", "Windows did not provide a position before the timeout")
            }
            Err(e) if e.code().0 as u32 == 0x8007_0005 => GpsOutcome::fail("permission_denied", "Location access is denied for this app"),
            Err(e) => GpsOutcome::fail("unavailable", format!("Windows location is not available ({})", e.message())),
        }
    }
}

// ---------- Linux: GeoClue2 over the system D-Bus ----------

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure};

    const DEST: &str = "org.freedesktop.GeoClue2";

    pub fn acquire(req: &GpsRequest, cancel: &AtomicBool) -> GpsOutcome {
        let run = || -> zbus::Result<GpsOutcome> {
            let conn = Connection::system()?;
            let mgr = Proxy::new(&conn, DEST, "/org/freedesktop/GeoClue2/Manager", "org.freedesktop.GeoClue2.Manager")?;
            let path: OwnedObjectPath = mgr.call("GetClient", &())?;
            let client = Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.GeoClue2.Client")?;
            client.set_property("DesktopId", "org.emcommlog.desktop")?;
            client.set_property("RequestedAccuracyLevel", 8u32)?;
            client.call::<_, _, ()>("Start", &())?;
            let deadline = Instant::now() + Duration::from_secs(req.timeout_s.clamp(5, 600));
            let out = loop {
                if cancel.load(Ordering::SeqCst) {
                    break GpsOutcome::fail("cancelled", "Cancelled");
                }
                if Instant::now() >= deadline {
                    break GpsOutcome::fail("timed_out", "GeoClue did not provide a position before the timeout");
                }
                let loc: OwnedObjectPath = client.get_property("Location")?;
                if loc.as_str() != "/" {
                    let l = Proxy::new(&conn, DEST, loc.as_str(), "org.freedesktop.GeoClue2.Location")?;
                    let lat: f64 = l.get_property("Latitude")?;
                    let lon: f64 = l.get_property("Longitude")?;
                    let acc: f64 = l.get_property("Accuracy")?;
                    let alt: f64 = l.get_property("Altitude")?;
                    let ts: OwnedValue = l.get_property("Timestamp")?;
                    let time = Structure::try_from(ts)
                        .ok()
                        .and_then(|s| {
                            let f = s.fields();
                            let secs = u64::try_from(f.first()?.try_clone().ok()?).ok()?;
                            let usecs = u64::try_from(f.get(1)?.try_clone().ok()?).ok()?;
                            DateTime::from_timestamp(secs as i64, (usecs * 1000) as u32)
                        })
                        .unwrap_or_else(Utc::now);
                    break outcome(
                        OsFix {
                            lat,
                            lon,
                            time,
                            accuracy_m: Some(acc),
                            altitude_m: (alt > -1.0e300).then_some(alt),
                            source: LocationSource::OsPosition,
                            provider: "GeoClue2".into(),
                        },
                        req.max_age_s,
                    );
                }
                std::thread::sleep(Duration::from_millis(250));
            };
            let _ = client.call::<_, _, ()>("Stop", &());
            Ok(out)
        };
        match run() {
            Ok(o) => o,
            Err(zbus::Error::MethodError(name, msg, _)) if name.as_str().contains("AccessDenied") => {
                GpsOutcome::fail("permission_denied", format!("GeoClue refused location access ({})", msg.unwrap_or_default()))
            }
            Err(e) => GpsOutcome::fail("unavailable", format!("System location (GeoClue) is not available: {e}")),
        }
    }
}

/// One OS position. `main` runs work on the app's main thread (only macOS needs it).
#[allow(unused_variables)]
pub fn acquire(req: &GpsRequest, cancel: &AtomicBool, main: MainThread) -> GpsOutcome {
    #[cfg(target_os = "macos")]
    return mac::acquire(req, cancel, main);
    #[cfg(target_os = "windows")]
    return win::acquire(req);
    #[cfg(target_os = "linux")]
    return linux::acquire(req, cancel);
    #[allow(unreachable_code)]
    GpsOutcome::fail("unavailable", "This computer has no operating-system location service EMCOMM LOG can use")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_positions_are_labelled_honestly_and_judged_for_age() {
        let now = Utc::now();
        let f = OsFix {
            lat: 33.8121,
            lon: -84.4011,
            time: now,
            accuracy_m: Some(35.0),
            altitude_m: None,
            source: LocationSource::OsPosition,
            provider: "test".into(),
        };
        let o = outcome(f.clone(), 10);
        assert_eq!(o.status, "fix");
        let l = o.location.unwrap();
        assert_eq!((l.source, l.accuracy_m, l.satellites), (LocationSource::OsPosition, Some(35.0), None));
        let old = OsFix { time: now - chrono::Duration::minutes(5), ..f.clone() };
        assert_eq!(outcome(old, 10).status, "stale_fix");
        let bad = OsFix { lat: f64::NAN, ..f };
        assert_eq!(outcome(bad, 10).status, "invalid_sentence");
    }
}
