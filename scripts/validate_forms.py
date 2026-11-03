"""Independent check of EMCOMM LOG's ICS form exports with pypdf (not the library that wrote them).

Usage: python scripts/validate_forms.py <dir with ics213.pdf, ics214.pdf, ics309.pdf>
Written by: EMCOMM_FORM_OUT=<dir> cargo test -p emcomm-core --test forms write_form_fixtures -- --ignored
"""

import sys
from pathlib import Path

from pypdf import PdfReader

d = Path(sys.argv[1])
failures = 0


def check(name: str, ok: bool, detail: str = "") -> None:
    global failures
    print(("PASS " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""), flush=True)
    failures += 0 if ok else 1


def values(path: Path) -> dict:
    fields = PdfReader(path).get_fields() or {}
    return {k: v.get("/V") for k, v in fields.items() if v.get("/V")}


r213 = PdfReader(d / "ics213.pdf")
v213 = values(d / "ics213.pdf")
check("213: one page (instructions dropped)", len(r213.pages) == 1, str(len(r213.pages)))
check("213: AcroForm kept (fields editable)", "/AcroForm" in r213.trailer["/Root"])
check("213: subject with Unicode", v213.get("4 Subject") == "Shelter supply request ✓", repr(v213.get("4 Subject")))
check("213: From with accents", v213.get("3 From Name and Position") == "José Núñez, EOC Radio Operator", repr(v213.get("3 From Name and Position")))
check("213: approved by", v213.get("8 Approved by Name") == "R. Smith")
check("213: reply", v213.get("9 Reply") == "Approved. Truck 12 dispatched.")
check("213: no signature filled", not any(k.startswith("Signature") for k in v213))
check("213: footer present", "Prepared with EMCOMM LOG" in (r213.pages[0].extract_text() or ""))

v214 = values(d / "ics214.pdf")
check("214: operational period", (v214.get("Date From"), v214.get("Time From")) == ("2026-11-02", "12:00Z"), f"{v214.get('Date From')} {v214.get('Time From')}")
check("214: resource row", v214.get("NameRow1_3") == "José Núñez", repr(v214.get("NameRow1_3")))
check("214: activity row", str(v214.get("Notable ActivitiesRow1", "")).startswith("Opened shelter net"), repr(v214.get("Notable ActivitiesRow1")))
check("214: no signature filled", not any(k.startswith("Signature") for k in v214))

t309 = "\n".join(p.extract_text() or "" for p in PdfReader(d / "ics309.pdf").pages)
check("309: title", "COMMUNICATIONS LOG (ICS 309)" in t309)
check("309: net name", "Lake County ARES Net" in t309)
check("309: row from call sign", "KJ4ABC" in t309)
check("309: message text", "Radio check" in t309 and "Shelter supply request" in t309)
check("309: Unicode period name", "OP 1 ✓" in t309)

print(f"{'All' if not failures else failures} form checks {'passed' if not failures else 'FAILED'}")
sys.exit(1 if failures else 0)
