"""Independent validation of EMCOMM LOG export fixtures with third-party parsers.

Usage: python scripts/validate_exports.py <fixture-dir>
Fixtures are produced by:  EMCOMM_FIXTURE_OUT=<dir> cargo test -p emcomm-core --test exports write_fixture
"""

import csv
import io
import sys
from pathlib import Path

import adif_io
import docx
import openpyxl
from pypdf import PdfReader

PREAMBLE = 7
failures = []


def check(cond, msg):
    print(("PASS " if cond else "FAIL ") + msg)
    if not cond:
        failures.append(msg)


def main(d: Path):
    # CSV (Python stdlib csv, utf-8-sig strips the BOM)
    text = (d / "fixture.csv").read_text(encoding="utf-8-sig")
    rows = list(csv.reader(io.StringIO(text, newline="")))
    header = rows[PREAMBLE]
    data = rows[PREAMBLE + 1 :]
    check(header[:4] == ["Seq", "Status", "Event Time", "Event Time UTC"], "csv: stable header prefix")
    check(len(data) == 5, f"csv: 5 records incl. voided (got {len(data)})")
    check(all(len(r) == len(header) for r in data), "csv: every row has every column")
    check([r[0] for r in data] == ["1", "2", "3", "4", "5"], "csv: chronological order")
    check(data[1][13].startswith("'="), "csv: formula neutralized with apostrophe")
    long = data[2][14]
    check(long.startswith("START-OF-LONG-MESSAGE") and long.endswith("END-OF-LONG-MESSAGE"), "csv: long text intact")
    check(data[3][1] == "void", "csv: voided status column")

    # XLSX (openpyxl)
    wb = openpyxl.load_workbook(d / "fixture.xlsx")
    ws = wb["Entries"]
    vals = list(ws.iter_rows(values_only=True))
    check(len(vals) == 6, f"xlsx: header + 5 rows (got {len(vals)})")
    check(ws.freeze_panes == "A2", f"xlsx: frozen header ({ws.freeze_panes})")
    check(ws.auto_filter.ref is not None and ws.auto_filter.ref.startswith("A1:"), f"xlsx: autofilter {ws.auto_filter.ref}")
    import datetime

    check(isinstance(vals[1][3], datetime.datetime), f"xlsx: typed UTC datetime cell ({type(vals[1][3]).__name__})")
    formula_cell = ws.cell(row=3, column=14)
    check(formula_cell.data_type == "s" and formula_cell.value.startswith("=HYPERLINK"), "xlsx: formula-like text stored as text")
    check(formula_cell.quotePrefix if hasattr(formula_cell, "quotePrefix") else formula_cell.style_array.quotePrefix, "xlsx: quote prefix style applied")
    check("Report" in wb.sheetnames, "xlsx: report metadata sheet")
    check(str(vals[3][14]).endswith("END-OF-LONG-MESSAGE"), "xlsx: long text intact")

    # DOCX (python-docx)
    doc = docx.Document(d / "fixture.docx")
    full = "\n".join(p.text for p in doc.paragraphs)
    for s in ["#0001", "#0002", "#0003", "#0004", "#0005", "END-OF-LONG-MESSAGE", "VOIDED", "Scope:", "Generated:"]:
        check(s in full, f"docx: contains {s}")
    check("日本" in full, "docx: unicode preserved")
    pdoc = docx.Document(d / "paging.docx")
    check(sum(1 for p in pdoc.paragraphs if p.style.name.lower() == "heading 2") == 300, "docx: 300 entry headings in paging fixture")

    # PDF (pypdf text extraction through the embedded ToUnicode map)
    r = PdfReader(d / "fixture.pdf")
    ptext = "\n".join(p.extract_text() for p in r.pages)
    check("EMCOMM LOG" in ptext and "Export Fixture" in ptext, "pdf: title extractable")
    check("START-OF-LONG-MESSAGE" in ptext and "END-OF-LONG-MESSAGE" in ptext, "pdf: long message start and end present")
    check("ünïcödé" in ptext, "pdf: unicode extractable")
    check("Page 1 of" in ptext, "pdf: page numbering")
    big = PdfReader(d / "paging.pdf")
    n = len(big.pages)
    check(n > 5, f"pdf: paging fixture has {n} pages")
    heads = sum(1 for p in big.pages if "Summary, message and details" in p.extract_text())
    check(heads == n, f"pdf: column headings repeated on all {n} pages")
    last = big.pages[-1].extract_text()
    check("End of report - 300 record(s)." in last, "pdf: end-of-report count")
    for i in (0, 149, 299):
        check(any(f"Paging entry {i}" in p.extract_text() for p in big.pages), f"pdf: entry {i} present")

    # ADIF (adif-io)
    qsos, header = adif_io.read_from_file(str(d / "fixture.adi"))
    check(header.get("ADIF_VER") == "3.1.8", f"adif: version header {header.get('ADIF_VER')}")
    check(len(qsos) == 4, f"adif: 4 eligible QSOs (got {len(qsos)})")
    calls = sorted(q["CALL"] for q in qsos)
    check(calls == ["JA1AAA", "K1ABC/P", "VE3XYZ", "W1AW"], f"adif: calls {calls}")
    for q in qsos:
        t = adif_io.time_on(q)
        check(t.tzinfo is not None and t.year == 2026, f"adif: {q['CALL']} time_on parses as UTC {t.isoformat()}")

    # 2.1: POTA activator preset (with precise coordinates) and the default privacy policy.
    qsos, header = adif_io.read_from_file(str(d / "pota.adi"))
    check(len(qsos) == 2, f"pota: 2 activation QSOs (got {len(qsos)})")
    by = {q["CALL"]: q for q in qsos}
    w = by.get("W1AW", {})
    check(w.get("MY_SIG") == "POTA" and w.get("MY_SIG_INFO") == "US-0005", "pota: MY_SIG/MY_SIG_INFO")
    check(w.get("SIG") == "POTA" and w.get("SIG_INFO") == "K-1234", "pota: park-to-park SIG/SIG_INFO")
    check(w.get("MY_POTA_REF") == "US-0005" and w.get("POTA_REF") == "K-1234", "pota: MY_POTA_REF/POTA_REF")
    check(w.get("MY_LAT") == "N033 48.726" and w.get("MY_LON") == "W084 24.066", "pota: MY_LAT/MY_LON location format")
    dl = by.get("DL1ABC", {})
    check(dl.get("QSO_DATE") == "20261103" and dl.get("TIME_ON") == "000001", "pota: after-midnight QSO keeps its UTC date")
    check(adif_io.time_on(w).isoformat() == "2026-11-02T23:59:59+00:00", "pota: before-midnight QSO time_on")
    with open(d / "private.csv", newline="", encoding="utf-8-sig") as f:
        text = f.read()
    check("33.812100" not in text and "-84.401100" not in text, "privacy: CSV withholds coordinates by default")
    check("EM73wv12" not in text and "EM73wv" in text, "privacy: CSV grid cut to 6 characters")

    print(f"\n{len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1])))
