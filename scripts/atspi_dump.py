"""Print the AT-SPI accessibility tree (what Orca reads) of the EMCOMM LOG window as JSON lines.

Runs with the system python3 (python3-pyatspi). Usage: python3 scripts/atspi_dump.py
"""

import json
import sys

import pyatspi

out = []


def walk(node, depth=0):
    if depth > 60 or node is None:
        return
    try:
        out.append({"role": node.getRoleName(), "name": node.name or "", "depth": depth})
        for i in range(node.childCount):
            walk(node.getChildAtIndex(i), depth + 1)
    except Exception:  # noqa: BLE001 - nodes can disappear while walking
        pass


desktop = pyatspi.Registry.getDesktop(0)
for i in range(desktop.childCount):
    app = desktop.getChildAtIndex(i)
    if app is None:
        continue
    for j in range(app.childCount):
        win = app.getChildAtIndex(j)
        if win is not None and "EMCOMM LOG" in (win.name or ""):
            walk(win)
json.dump(out, sys.stdout)
