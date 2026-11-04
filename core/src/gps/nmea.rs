//! NMEA 0183 sentence parsing for one-shot fixes (RMC, GGA; GSA for diagnostics). Every sentence must
//! carry a valid checksum; malformed data is rejected, never guessed at. Any talker ID is accepted
//! (GP, GN, GL, GA, BD, …). Nothing is ever sent to the receiver.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum Sentence {
    Rmc(Rmc),
    Gga(Gga),
    Gsa {
        fix_type: u8,
    },
    /// A valid sentence of a type this reader does not use (GSV, VTG, TXT, …).
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rmc {
    pub time: Option<NaiveTime>,
    pub date: Option<NaiveDate>,
    /// Status "A" (valid) and, when present, a mode indicator that is not "N" (not valid).
    pub valid: bool,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gga {
    pub time: Option<NaiveTime>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// 0 = no fix; 1 GPS; 2 DGPS; 4/5 RTK; 6 = dead reckoning (estimated, treated as no fix).
    pub quality: u8,
    pub satellites: Option<u32>,
    pub hdop: Option<f64>,
    pub altitude_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NmeaError {
    NotNmea,
    BadChecksum,
    Malformed(&'static str),
}

fn checksum_ok(body: &str, hex: &str) -> bool {
    let want = match u8::from_str_radix(hex.trim(), 16) {
        Ok(v) if hex.trim().len() == 2 => v,
        _ => return false,
    };
    body.bytes().fold(0u8, |a, b| a ^ b) == want
}

/// ddmm.mmmm + hemisphere -> signed decimal degrees.
fn coord(v: &str, hemi: &str, deg_digits: usize) -> Result<Option<f64>, NmeaError> {
    if v.is_empty() && hemi.is_empty() {
        return Ok(None);
    }
    if v.len() < deg_digits + 2 || !v.is_ascii() {
        return Err(NmeaError::Malformed("coordinate"));
    }
    let deg: f64 = v[..deg_digits].parse().map_err(|_| NmeaError::Malformed("coordinate"))?;
    let min: f64 = v[deg_digits..].parse().map_err(|_| NmeaError::Malformed("coordinate"))?;
    if !(0.0..60.0).contains(&min) {
        return Err(NmeaError::Malformed("coordinate minutes"));
    }
    let mut d = deg + min / 60.0;
    match hemi {
        "N" | "E" => {}
        "S" | "W" => d = -d,
        _ => return Err(NmeaError::Malformed("hemisphere")),
    }
    let max = if deg_digits == 2 { 90.0 } else { 180.0 };
    if d.abs() > max {
        return Err(NmeaError::Malformed("coordinate range"));
    }
    Ok(Some(d))
}

fn time(v: &str) -> Result<Option<NaiveTime>, NmeaError> {
    if v.is_empty() {
        return Ok(None);
    }
    if v.len() < 6 {
        return Err(NmeaError::Malformed("time"));
    }
    let (h, m, s) = (&v[0..2], &v[2..4], &v[4..]);
    let secs: f64 = s.parse().map_err(|_| NmeaError::Malformed("time"))?;
    let whole = secs.trunc() as u32;
    let nanos = ((secs.fract()) * 1e9).round() as u32;
    NaiveTime::from_hms_nano_opt(
        h.parse().map_err(|_| NmeaError::Malformed("time"))?,
        m.parse().map_err(|_| NmeaError::Malformed("time"))?,
        whole.min(59),
        nanos.min(999_999_999),
    )
    .map(Some)
    .ok_or(NmeaError::Malformed("time"))
}

fn date(v: &str) -> Result<Option<NaiveDate>, NmeaError> {
    if v.is_empty() {
        return Ok(None);
    }
    if v.len() != 6 {
        return Err(NmeaError::Malformed("date"));
    }
    let d: u32 = v[0..2].parse().map_err(|_| NmeaError::Malformed("date"))?;
    let mo: u32 = v[2..4].parse().map_err(|_| NmeaError::Malformed("date"))?;
    let y: i32 = v[4..6].parse().map_err(|_| NmeaError::Malformed("date"))?;
    // Two-digit year: NMEA receivers report 1980 onwards.
    let year = if y >= 80 { 1900 + y } else { 2000 + y };
    NaiveDate::from_ymd_opt(year, mo, d).map(Some).ok_or(NmeaError::Malformed("date"))
}

fn num<T: std::str::FromStr>(v: &str) -> Option<T> {
    if v.is_empty() { None } else { v.parse().ok() }
}

/// Parse one line ("$GNRMC,...*hh"). Lines without a leading '$' or with a bad checksum are errors.
pub fn parse(line: &str) -> Result<Sentence, NmeaError> {
    let line = line.trim();
    let rest = line.strip_prefix('$').ok_or(NmeaError::NotNmea)?;
    let (body, cs) = rest.rsplit_once('*').ok_or(NmeaError::BadChecksum)?;
    if !checksum_ok(body, cs) {
        return Err(NmeaError::BadChecksum);
    }
    let f: Vec<&str> = body.split(',').collect();
    let tag = f[0];
    if tag.len() < 5 || !tag.is_ascii() {
        return Err(NmeaError::Malformed("address"));
    }
    let kind = &tag[tag.len() - 3..];
    let get = |i: usize| f.get(i).copied().unwrap_or("");
    match kind {
        "RMC" => {
            if f.len() < 10 {
                return Err(NmeaError::Malformed("RMC fields"));
            }
            let mode = get(12);
            Ok(Sentence::Rmc(Rmc {
                time: time(get(1))?,
                valid: get(2) == "A" && mode != "N",
                lat: coord(get(3), get(4), 2)?,
                lon: coord(get(5), get(6), 3)?,
                date: date(get(9))?,
            }))
        }
        "GGA" => {
            if f.len() < 10 {
                return Err(NmeaError::Malformed("GGA fields"));
            }
            Ok(Sentence::Gga(Gga {
                time: time(get(1))?,
                lat: coord(get(2), get(3), 2)?,
                lon: coord(get(4), get(5), 3)?,
                quality: num(get(6)).unwrap_or(0),
                satellites: num(get(7)),
                hdop: num(get(8)),
                altitude_m: if get(10).is_empty() || get(10) == "M" { num(get(9)) } else { None },
            }))
        }
        "GSA" => Ok(Sentence::Gsa { fix_type: num(get(2)).unwrap_or(1) }),
        _ => Ok(Sentence::Other),
    }
}

/// A verified fix assembled from RMC (+ GGA when the receiver sends it for the same second).
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    pub satellites: Option<u32>,
    pub altitude_m: Option<f64>,
    pub hdop: Option<f64>,
}

/// Collects sentences until a valid, fully timestamped fix exists. GGA quality 0 (no fix) or 6 (dead
/// reckoning) for the same second vetoes the RMC; RMC status V never yields a fix.
#[derive(Default)]
pub struct FixBuilder {
    rmc: Option<Rmc>,
    gga: Option<Gga>,
    /// Consecutive valid RMC epochs without a GGA for the same time (receivers that send GGA rarely).
    unmatched: u32,
    pub sentences: u32,
    pub rejected: u32,
}

impl FixBuilder {
    pub fn push_line(&mut self, line: &str) -> Option<Fix> {
        match parse(line) {
            Ok(s) => {
                self.sentences += 1;
                self.push(s)
            }
            Err(NmeaError::NotNmea) => None,
            Err(_) => {
                self.rejected += 1;
                None
            }
        }
    }

    pub fn push(&mut self, s: Sentence) -> Option<Fix> {
        match s {
            Sentence::Rmc(r) => {
                self.rmc = Some(r);
                // Many receivers (u-blox among them) send RMC before the GGA of the same second: wait for
                // that GGA so satellites and altitude are not lost, unless GGA keeps not arriving.
                if let Some(rt) = self.rmc.as_ref().and_then(|r| r.time)
                    && self.gga.is_some()
                    && self.gga.as_ref().and_then(|g| g.time) != Some(rt)
                {
                    self.unmatched += 1;
                    if self.unmatched <= 2 {
                        return None;
                    }
                }
            }
            Sentence::Gga(g) => {
                if g.time.is_some() && g.time == self.rmc.as_ref().and_then(|r| r.time) {
                    self.unmatched = 0;
                }
                self.gga = Some(g);
            }
            _ => return None,
        }
        self.current()
    }

    fn current(&self) -> Option<Fix> {
        let r = self.rmc.as_ref()?;
        let (lat, lon, t, d) = (r.lat?, r.lon?, r.time?, r.date?);
        if !r.valid {
            return None;
        }
        let gga = self.gga.as_ref().filter(|g| g.time == Some(t));
        if let Some(g) = gga
            && (g.quality == 0 || g.quality == 6)
        {
            return None;
        }
        Some(Fix {
            lat,
            lon,
            time: Utc.from_utc_datetime(&d.and_time(t)),
            satellites: gga.and_then(|g| g.satellites),
            altitude_m: gga.and_then(|g| g.altitude_m),
            hdop: gga.and_then(|g| g.hdop),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a line with a correct checksum.
    pub fn line(body: &str) -> String {
        format!("${body}*{:02X}", body.bytes().fold(0u8, |a, b| a ^ b))
    }

    #[test]
    fn rmc_before_gga_keeps_satellites_and_altitude() {
        // u-blox order: RMC, VTG, GGA for each second.
        let rmc = |t: &str| line(&format!("GNRMC,{t},A,2847.40984,N,08131.63662,W,0.1,,041126,,,A,V"));
        let gga = |t: &str| line(&format!("GNGGA,{t},2847.40984,N,08131.63662,W,1,04,3.10,35.2,M,-30.1,M,,"));
        let mut b = FixBuilder::default();
        assert!(b.push_line(&gga("023800.00")).is_none(), "GGA alone is not a fix");
        assert!(b.push_line(&rmc("023801.00")).is_none(), "wait for this second's GGA");
        let f = b.push_line(&gga("023801.00")).expect("fix once GGA arrives");
        assert_eq!((f.satellites, f.altitude_m), (Some(4), Some(35.2)));
        // A receiver whose GGA stops arriving still yields a fix (without them) after two seconds.
        let mut b = FixBuilder::default();
        b.push_line(&gga("010000.00"));
        assert!(b.push_line(&rmc("010001.00")).is_none());
        assert!(b.push_line(&rmc("010002.00")).is_none());
        let f = b.push_line(&rmc("010003.00")).expect("fallback");
        assert_eq!(f.satellites, None);
    }

    #[test]
    fn parses_rmc_and_gga_from_any_talker() {
        let r = parse(&line("GNRMC,140005.00,A,3348.7260,N,08424.0660,W,0.02,,021126,,,A,V")).unwrap();
        let Sentence::Rmc(r) = r else { panic!() };
        assert!(r.valid);
        assert!((r.lat.unwrap() - 33.8121).abs() < 1e-9);
        assert!((r.lon.unwrap() + 84.4011).abs() < 1e-9);
        assert_eq!(r.date, NaiveDate::from_ymd_opt(2026, 11, 2));
        let Sentence::Gga(g) = parse(&line("GPGGA,140005.00,3348.7260,N,08424.0660,W,1,09,0.9,212.5,M,-30.0,M,,")).unwrap() else {
            panic!()
        };
        assert_eq!((g.quality, g.satellites, g.altitude_m), (1, Some(9), Some(212.5)));
    }

    #[test]
    fn rejects_bad_checksums_and_malformed_data() {
        let good = line("GPRMC,140005.00,A,3348.7260,N,08424.0660,W,0.02,,021126,,,A");
        let bad = good.replacen(",A,", ",V,", 1);
        assert_eq!(parse(&bad), Err(NmeaError::BadChecksum));
        assert_eq!(parse("$GPRMC,140005.00,A,3348.7260,N"), Err(NmeaError::BadChecksum), "no checksum");
        assert_eq!(parse("garbage"), Err(NmeaError::NotNmea));
        assert!(matches!(parse(&line("GPRMC,140005.00,A,9948.7260,N,08424.0660,W,0,,021126,,")), Err(NmeaError::Malformed(_))));
        assert!(matches!(parse(&line("GPRMC,140005.00,A,3348.7260,Q,08424.0660,W,0,,021126,,")), Err(NmeaError::Malformed(_))));
        assert!(matches!(parse(&line("GPRMC,140005.00,A,3348.7260,N,08424.0660,W,0,,321326,,")), Err(NmeaError::Malformed(_))));
    }

    #[test]
    fn only_valid_fixes_are_reported() {
        let mut b = FixBuilder::default();
        // Cold start: void RMC, no-fix GGA.
        assert!(b.push_line(&line("GPRMC,135959.00,V,,,,,,,021126,,,N")).is_none());
        assert!(b.push_line(&line("GPGGA,135959.00,,,,,0,00,99.9,,,,,,")).is_none());
        // Coordinates present but status V: still no fix.
        assert!(b.push_line(&line("GPRMC,140000.00,V,3348.7260,N,08424.0660,W,0,,021126,,,N")).is_none());
        // GGA says no fix for the same second as a valid RMC: vetoed.
        b.push_line(&line("GPGGA,140001.00,3348.7260,N,08424.0660,W,0,03,9.9,200.0,M,,,,"));
        assert!(b.push_line(&line("GPRMC,140001.00,A,3348.7260,N,08424.0660,W,0,,021126,,,A")).is_none());
        // Valid pair.
        b.push_line(&line("GPGGA,140002.00,3348.7260,N,08424.0660,W,1,09,0.9,212.5,M,-30.0,M,,"));
        let fix = b.push_line(&line("GPRMC,140002.00,A,3348.7260,N,08424.0660,W,0,,021126,,,A")).unwrap();
        assert_eq!(fix.time.to_rfc3339(), "2026-11-02T14:00:02+00:00");
        assert_eq!((fix.satellites, fix.altitude_m), (Some(9), Some(212.5)));
        // A corrupted line is counted, not used.
        assert!(b.push_line("$GPRMC,140003.00,A,0000.0000,N,00000.0000,E,0,,021126,,,A*00").is_none());
        assert_eq!(b.rejected, 1);
    }

    #[test]
    fn rmc_only_receivers_still_give_a_fix() {
        let mut b = FixBuilder::default();
        let f = b.push_line(&line("GPRMC,235959.50,A,0001.0000,S,00001.0000,E,0,,311226,,,D")).unwrap();
        assert!((f.lat + 1.0 / 60.0).abs() < 1e-9);
        assert_eq!(f.time.to_rfc3339(), "2026-12-31T23:59:59.500+00:00");
        assert_eq!(f.satellites, None, "unknown, not invented");
    }
}
