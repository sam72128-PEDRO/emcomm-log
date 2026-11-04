//! Desktop GPS (2.1 spec §3). One foreground acquisition at a time, started only by an explicit
//! "Capture GPS" / "Test GPS" action: no background tracking, no location history, no network
//! (gpsd is reached on the loopback interface only). Every adapter returns either a verified fix or a
//! typed failure; nothing is ever fabricated, and quality fields are only filled when reported.

pub mod nmea;
pub mod os;

use crate::models::{Location, LocationSource};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT_S: u64 = 30;
pub const DEFAULT_MAX_AGE_S: u64 = 10;
pub const PROBE_BAUDS: [u32; 2] = [4800, 9600];

/// Where a receiver is attached; decides the honest source label of its fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialKind {
    Usb,
    Bluetooth,
    /// A GNSS module built into the computer that shows up as a serial port.
    Internal,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpsSource {
    /// "serial", "gpsd" or "os".
    pub provider: String,
    pub path: String,
    pub label: String,
    pub serial_kind: Option<SerialKind>,
    /// Free text from the OS (USB vendor/product, Bluetooth name) to help recognise a device.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GpsRequest {
    /// "serial", "gpsd" or "os".
    pub provider: String,
    pub path: String,
    /// 0 = probe 4800 then 9600.
    pub baud: u32,
    pub serial_kind: SerialKind,
    /// gpsd endpoint; must resolve to a loopback address.
    pub gpsd_endpoint: String,
    pub timeout_s: u64,
    pub max_age_s: u64,
}

impl Default for GpsRequest {
    fn default() -> Self {
        GpsRequest {
            provider: "serial".into(),
            path: String::new(),
            baud: 0,
            serial_kind: SerialKind::Usb,
            gpsd_endpoint: "127.0.0.1:2947".into(),
            timeout_s: DEFAULT_TIMEOUT_S,
            max_age_s: DEFAULT_MAX_AGE_S,
        }
    }
}

/// Typed outcome. `Fix` and `StaleFix` carry the location; every other status is a failure the UI
/// explains. A stale fix is only attached after the operator explicitly accepts it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpsOutcome {
    /// fix | stale_fix | permission_denied | no_device | device_busy | no_fix | timed_out |
    /// disconnected | invalid_sentence | unavailable | cancelled
    pub status: String,
    pub message: String,
    pub location: Option<Location>,
    pub fix_age_s: Option<f64>,
    pub baud: Option<u32>,
    /// Valid sentences seen / sentences rejected (bad checksum or malformed), for diagnostics.
    pub sentences: u32,
    pub rejected: u32,
}

impl GpsOutcome {
    pub fn fail(status: &str, message: impl Into<String>) -> GpsOutcome {
        GpsOutcome {
            status: status.into(),
            message: message.into(),
            location: None,
            fix_age_s: None,
            baud: None,
            sentences: 0,
            rejected: 0,
        }
    }
}

/// Seconds between the fix time and now (never negative: a receiver slightly ahead is age 0).
pub fn age_s(fix_time: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    ((now - fix_time).num_milliseconds() as f64 / 1000.0).max(0.0)
}

#[cfg_attr(target_os = "android", allow(dead_code))]
fn source_for(kind: SerialKind) -> LocationSource {
    match kind {
        SerialKind::Usb => LocationSource::ExternalUsbNmea,
        SerialKind::Bluetooth => LocationSource::ExternalBluetoothNmea,
        SerialKind::Internal => LocationSource::InternalGnss,
        SerialKind::Unknown => LocationSource::Unknown,
    }
}

/// Turn a verified fix into a location with an honest freshness verdict.
pub fn outcome_from_fix(fix: &nmea::Fix, source: LocationSource, provider: String, max_age_s: u64, now: DateTime<Utc>) -> GpsOutcome {
    let age = age_s(fix.time, now);
    let location = Location {
        latitude_deg: Some(fix.lat),
        longitude_deg: Some(fix.lon),
        label: String::new(),
        maidenhead: maidenhead(fix.lat, fix.lon),
        source,
        provider_id: provider,
        captured_at_utc: Some(fix.time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        accuracy_m: None,
        altitude_m: fix.altitude_m,
        satellites: fix.satellites,
        notes: fix.hdop.map(|h| format!("HDOP {h:.1} (not converted to metres)")).unwrap_or_default(),
    };
    let stale = age > max_age_s as f64;
    GpsOutcome {
        status: if stale { "stale_fix" } else { "fix" }.into(),
        message: if stale {
            format!("The receiver's last fix is {age:.0} s old (limit {max_age_s} s). Accept it only if the station has not moved.")
        } else {
            "Fix received".into()
        },
        location: Some(location),
        fix_age_s: Some(age),
        baud: None,
        sentences: 0,
        rejected: 0,
    }
}

/// 6-character Maidenhead locator (offline arithmetic).
pub fn maidenhead(lat: f64, lon: f64) -> String {
    let lon = (lon + 180.0).clamp(0.0, 359.999_999);
    let lat = (lat + 90.0).clamp(0.0, 179.999_999);
    let a = |v: f64| (b'A' + v as u8) as char;
    let s = |v: f64| (b'a' + v as u8) as char;
    format!(
        "{}{}{}{}{}{}",
        a((lon / 20.0).floor()),
        a((lat / 10.0).floor()),
        ((lon % 20.0) / 2.0).floor() as u8,
        (lat % 10.0).floor() as u8,
        s(((lon % 2.0) * 12.0).floor()),
        s(((lat % 1.0) * 24.0).floor()),
    )
}

/// Read NMEA lines from any byte stream until a fix, the deadline, or cancellation.
pub fn read_fix<R: Read>(
    reader: R,
    deadline: Instant,
    cancel: &AtomicBool,
    source: LocationSource,
    provider: String,
    max_age_s: u64,
) -> GpsOutcome {
    let mut r = BufReader::new(reader);
    let mut b = nmea::FixBuilder::default();
    let mut buf = Vec::new();
    // A valid RMC fix still waiting for its GGA is used if the stream ends or time runs out first.
    let held = |b: &nmea::FixBuilder| {
        b.pending().map(|fix| {
            let mut o = outcome_from_fix(&fix, source, provider.clone(), max_age_s, Utc::now());
            o.sentences = b.sentences;
            o.rejected = b.rejected;
            o
        })
    };
    loop {
        if cancel.load(Ordering::SeqCst) {
            return GpsOutcome { sentences: b.sentences, rejected: b.rejected, ..GpsOutcome::fail("cancelled", "Cancelled") };
        }
        if Instant::now() >= deadline {
            if let Some(o) = held(&b) {
                return o;
            }
            let (status, msg) = if b.sentences > 0 {
                (
                    "no_fix",
                    "The receiver is talking but has no valid fix yet. Move to open sky and try again (a cold start can take several minutes).",
                )
            } else if b.rejected > 0 {
                ("invalid_sentence", "Data arrived but none of it was valid NMEA (wrong baud rate or a non-NMEA device?)")
            } else {
                ("timed_out", "No data from the receiver before the timeout")
            };
            return GpsOutcome { sentences: b.sentences, rejected: b.rejected, ..GpsOutcome::fail(status, msg) };
        }
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) => {
                if let Some(o) = held(&b) {
                    return o;
                }
                return GpsOutcome {
                    sentences: b.sentences,
                    rejected: b.rejected,
                    ..GpsOutcome::fail("disconnected", "The receiver closed the connection")
                };
            }
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                if let Some(fix) = b.push_line(&line) {
                    let mut o = outcome_from_fix(&fix, source, provider.clone(), max_age_s, Utc::now());
                    o.sentences = b.sentences;
                    o.rejected = b.rejected;
                    return o;
                }
            }
            Err(e)
                if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {}
            Err(e) => {
                return GpsOutcome {
                    sentences: b.sentences,
                    rejected: b.rejected,
                    ..GpsOutcome::fail("disconnected", format!("The receiver stopped responding: {e}"))
                };
            }
        }
    }
}

// ---------- serial (USB / Bluetooth SPP / internal modules that enumerate as serial) ----------

#[cfg(not(target_os = "android"))]
pub fn list_serial() -> Vec<GpsSource> {
    let ports = serialport::available_ports().unwrap_or_default();
    let mut out: Vec<GpsSource> = ports
        .into_iter()
        // macOS lists each device twice; the "cu." call-out device is the one to open.
        .filter(|p| !p.port_name.starts_with("/dev/tty."))
        .filter(|p| !is_system_port(&p.port_name))
        .map(|p| {
            let (kind, detail) = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => (
                    SerialKind::Usb,
                    [
                        u.manufacturer.clone().unwrap_or_default(),
                        u.product.clone().unwrap_or_default(),
                        format!("{:04x}:{:04x}", u.vid, u.pid),
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                ),
                serialport::SerialPortType::BluetoothPort => (SerialKind::Bluetooth, "Bluetooth serial".into()),
                serialport::SerialPortType::PciPort => (SerialKind::Internal, "Built-in serial".into()),
                serialport::SerialPortType::Unknown => (SerialKind::Unknown, String::new()),
            };
            GpsSource { provider: "serial".into(), label: p.port_name.clone(), path: p.port_name, serial_kind: Some(kind), detail }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

/// macOS system ports that are never a GPS receiver (Bluetooth incoming service, Wi-Fi/debug consoles).
pub fn is_system_port(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.strip_prefix("cu.").or_else(|| name.strip_prefix("tty.")).unwrap_or(name);
    ["Bluetooth-Incoming-Port", "wlan-debug", "debug-console"].contains(&name) || name.ends_with("-WirelessiAP")
}

#[cfg(not(target_os = "android"))]
fn open_serial(path: &str, baud: u32) -> Result<Box<dyn serialport::SerialPort>, Box<GpsOutcome>> {
    serialport::new(path, baud).timeout(Duration::from_millis(500)).open().map_err(|e| {
        Box::new({
            use serialport::ErrorKind as K;
            let msg = e.description.clone();
            match e.kind {
                K::NoDevice => GpsOutcome::fail("no_device", format!("{path} is not connected ({msg})")),
                K::Io(std::io::ErrorKind::PermissionDenied) => GpsOutcome::fail(
                    "permission_denied",
                    if cfg!(target_os = "linux") {
                        format!("No permission to open {path}. Add your user to the 'dialout' (or 'uucp') group, then log out and back in.")
                    } else {
                        format!("No permission to open {path} ({msg})")
                    },
                ),
                K::Io(std::io::ErrorKind::NotFound) => {
                    GpsOutcome::fail("no_device", format!("{path} was not found. Is the receiver plugged in or paired?"))
                }
                _ if msg.to_lowercase().contains("busy") || msg.to_lowercase().contains("access is denied") => {
                    GpsOutcome::fail("device_busy", format!("{path} is in use by another program ({msg})"))
                }
                _ => GpsOutcome::fail("unavailable", format!("Could not open {path}: {msg}")),
            }
        })
    })
}

/// One fix from a serial NMEA receiver. With `baud == 0`, 4800 and 9600 are probed (a few seconds each
/// for valid sentences) before waiting for a fix. The port is closed on every return path.
#[cfg(not(target_os = "android"))]
pub fn acquire_serial(req: &GpsRequest, cancel: &AtomicBool) -> GpsOutcome {
    if req.path.trim().is_empty() {
        return GpsOutcome::fail("no_device", "Choose the receiver's port first");
    }
    let deadline = Instant::now() + Duration::from_secs(req.timeout_s.clamp(5, 600));
    let bauds: Vec<u32> = if req.baud == 0 { PROBE_BAUDS.to_vec() } else { vec![req.baud] };
    let source = source_for(req.serial_kind);
    let mut last = GpsOutcome::fail("timed_out", "No data from the receiver");
    for (i, &baud) in bauds.iter().enumerate() {
        let port = match open_serial(&req.path, baud) {
            Ok(p) => p,
            Err(o) => return *o,
        };
        // While probing, give each speed a few seconds to produce valid sentences.
        let probing = bauds.len() > 1 && i + 1 < bauds.len();
        let until = if probing { (Instant::now() + Duration::from_secs(4)).min(deadline) } else { deadline };
        let provider = format!("{} @ {baud}", req.path);
        let mut o = read_fix(port, until, cancel, source, provider, req.max_age_s);
        o.baud = Some(baud);
        let heard_valid = o.sentences > 0;
        if matches!(o.status.as_str(), "fix" | "stale_fix" | "cancelled" | "disconnected") || (probing && heard_valid) {
            if probing && heard_valid && o.location.is_none() {
                // Right speed, no fix yet: keep listening at this speed for the rest of the time.
                let port = match open_serial(&req.path, baud) {
                    Ok(p) => p,
                    Err(o) => return *o,
                };
                let mut o2 = read_fix(port, deadline, cancel, source, format!("{} @ {baud}", req.path), req.max_age_s);
                o2.baud = Some(baud);
                return o2;
            }
            return o;
        }
        last = o;
    }
    last
}

// ---------- gpsd (Linux), loopback only ----------

fn loopback_addr(endpoint: &str) -> Result<SocketAddr, Box<GpsOutcome>> {
    let ep = if endpoint.trim().is_empty() { "127.0.0.1:2947" } else { endpoint.trim() };
    let addrs: Vec<SocketAddr> =
        ep.to_socket_addrs().map_err(|e| Box::new(GpsOutcome::fail("unavailable", format!("Bad gpsd address {ep}: {e}"))))?.collect();
    addrs
        .into_iter()
        .find(|a| match a.ip() {
            IpAddr::V4(v) => v.is_loopback(),
            IpAddr::V6(v) => v.is_loopback(),
        })
        .ok_or_else(|| {
            Box::new(GpsOutcome::fail("unavailable", "gpsd must be on this computer (localhost); network addresses are not used"))
        })
}

/// Parse one gpsd TPV report into a fix. Mode 2/3 means a fix; times are ISO 8601 UTC.
pub fn tpv_fix(line: &str) -> Option<nmea::Fix> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("class")?.as_str()? != "TPV" || v.get("mode")?.as_i64()? < 2 {
        return None;
    }
    let lat = v.get("lat")?.as_f64()?;
    let lon = v.get("lon")?.as_f64()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let time = DateTime::parse_from_rfc3339(v.get("time")?.as_str()?).ok()?.with_timezone(&Utc);
    let alt = v.get("altMSL").or_else(|| v.get("alt")).and_then(|a| a.as_f64());
    Some(nmea::Fix { lat, lon, time, satellites: None, altitude_m: if v.get("mode")?.as_i64()? == 3 { alt } else { None }, hdop: None })
}

pub fn acquire_gpsd(req: &GpsRequest, cancel: &AtomicBool) -> GpsOutcome {
    let addr = match loopback_addr(&req.gpsd_endpoint) {
        Ok(a) => a,
        Err(o) => return *o,
    };
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(s) => s,
        Err(e) => {
            return GpsOutcome::fail(
                "unavailable",
                format!("gpsd is not running at {addr} ({e}). Install and start gpsd, or use the receiver's serial port directly."),
            );
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    if stream.write_all(b"?WATCH={\"enable\":true,\"json\":true};\n").is_err() {
        return GpsOutcome::fail("disconnected", "gpsd closed the connection");
    }
    let deadline = Instant::now() + Duration::from_secs(req.timeout_s.clamp(5, 600));
    let mut r = BufReader::new(stream);
    let mut buf = String::new();
    let mut reports = 0u32;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return GpsOutcome::fail("cancelled", "Cancelled");
        }
        if Instant::now() >= deadline {
            return if reports > 0 {
                GpsOutcome { sentences: reports, ..GpsOutcome::fail("no_fix", "gpsd is running but reports no fix yet") }
            } else {
                GpsOutcome::fail("no_device", "gpsd reports no GPS device. Check that gpsd is configured with your receiver.")
            };
        }
        buf.clear();
        match r.read_line(&mut buf) {
            Ok(0) => return GpsOutcome::fail("disconnected", "gpsd closed the connection"),
            Ok(_) => {
                if buf.contains("\"class\":\"TPV\"") {
                    reports += 1;
                }
                if let Some(fix) = tpv_fix(&buf) {
                    let mut o = outcome_from_fix(&fix, LocationSource::Gpsd, format!("gpsd {addr}"), req.max_age_s, Utc::now());
                    o.sentences = reports;
                    return o;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {}
            Err(e) => return GpsOutcome::fail("disconnected", format!("gpsd connection failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(body: &str) -> String {
        format!("${body}*{:02X}\r\n", body.bytes().fold(0u8, |a, b| a ^ b))
    }

    #[test]
    fn maidenhead_matches_known_locators() {
        assert_eq!(maidenhead(33.8121, -84.4011), "EM73tt");
        assert_eq!(maidenhead(41.714775, -72.727260), "FN31pr");
        assert_eq!(maidenhead(-33.8688, 151.2093), "QF56od");
    }

    #[test]
    fn reads_a_recorded_session_to_a_fix() {
        let now = Utc::now();
        let t = now.format("%H%M%S.00").to_string();
        let d = now.format("%d%m%y").to_string();
        let data = [
            "noise from a USB adapter\r\n".to_string(),
            line(&format!("GPGGA,{t},,,,,0,00,99.9,,,,,,")),
            "$GPRMC,BROKEN*00\r\n".to_string(),
            line(&format!("GPGGA,{t},3348.7260,N,08424.0660,W,1,09,0.9,212.5,M,-30.0,M,,")),
            line(&format!("GPRMC,{t},A,3348.7260,N,08424.0660,W,0,,{d},,,A")),
        ]
        .concat();
        let cancel = AtomicBool::new(false);
        let o =
            read_fix(data.as_bytes(), Instant::now() + Duration::from_secs(5), &cancel, LocationSource::ExternalUsbNmea, "test".into(), 10);
        assert_eq!(o.status, "fix", "{}", o.message);
        let l = o.location.unwrap();
        assert_eq!((l.source, l.satellites, l.accuracy_m), (LocationSource::ExternalUsbNmea, Some(9), None));
        assert_eq!(o.rejected, 1);
        assert!(l.notes.contains("HDOP 0.9"));
    }

    #[test]
    fn old_fix_is_stale_and_no_fix_is_never_success() {
        let data = line("GPRMC,120000.00,A,3348.7260,N,08424.0660,W,0,,010120,,,A");
        let cancel = AtomicBool::new(false);
        let o = read_fix(data.as_bytes(), Instant::now() + Duration::from_secs(2), &cancel, LocationSource::Gpsd, "t".into(), 10);
        assert_eq!(o.status, "stale_fix");
        assert!(o.fix_age_s.unwrap() > 1e6);

        let void = line("GPRMC,120000.00,V,,,,,,,010126,,,N").repeat(3);
        let o = read_fix(void.as_bytes(), Instant::now() + Duration::from_secs(2), &cancel, LocationSource::Gpsd, "t".into(), 10);
        assert_eq!((o.status.as_str(), o.location.is_none()), ("disconnected", true), "stream ended without a fix");

        // A live port never ends: junk followed by silence until the deadline.
        let junk = "$GPRMC,1,A*00\r\n".repeat(3);
        let o = read_fix(
            junk.as_bytes().chain(std::io::repeat(b'\n')),
            Instant::now() + Duration::from_millis(300),
            &cancel,
            LocationSource::Gpsd,
            "t".into(),
            10,
        );
        assert_eq!((o.status.as_str(), o.rejected), ("invalid_sentence", 3));
        let quiet =
            read_fix(std::io::repeat(b'\n'), Instant::now() + Duration::from_millis(200), &cancel, LocationSource::Gpsd, "t".into(), 10);
        assert_eq!(quiet.status, "timed_out");

        cancel.store(true, Ordering::SeqCst);
        let o = read_fix(void.as_bytes(), Instant::now() + Duration::from_secs(2), &cancel, LocationSource::Gpsd, "t".into(), 10);
        assert_eq!(o.status, "cancelled");
    }

    #[test]
    fn macos_system_ports_are_hidden() {
        assert!(is_system_port("/dev/cu.Bluetooth-Incoming-Port"));
        assert!(is_system_port("/dev/cu.wlan-debug"));
        assert!(!is_system_port("/dev/cu.usbmodem101"));
        assert!(!is_system_port("/dev/cu.GPS-Puck-SerialPort"));
        assert!(!is_system_port("COM3"));
    }

    #[test]
    fn gpsd_tpv_reports_and_loopback_rule() {
        assert!(tpv_fix(r#"{"class":"TPV","mode":1,"time":"2026-11-02T14:00:00.000Z"}"#).is_none(), "mode 1 = no fix");
        let f =
            tpv_fix(r#"{"class":"TPV","mode":3,"time":"2026-11-02T14:00:00.000Z","lat":33.8121,"lon":-84.4011,"altMSL":212.5,"eph":5.2}"#)
                .unwrap();
        assert_eq!((f.lat, f.altitude_m), (33.8121, Some(212.5)));
        assert!(tpv_fix(r#"{"class":"SKY","mode":3}"#).is_none());
        assert_eq!(loopback_addr("192.168.1.10:2947").unwrap_err().status, "unavailable");
        assert!(loopback_addr("127.0.0.1:2947").is_ok());
        assert!(loopback_addr("[::1]:2947").is_ok());
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn missing_port_is_reported_not_faked() {
        let cancel = AtomicBool::new(false);
        let path = if cfg!(windows) { "COM250" } else { "/dev/cu.emcomm-no-such-port" };
        let o = acquire_serial(&GpsRequest { path: path.into(), timeout_s: 5, ..Default::default() }, &cancel);
        assert!(matches!(o.status.as_str(), "no_device" | "unavailable"), "{} {}", o.status, o.message);
        assert!(o.location.is_none());
        let o = acquire_serial(&GpsRequest { path: String::new(), ..Default::default() }, &cancel);
        assert_eq!(o.status, "no_device");
    }

    #[test]
    fn gpsd_end_to_end_over_loopback() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 64];
            let _ = s.read(&mut req);
            let _ = s.write_all(b"{\"class\":\"VERSION\",\"release\":\"3.25\"}\n{\"class\":\"TPV\",\"mode\":1}\n");
            let _ = s.write_all(format!("{{\"class\":\"TPV\",\"mode\":2,\"time\":\"{now}\",\"lat\":45.5,\"lon\":-122.6}}\n").as_bytes());
        });
        let cancel = AtomicBool::new(false);
        let o = acquire_gpsd(
            &GpsRequest { provider: "gpsd".into(), gpsd_endpoint: addr.to_string(), timeout_s: 5, ..Default::default() },
            &cancel,
        );
        assert_eq!(o.status, "fix", "{}", o.message);
        let l = o.location.unwrap();
        assert_eq!((l.source, l.latitude_deg, l.altitude_m), (LocationSource::Gpsd, Some(45.5), None));
        let none = acquire_gpsd(&GpsRequest { gpsd_endpoint: "127.0.0.1:9".into(), timeout_s: 5, ..Default::default() }, &cancel);
        assert_eq!(none.status, "unavailable");
    }
}

/// The real serial path (open, read, probe, close) driven through a pseudo-terminal that behaves like a
/// USB receiver. Recorded data, not hardware: real receivers are verified separately. Linux only: macOS
/// refuses the custom-speed ioctl on a pseudo-terminal ("Not a typewriter"), which real devices accept.
#[cfg(all(test, target_os = "linux"))]
mod serial_pty_tests {
    use super::*;
    use std::ffi::CStr;
    use std::os::fd::FromRawFd;

    fn pty() -> (std::fs::File, String) {
        let (mut master, mut slave) = (0, 0);
        let mut name = [0 as libc::c_char; 128];
        let rc = unsafe { libc::openpty(&mut master, &mut slave, name.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(rc, 0, "openpty");
        let path = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().to_string();
        // Keep the slave open in this process so the master does not see a hang-up between opens.
        std::mem::forget(unsafe { std::fs::File::from_raw_fd(slave) });
        (unsafe { std::fs::File::from_raw_fd(master) }, path)
    }

    fn line(body: &str) -> String {
        format!("${body}*{:02X}\r\n", body.bytes().fold(0u8, |a, b| a ^ b))
    }

    #[test]
    fn serial_receiver_gives_a_fresh_fix_and_releases_the_port() {
        let (mut master, path) = pty();
        let feeder = std::thread::spawn(move || {
            for _ in 0..40 {
                let now = Utc::now();
                let (t, d) = (now.format("%H%M%S.00").to_string(), now.format("%d%m%y").to_string());
                let chunk = line(&format!("GNGGA,{t},4542.1234,N,12236.4321,W,1,11,0.8,61.0,M,-21.0,M,,"))
                    + &line(&format!("GNRMC,{t},A,4542.1234,N,12236.4321,W,0.01,,{d},,,A,V"));
                if master.write_all(chunk.as_bytes()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let cancel = AtomicBool::new(false);
        let req = GpsRequest { path: path.clone(), baud: 0, serial_kind: SerialKind::Usb, timeout_s: 10, ..Default::default() };
        let o = acquire_serial(&req, &cancel);
        assert_eq!(o.status, "fix", "{}", o.message);
        let l = o.location.unwrap();
        assert_eq!((l.source, l.satellites), (LocationSource::ExternalUsbNmea, Some(11)));
        assert!((l.latitude_deg.unwrap() - (45.0 + 42.1234 / 60.0)).abs() < 1e-9);
        assert!(l.provider_id.starts_with(&path));
        assert!(o.fix_age_s.unwrap() < 10.0);
        // Released: the same port opens again straight away.
        assert!(serialport::new(&path, 4800).timeout(Duration::from_millis(100)).open().is_ok());
        let _ = feeder.join();
    }
}
