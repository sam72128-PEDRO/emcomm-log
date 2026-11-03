"""End-to-end test of the INSTALLED EMCOMM LOG app via tauri-driver (WebDriver).

Covers A01 (first launch blocks use), A03/A07 through the UI, A14 (keyboard shortcuts, Tab order,
Esc and focus restoration, shortcuts blocked in dialogs, screen-reader live regions, native menus)
and an axe-core accessibility audit of every screen.

Usage: python scripts/e2e.py <app-binary> <data-dir> <work-dir>
Requires tauri-driver listening on 127.0.0.1:4444 started with EMCOMM_DATA_DIR=<data-dir>.
"""

import json
import os
import platform
import sqlite3
import subprocess
import sys
import time
import uuid
from pathlib import Path

from selenium import webdriver
from selenium.common.exceptions import TimeoutException
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.options import ArgOptions
from selenium.webdriver.support.ui import WebDriverWait

APP, DATA, WORK = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
AXE = (Path(__file__).resolve().parent.parent / "node_modules/axe-core/axe.min.js").read_text(encoding="utf-8")
SHOTS = WORK / "shots"
SHOTS.mkdir(parents=True, exist_ok=True)
results: list[tuple[str, bool, str]] = []
axe_report: dict = {}


def check(name: str, ok: bool, detail: str = "") -> None:
    results.append((name, bool(ok), detail))
    print(("PASS " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""), flush=True)


def session() -> webdriver.Remote:
    opts = ArgOptions()
    opts.set_capability("browserName", "wry")
    opts.set_capability("tauri:options", {"application": str(APP)})
    d = webdriver.Remote("http://127.0.0.1:4444", options=opts)
    d.implicitly_wait(0)
    return d


def body(d) -> str:
    # Read text in one script call: element handles go stale while Svelte re-renders.
    return d.execute_script("return document.body ? document.body.innerText : ''") or ""


def wait_text(d, text: str, timeout: float = 20) -> bool:
    try:
        WebDriverWait(d, timeout).until(lambda x: text in body(x))
        return True
    except TimeoutException:
        return False


def end(d) -> None:
    """Quit the session and make sure the app and its webview processes are gone, so the next
    session can reuse the WebView2/WebKit profile."""
    try:
        d.quit()
    except Exception:  # noqa: BLE001
        pass
    time.sleep(2)
    if platform.system() == "Windows":
        subprocess.run(["taskkill", "/F", "/T", "/IM", APP.name], capture_output=True)
        subprocess.run(["taskkill", "/F", "/IM", "msedgewebview2.exe"], capture_output=True)
    else:
        subprocess.run(["pkill", "-f", str(APP)], capture_output=True)
    time.sleep(3)


def chord(d, key: str) -> None:
    ActionChains(d).key_down(Keys.CONTROL).send_keys(key).key_up(Keys.CONTROL).perform()


def active_id(d) -> str:
    return d.execute_script("return document.activeElement ? document.activeElement.id : ''") or ""


def live_text(d) -> str:
    return d.execute_script(
        "return [...document.querySelectorAll('[aria-live]')].map(e => e.textContent).join(' | ')"
    )


def axe(d, screen: str) -> None:
    d.execute_script(AXE)
    res = d.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "axe.run(document, {resultTypes: ['violations']}).then(r => done(r.violations.map(v => "
        "({id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.slice(0, 5).map(n => n.target.join(' '))}))))"
        ".catch(e => done([{id: 'axe-error', impact: 'critical', help: String(e), nodes: []}]));"
    )
    axe_report[screen] = res
    serious = [v for v in res if v["impact"] in ("serious", "critical")]
    check(f"axe: no serious/critical violations on {screen}", not serious, json.dumps(serious)[:900])


def shot(d, name: str) -> None:
    try:
        d.save_screenshot(str(SHOTS / f"{name}.png"))
    except Exception:  # noqa: BLE001 - screenshots are best effort evidence
        pass


def native_menu(menu: str, item: str, downs: int) -> bool:
    """Choose a native menu item through OS automation (not the webview)."""
    system = platform.system()
    try:
        if system == "Windows":
            from pywinauto import Application

            app = Application(backend="win32").connect(title_re="EMCOMM LOG.*", timeout=15)
            app.top_window().menu_select(f"{menu}->{item}")
            return True
        if system == "Linux":
            wid = subprocess.run(["xdotool", "search", "--name", "EMCOMM LOG"], capture_output=True, text=True).stdout.split()
            if not wid:
                return False
            w = wid[-1]
            subprocess.run(["xdotool", "windowfocus", "--sync", w], check=False)
            # F10 opens the GTK menubar on File; Right x2 -> View; Down to the item.
            keys = ["F10", "Right", "Right"] + ["Down"] * downs + ["Return"]
            for k in keys:
                subprocess.run(["xdotool", "key", "--window", w, k], check=True)
                time.sleep(0.25)
            return True
    except Exception as e:  # noqa: BLE001
        print(f"native menu automation error: {e}", flush=True)
    return False


def os_accessibility_tree() -> list[dict]:
    """Accessibility tree exposed to screen readers by the OS: UI Automation on Windows
    (Narrator/NVDA), AT-SPI on Linux (Orca)."""
    system = platform.system()
    try:
        if system == "Windows":
            from pywinauto import Desktop

            win = Desktop(backend="uia").window(title_re="EMCOMM LOG.*")
            return [
                {"role": e.element_info.control_type or "", "name": e.element_info.name or ""}
                for e in win.descendants()
            ]
        if system == "Linux":
            r = subprocess.run(["/usr/bin/python3", str(Path(__file__).with_name("atspi_dump.py"))], capture_output=True, text=True, timeout=60)
            if r.returncode != 0:
                print(r.stderr[-2000:], flush=True)
            return json.loads(r.stdout or "[]")
    except Exception as e:  # noqa: BLE001
        print(f"accessibility tree error: {e}", flush=True)
    return []


def check_screen_reader_tree(expect: list[tuple[str, str]]) -> None:
    tree = os_accessibility_tree()
    (WORK / f"a11y-tree-{platform.system().lower()}.json").write_text(json.dumps(tree, indent=1))
    check("A14 OS accessibility tree exposed to screen readers", len(tree) > 20, f"{len(tree)} nodes")
    for role_hint, name in expect:
        found = any(name.lower() in n["name"].lower() and role_hint.lower() in n["role"].lower() for n in tree)
        check(f"A14 screen reader sees {role_hint} '{name}'", found)


# ---------------- Phase A: fresh install, first-run wizard (A01) ----------------
d = session()
check("A01 wizard shown on first launch", wait_text(d, "Welcome to EMCOMM LOG"))
shot(d, "01-wizard")
check("A01 normal use blocked (no navigation)", "Traffic Log" not in body(d))
axe(d, "wizard")
d.find_element(By.XPATH, "//button[normalize-space()='Continue']").click()
time.sleep(0.4)
check("A01 empty operator name rejected", "Operator name is required" in body(d))
check("A01 invalid field marked aria-invalid", d.find_element(By.ID, "wz-name").get_attribute("aria-invalid") == "true")
check("A14 focus moved to the invalid field", active_id(d) == "wz-name")
d.find_element(By.ID, "wz-name").send_keys("E2E Operator")
d.find_element(By.XPATH, "//button[normalize-space()='Continue']").click()
check("A01 wizard advances after valid input", wait_text(d, "Time display", 5))
end(d)

# Folder pickers are native OS dialogs that WebDriver cannot drive, so complete setup by writing
# the same records the wizard writes (profile + settings) into the app's own database.
logs, backups = WORK / "logbooks", WORK / "backups"
logs.mkdir(exist_ok=True)
backups.mkdir(exist_ok=True)
db = sqlite3.connect(DATA / "app.sqlite")
pid = str(uuid.uuid4())
now = "2026-11-03T00:00:00.000Z"
db.execute(
    "INSERT INTO profiles(id,name,callsign,license_class,organization,position,grid,active,created_at_utc,updated_at_utc) "
    "VALUES (?,?,?,?,?,?,?,1,?,?)",
    (pid, "E2E Operator", "KJ4MRC", "", "Test Org", "Comm Unit", "", now, now),
)
settings = {
    "schema_version": 1,
    "wizard_completed": True,
    "time_display": "utc",
    "clock": "24h",
    "theme": "dark",
    "default_logbook_parent": str(logs),
    "backup": {"destination": str(backups), "interval_minutes": 15, "on_close": True, "keep_recent": 8, "keep_daily": 7, "keep_weekly": 4},
    "active_profile_id": pid,
    "default_preset_ids": {},
}
db.execute("INSERT OR REPLACE INTO app_settings(id,settings_json,updated_at_utc) VALUES (1,?,?)", (json.dumps(settings), now))
db.commit()
db.close()

# ---------------- Phase B: logging workflow and keyboard/a11y (A03, A07, A14) ----------------
d = session()
check("Home shown after setup", wait_text(d, "Local logbooks"))
axe(d, "home")
create = d.find_element(By.XPATH, "//button[normalize-space()='Create Logbook']")
create.click()
check("Create dialog opened", wait_text(d, "Create logbook", 5))
in_dialog = d.execute_script("return !!document.activeElement.closest('[role=dialog]')")
check("A14 focus moves into dialog", in_dialog)
axe(d, "create-logbook-dialog")
ActionChains(d).send_keys(Keys.ESCAPE).perform()
time.sleep(0.4)
check("A14 Esc closes dialog", not d.find_elements(By.CSS_SELECTOR, "[role=dialog]"))
check(
    "A14 focus restored to opener after Esc",
    d.execute_script("return document.activeElement && document.activeElement.textContent.includes('Create Logbook')"),
)
create.click()
wait_text(d, "Create logbook", 5)
d.find_element(By.XPATH, "//label[.//strong[normalize-space()='Emergency']]").click()
time.sleep(0.3)
d.find_element(By.ID, "cl-name").send_keys("E2E Storm")
d.find_element(By.ID, "cl-incident_name").send_keys("E2E Incident")
d.find_element(By.XPATH, "//button[normalize-space()='Create and open']").click()
check("Emergency logbook created and opened", wait_text(d, "E2E Incident"))
shot(d, "02-dashboard")
axe(d, "dashboard")

chord(d, "2")
check("A14 Ctrl+2 opens Quick Entry", wait_text(d, "Quick Entry – Add Traffic", 5))
time.sleep(0.5)
axe(d, "quick-entry")
check("A14 Quick Entry autofocuses From", active_id(d) == "ed-new-from", active_id(d))
order = []
for _ in range(4):
    ActionChains(d).send_keys(Keys.TAB).perform()
    order.append(active_id(d))
check_screen_reader_tree(
    [
        ("edit" if platform.system() == "Windows" else "entry", "Summary"),
        ("edit" if platform.system() == "Windows" else "entry", "From / Station"),
        ("button", "Save Entry"),
        ("button", "Attach file"),
        ("combo", "Priority"),
    ]
)
check("A14 Tab order From→To→Channel→Handling→Summary", order == ["ed-new-to", "ed-new-channel", "ed-new-handling", "ed-new-subject"], str(order))
d.find_element(By.ID, "ed-new-subject").send_keys("Bridge on CR-12 blocked")
chord(d, Keys.ENTER)
check("A03 Ctrl+Enter saves: ENTRY SAVED #1 shown", wait_text(d, "ENTRY SAVED #1", 10))
check("A14 save announced to screen readers", "Entry Saved" in live_text(d), live_text(d))
check("A14 focus returns to first field for next entry", active_id(d) == "ed-new-from", active_id(d))
d.find_element(By.ID, "ed-new-subject").send_keys("Welfare check 1247 Pine Rd")
d.find_element(By.ID, "ed-new-subject").click()
chord(d, Keys.ENTER)
check("A03 second entry saved as #2", wait_text(d, "ENTRY SAVED #2", 10))
d.find_element(By.ID, "ed-new-subject").click()
chord(d, Keys.ENTER)
time.sleep(0.6)
check("Empty summary blocked with message", "Summary is required" in body(d))
check("A14 validation error announced", "Summary is required" in live_text(d), live_text(d))
shot(d, "03-quick-entry")

chord(d, "3")
check("A14 Ctrl+3 opens Traffic Log", wait_text(d, "Showing 1–2 of 2 entries", 8))
axe(d, "traffic-log")
chord(d, "f")
time.sleep(0.4)
check("A14 Ctrl+F focuses search", d.execute_script("return document.activeElement.type") == "search")
rows = d.find_elements(By.CSS_SELECTOR, "tr[data-row]")
rows[-1].click()
check("Entry detail opens", wait_text(d, "Entry #0001", 5))
axe(d, "entry-detail")
chord(d, "1")
time.sleep(0.5)
check("A14 shortcuts blocked while dialog open", "not available while a dialog is open" in live_text(d), live_text(d))
check("A14 screen unchanged under dialog", "Entry #0001" in body(d))
# Void through the UI (A07)
d.find_element(By.XPATH, "//button[normalize-space()='Void']").click()
wait_text(d, "Void entry #1?", 5)
d.find_element(By.ID, "confirm-input").send_keys("e2e void")
d.find_element(By.XPATH, "//button[normalize-space()='Void entry']").click()
check("A07 void recorded as revision 2", wait_text(d, "Rev 2 · Voided", 8))
ActionChains(d).send_keys(Keys.ESCAPE).perform()
time.sleep(0.5)
check("A07 default list hides voided entry", wait_text(d, "Showing 1–1 of 1 entries", 5))

chord(d, "5")
check("A14 Ctrl+5 opens Reports", wait_text(d, "Reports & Export", 5))
axe(d, "reports")
chord(d, "4")
check("A14 Ctrl+4 opens Messages", wait_text(d, "Needs action", 5))
axe(d, "messages")
chord(d, ",")
check("A14 Ctrl+, opens Settings", wait_text(d, "Operator Profiles", 5))
axe(d, "settings")

# Native menu (OS automation, not the webview): View → Traffic Log
ok = native_menu("View", "Traffic Log", downs=3)
check("A14 native menu View→Traffic Log works", ok and wait_text(d, "Traffic Log", 6) and "Showing 1–1 of 1 entries" in body(d))
shot(d, "04-after-native-menu")
end(d)

# ---------------- Phase C: restart retains exact data (A03) ----------------
d = session()
wait_text(d, "Local logbooks")
d.find_element(By.XPATH, "//button[normalize-space()='Open']").click()
check("Logbook reopens after restart", wait_text(d, "E2E Incident", 10))
chord(d, "3")
check("A03 entries survive restart without any save step", wait_text(d, "Welfare check 1247 Pine Rd", 8))
end(d)

(WORK / "axe-report.json").write_text(json.dumps(axe_report, indent=2))
failed = [r for r in results if not r[1]]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed", flush=True)
sys.exit(1 if failed else 0)
