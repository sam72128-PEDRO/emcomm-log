# Genuine EMCOMM LOG 2.1 fixtures (schema 4)

Written on 2026-11-04 by the 2.1.0 code itself (tag `v2.1.0`), not by 2.2: a throwaway test in the
2.1 worktree created these logbooks through the public 2.1 API, then the WAL was checkpointed and the
lock file removed so they look like a cleanly closed 2.1 logbook.

- `V21_HAM_Fixture--*`: station (tactical ID, W4CLB, US-0005, default position), Alice's shift then Bob's,
  three QSOs (one with station position, POTA refs and a JPEG attachment; one voided; one after the
  handover with a reported location).
- `V21_EMCOMM_Fixture--*`: one operational period, six Message entries in it, one flagged for report, one
  favorite.
- `v21_ham.emlog`: 2.1 package of the HAM logbook, exported by 2.1 after all three QSOs.

Tests copy these folders before opening them; the committed files are never modified.
