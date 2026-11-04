# GPS setup (EMCOMM LOG 2.1, desktop)

EMCOMM LOG takes **one position when you ask for it**: **Capture GPS…** on an entry's *Station position*,
or **Test GPS** in Settings. It never tracks, never runs in the background, never sends anything to the
receiver, and never uses the network. Positions are stored only in your logbook.

## Sources

| Source | Where | What you need |
|---|---|---|
| **USB GPS receiver** (NMEA 0183) | Windows, macOS, Linux | A receiver that appears as a serial port (most USB "GPS mice": u-blox, SiRF, Prolific/CH340/FTDI adapters). |
| **Bluetooth GPS receiver** (NMEA over SPP) | Windows, macOS, Linux | Pair the receiver in the operating system first so it gets a serial port (below). |
| **gpsd** | Linux (any OS if you run gpsd yourself) | A running `gpsd` on this computer. Only loopback addresses are accepted (`127.0.0.1`, `::1`, `localhost`). |
| **Built-in GNSS** | Windows laptops/tablets with a GNSS chip | Reported by Windows location; marked *Internal GNSS* only when Windows says the position came from satellites. |
| **Computer's location** (fallback) | macOS Location Services, Windows location, Linux GeoClue | May come from Wi-Fi or IP rather than satellites, and is labelled *OS position* so nobody mistakes it for a GPS fix. On a computer with no network it usually has nothing to give. |

**Settings → Location & GPS** chooses the preferred source. *Auto* tries, in order: a configured serial
receiver, then gpsd (Linux), then the computer's location. The capture dialog always lets you pick another source.

### Serial ports by OS

- **macOS**: USB receivers appear as `/dev/cu.usbserial-…` or `/dev/cu.usbmodem…`. Bluetooth: pair in
  *System Settings → Bluetooth*; the receiver appears as `/dev/cu.<name>`. (EMCOMM LOG lists the `cu.`
  device only; the matching `tty.` device is the same receiver.) Some adapters need the maker's driver.
- **Windows**: USB receivers appear as `COMn` (Device Manager → Ports). Bluetooth: pair in *Settings →
  Bluetooth & devices*, then *More Bluetooth settings → COM Ports* shows an **Outgoing** port — use that one.
- **Linux**: USB receivers are usually `/dev/ttyUSB0` or `/dev/ttyACM0`. Your user must be in the group that
  owns the device (`dialout` on Debian/Ubuntu, `uucp` on Arch/Fedora): `sudo usermod -aG dialout $USER`, then
  log out and back in. Bluetooth: pair, then bind an RFCOMM port, e.g. `sudo rfcomm bind 0 <MAC> 1` →
  `/dev/rfcomm0`. If **gpsd** is already using the receiver, use the *gpsd* source instead (the port is busy).

### Speed (baud)

Leave **Baud rate = Detect (4800, then 9600)**: EMCOMM LOG listens at 4800 and then 9600, which covers almost all receivers. Set
a fixed rate (4800–115200) only if your receiver's manual says so. EMCOMM LOG only listens; it never
reconfigures the receiver.

## What counts as a fix

- Every NMEA sentence must have a valid checksum; others are counted as *rejected* and ignored.
- A position is accepted only when the receiver itself says it is valid (RMC status `A` and a non-`N` mode,
  GGA fix quality ≥ 1). A receiver still searching for satellites gives **No fix yet**, never a guess.
- The fix must be **no older than 10 s**. An older one is shown as **Old fix** with its age, and you
  may still choose *Use this old position anyway*.
- Each attempt gives up after **30 s** (you can cancel sooner). A receiver just switched on may need longer
  outdoors: try again.
- **Accuracy** is shown only when the source reports it in metres (gpsd, OS location). It is never
  calculated from HDOP; otherwise the dialog says *accuracy unknown*.
- The grid square (Maidenhead) is calculated on this computer from the coordinates.

## Results you may see

| Result | Meaning / what to do |
|---|---|
| **Fix received** | Accept it with *Use this position*. Source, fix time, satellites and accuracy (if known) are kept with the entry. |
| **Old fix** | The receiver's last fix is stale. Wait for a fresh one, or accept it knowingly. |
| **No fix yet** | Valid data but no satellite fix. Move to open sky and retry. |
| **Receiver not found** | Check the cable or pairing, then the port in the list. |
| **Receiver busy** | Another program (often gpsd, or a mapping program) has the port open. Close it or use gpsd. |
| **Permission needed** | Linux: join the `dialout`/`uucp` group. macOS: allow EMCOMM LOG in *Privacy & Security → Location Services*. Windows: *Settings → Privacy & security → Location*. |
| **Unreadable data** | Data arrived but no sentence had a valid checksum — usually the wrong baud rate. Try *Detect* or the rate from the manual. |
| **Receiver disconnected** | The device went away during the attempt. |
| **Timed out** | Nothing usable within 30 s. |
| **Not available** | That source does not exist on this computer (e.g. no GeoClue, location turned off). |

Whatever happens, you can always save the entry without a position or type one by hand
(decimal degrees, or e.g. `33.8121 N`, `84.4011 W`).

## Privacy

Positions stay in the logbook. Reports, CSV/XLSX/DOCX/TXT/PDF and ADIF **leave out latitude/longitude by
default** and shorten grid squares to 6 characters; tick *Include precise coordinates* on the Reports screen
only when the recipient needs exact positions. A `.emlog` package is a complete copy of the logbook and does
contain them. See the operator guide, section "Locations and privacy".

## Tested hardware

| Receiver / source | OS | Status |
|---|---|---|
| Recorded NMEA (u-blox-style RMC/GGA/GSA, checksum errors, no-fix, stale) | all | **passing** (automated fixtures, `core/src/gps`) |
| Pseudo-terminal serial port streaming NMEA | Linux | **passing** (automated) |
| gpsd JSON (TPV, loopback; non-loopback refused) | all | **passing** (automated, simulated gpsd on 127.0.0.1) |
| macOS Location Services | macOS 14 | **pending** — compiled; permission prompt not yet exercised by hand |
| Windows location / built-in GNSS | Windows 10/11 | **pending** — compiled in CI only |
| GeoClue2 | Linux | **pending** — compiled; no desktop session tested |
| Physical USB GPS receiver | any | **pending** — no receiver available during 2.1 development |
| Physical Bluetooth GPS receiver | any | **pending** |
| Real gpsd with a receiver | Linux | **pending** |

Hardware rows stay **pending** until someone runs them with real equipment; please report the receiver
model, OS and result.
