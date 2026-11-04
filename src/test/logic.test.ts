import { describe, expect, it } from "vitest";
import {
  parseFrequencyHz,
  bandForHz,
  newForm,
  toEntryInput,
  applyTemplate,
  allowedKinds,
  validateForm,
} from "../lib/entryForm";
import { fromInputValue, toInputValue, formatInstant, todayBoundsUtc, offsetLabel } from "../lib/time";
import { keyToAction } from "../lib/actions";
import { preset } from "./fixtures";
import { currentPeriod } from "../lib/periods";
import { blankChannel, channelHint, channelValue } from "../lib/plan";

describe("frequency parsing", () => {
  it("parses exact decimals without floating point error", () => {
    expect(parseFrequencyHz("146.52", "MHz").hz).toBe(146_520_000);
    expect(parseFrequencyHz("14.074", "MHz").hz).toBe(14_074_000);
    expect(parseFrequencyHz("7200", "kHz").hz).toBe(7_200_000);
    expect(parseFrequencyHz("1.2965", "GHz").hz).toBe(1_296_500_000);
    expect(parseFrequencyHz("", "MHz").hz).toBeNull();
    expect(parseFrequencyHz("abc", "MHz").error).toBeTruthy();
    expect(parseFrequencyHz("146.5200001", "MHz").error).toBeTruthy();
    expect(bandForHz(146_520_000)).toBe("2m");
  });
});

describe("time conversion", () => {
  it("round-trips UTC input values exactly", () => {
    const iso = fromInputValue("2026-11-02T19:37:05", "utc");
    expect(iso).toBe("2026-11-02T19:37:05.000Z");
    expect(toInputValue(iso, "utc")).toBe("2026-11-02T19:37:05");
  });
  it("formats with an unambiguous zone indicator", () => {
    expect(formatInstant("2026-11-02T19:37:05.000Z", "utc", "24h")).toBe("2026-11-02 19:37Z");
    expect(formatInstant("2026-11-02T19:37:05.000Z", "utc", "12h", { seconds: true })).toBe(
      "2026-11-02 7:37:05 PM UTC",
    );
    expect(formatInstant("2026-11-02T19:37:05.000Z", "local", "24h")).toMatch(/[+-]\d\d:\d\d$/);
    expect(offsetLabel(-300)).toBe("UTC-05:00");
  });
  it("computes today's UTC bounds", () => {
    const [a, b] = todayBoundsUtc("utc", new Date("2026-11-02T23:59:00Z"));
    expect(a).toBe("2026-11-02T00:00:00.000Z");
    expect(b).toBe("2026-11-03T00:00:00.000Z");
  });
});

describe("entry form", () => {
  const ctx = (type: "general" | "ham" | "emergency") => ({
    type,
    preset,
    profile: null,
    incident: {
      incident_name: "Storm",
      incident_number: "",
      operational_period: "OP1",
      agency: "",
      location: "",
    },
  });
  it("only sends type-specific sections for that logbook type", () => {
    const g = newForm(ctx("general"));
    g.subject = "x";
    const gi = toEntryInput(g, "general", "utc");
    expect(gi.ham).toBeNull();
    expect(gi.emergency).toBeNull();
    expect(gi.event_timezone).toBe("UTC");
    const h = newForm(ctx("ham"));
    h.ham.their_call = "W1AW";
    h.freqText = "14.074";
    const hi = toEntryInput(h, "ham", "utc");
    expect(hi.ham?.frequency_hz).toBe(14_074_000);
    expect(hi.ham?.qso_start_utc).toBe(h.event_at_utc);
    const e = newForm(ctx("emergency"));
    expect(toEntryInput(e, "emergency", "utc").emergency?.incident_name).toBe("Storm");
  });
  it("templates never carry timestamps", () => {
    const f = newForm(ctx("emergency"));
    const before = f.event_at_utc;
    const t = applyTemplate(
      f,
      {
        kind: "message",
        direction: "sent",
        priority: "priority",
        handling: "pending",
        flagged: false,
        from_text: "",
        to_text: "EOC",
        subject: "Shelter update",
        body: "",
        channel: "",
        location: "",
        tags: ["STATUS"],
        ham: null,
        emergency: null,
      },
      allowedKinds("emergency"),
    );
    expect(t.event_at_utc).toBe(before);
    expect(t.subject).toBe("Shelter update");
    expect(t.handling).toBe("pending");
  });
});

describe("shortcuts", () => {
  const ev = (key: string, mods: Partial<KeyboardEvent> = {}) =>
    ({ key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...mods }) as KeyboardEvent;
  it("maps Ctrl/Cmd chords to actions", () => {
    const mac = /mac/i.test(navigator.platform);
    const m = mac ? { metaKey: true } : { ctrlKey: true };
    expect(keyToAction(ev("n", m))).toBe("new_entry");
    expect(keyToAction(ev("Enter", m))).toBe("save_entry");
    expect(keyToAction(ev("f", m))).toBe("find");
    expect(keyToAction(ev("e", m))).toBe("export");
    expect(keyToAction(ev("n"))).toBeNull();
  });
});

describe("regressions from hands-on testing", () => {
  const ctx = {
    type: "ham" as const,
    preset,
    profile: {
      id: "p",
      name: "Op",
      callsign: "KJ4MRC",
      license_class: "",
      organization: "",
      position: "",
      grid: "EM73",
      active: true,
      created_at_utc: "",
      updated_at_utc: "",
    },
    incident: { incident_name: "", incident_number: "", operational_period: "", agency: "", location: "" },
  };
  it("traffic entries in a HAM logbook never store QSO fields from profile prefills", () => {
    const f = newForm(ctx);
    f.kind = "traffic";
    f.subject = "Net check-in";
    expect(f.ham.operator_call).toBe("KJ4MRC");
    expect(toEntryInput(f, "ham", "utc").ham).toBeNull();
  });
  it("takes the station callsign from the station, never from the operator's own callsign (2.1)", () => {
    expect(newForm(ctx).ham.station_call).toBe("");
    const station = {
      label: "",
      tactical_id: "",
      station_callsign: "W4CLB",
      agency: "",
      default_location: null,
      pota_ref: "US-1234",
      sota_ref: "",
      updated_at_utc: "",
    };
    const f = newForm({ ...ctx, station });
    expect([f.ham.station_call, f.ham.operator_call, f.ham.my_pota_ref]).toEqual([
      "W4CLB",
      "KJ4MRC",
      "US-1234",
    ]);
  });
  it("flags a frequency outside the chosen band before saving", async () => {
    const { validateForm } = await import("../lib/entryForm");
    const f = newForm(ctx);
    f.subject = "QSO";
    f.ham.their_call = "W1AW";
    f.freqText = "14.074";
    f.ham.band = "40m";
    expect(validateForm(f, "ham")["ham.band"]).toMatch(/outside the 40m band/);
    f.ham.band = "20m";
    expect(validateForm(f, "ham")["ham.band"]).toBeUndefined();
  });
});

describe("operational periods (V2)", () => {
  const op = (id: string, start: string, end: string, status: "active" | "void" = "active") => ({
    id,
    name: id,
    start_utc: start,
    end_utc: end,
    notes: "",
    status,
    created_at_utc: start,
    updated_at_utc: start,
  });
  const ps = [
    op("OP1", "2026-11-02T12:00:00.000Z", "2026-11-02T20:00:00.000Z"),
    op("Handover", "2026-11-02T19:30:00.000Z", "2026-11-02T20:30:00.000Z"),
    op("Voided", "2026-11-02T21:00:00.000Z", "2026-11-02T23:00:00.000Z", "void"),
  ];
  const at = (iso: string) => Date.parse(iso);

  it("current period is the running one, latest-starting during a handover, none between periods", () => {
    expect(currentPeriod(ps, at("2026-11-02T15:00:00Z"))?.id).toBe("OP1");
    expect(currentPeriod(ps, at("2026-11-02T19:45:00Z"))?.id).toBe("Handover");
    expect(currentPeriod(ps, at("2026-11-02T20:15:00Z"))?.id).toBe("Handover");
    expect(currentPeriod(ps, at("2026-11-02T22:00:00Z"))).toBeNull();
    expect(currentPeriod(ps, at("2026-11-02T20:00:00Z"))?.id).toBe("Handover");
  });

  it("new entries carry the current period; None is sent as null", () => {
    const ctx = {
      type: "emergency" as const,
      preset,
      profile: null,
      incident: {
        incident_name: "Storm",
        incident_number: "",
        operational_period: "",
        agency: "",
        location: "",
      },
      currentOpId: "OP1",
    };
    const f = newForm(ctx);
    f.subject = "x";
    expect(toEntryInput(f, "emergency", "utc").op_id).toBe("OP1");
    f.op_id = "";
    expect(toEntryInput(f, "emergency", "utc").op_id).toBeNull();
    expect(newForm({ ...ctx, currentOpId: null }).op_id).toBe("");
  });
});

describe("activity start defaults to the event time", () => {
  it("does not block saving and sends the event time as the start", () => {
    const f = newForm({
      type: "emergency",
      preset,
      profile: null,
      incident: {
        incident_name: "Storm",
        incident_number: "",
        operational_period: "",
        agency: "",
        location: "",
      },
    });
    f.kind = "activity";
    f.subject = "Net opened";
    expect(validateForm(f, "emergency")).toEqual({});
    expect(toEntryInput(f, "emergency", "utc").emergency?.activity_start_utc).toBe(f.event_at_utc);
  });
});

describe("channel plan", () => {
  it("writes a planned channel as name plus receive frequency", () => {
    const r = {
      ...blankChannel(),
      channel_name: "TAC 2",
      rx_freq: "155.4750",
      function: "Tactical",
      mode: "A",
    };
    expect(channelValue(r)).toBe("TAC 2 (155.4750 MHz)");
    expect(channelHint(r)).toBe("Tactical · analog");
    expect(channelValue({ ...blankChannel(), rx_freq: "146.520" })).toBe("146.520 MHz");
    expect(channelValue({ ...blankChannel(), zone_group: "Z1", channel_number: "3" })).toBe("Z1 ch 3");
    expect(channelValue(blankChannel())).toBe("");
  });
});

describe("Save & Add Another (2.1)", () => {
  it("keeps session settings and clears the contact", async () => {
    const { carrySession } = await import("../lib/entryForm");
    const ctx = {
      type: "ham" as const,
      preset,
      profile: null,
      incident: { incident_name: "", incident_number: "", operational_period: "", agency: "", location: "" },
    };
    const prev = newForm(ctx);
    prev.kind = "qso";
    Object.assign(prev.ham, {
      their_call: "K1ABC",
      rst_sent: "59",
      band: "20m",
      mode: "SSB",
      station_call: "W4CLB",
      my_pota_ref: "US-0005",
      pota_ref: "US-1234",
    });
    prev.freqText = "14.250";
    prev.subject = "QSO with K1ABC";
    prev.operator_location = {
      latitude_deg: 35.5,
      longitude_deg: -83.5,
      label: "",
      maidenhead: "",
      source: "external_usb_nmea",
      provider_id: "",
      captured_at_utc: null,
      accuracy_m: null,
      altitude_m: null,
      satellites: null,
      notes: "",
    };
    prev.reported_location = { ...prev.operator_location, source: "reported", label: "Their QTH" };
    const next = carrySession(prev, newForm(ctx));
    expect([
      next.ham.band,
      next.ham.mode,
      next.freqText,
      next.ham.station_call,
      next.ham.my_pota_ref,
    ]).toEqual(["20m", "SSB", "14.250", "W4CLB", "US-0005"]);
    expect([next.ham.their_call, next.ham.rst_sent, next.ham.pota_ref, next.subject]).toEqual([
      "",
      "",
      "",
      "",
    ]);
    expect(next.operator_location?.latitude_deg).toBe(35.5);
    expect(next.reported_location).toBeNull();
  });
});

import { maidenhead } from "../lib/location";
describe("maidenhead (2.1)", () => {
  it("matches the core's locator for known points", () => {
    expect(maidenhead(33.8121, -84.4011)).toBe("EM73tt");
    expect(maidenhead(41.714775, -72.72726)).toBe("FN31pr");
    expect(maidenhead(-33.86, 151.21)).toBe("QF56od");
  });
});

import { carrySession, followIdentity } from "../lib/entryForm";
const INC = { incident_name: "", incident_number: "", operational_period: "", agency: "", location: "" };
describe("operator handover in an open QSO (2.1)", () => {
  it("follows the new operator unless the callsign was typed", () => {
    const f = newForm({
      type: "ham",
      preset,
      profile: null,
      incident: INC,
      station: null,
      currentOpId: null,
    });
    f.ham.operator_call = "KJ4AAA";
    f.ham.station_call = "W4CLB";
    followIdentity(f, { operator: "KJ4AAA", station: "W4CLB" }, { operator: "KJ4BBB", station: "W4CLB" });
    expect([f.ham.operator_call, f.ham.station_call]).toEqual(["KJ4BBB", "W4CLB"]);
    f.ham.operator_call = "N0TYPED";
    followIdentity(f, { operator: "KJ4BBB", station: "W4CLB" }, { operator: "KJ4CCC", station: "K4XX" });
    expect([f.ham.operator_call, f.ham.station_call]).toEqual(["N0TYPED", "K4XX"]);
  });
  it("Save & Add Another does not carry the previous operator's callsign", () => {
    const ctx = {
      type: "ham" as const,
      preset,
      profile: null,
      incident: INC,
      station: null,
      currentOpId: null,
    };
    const prev = newForm(ctx);
    prev.ham.operator_call = "KJ4AAA";
    const next = newForm(ctx);
    next.ham.operator_call = "KJ4BBB";
    expect(carrySession(prev, next).ham.operator_call).toBe("KJ4BBB");
  });
});

import { blankFollowUp, formCopyOf } from "../lib/entryForm";
import { matchDirectory } from "../lib/directory";
describe("2.2 flags and directory", () => {
  const INC2 = { incident_name: "", incident_number: "", operational_period: "", agency: "", location: "" };
  const ctx = {
    type: "emergency" as const,
    preset,
    profile: null,
    incident: INC2,
    station: null,
    currentOpId: null,
  };
  it("new entries, copies and Save & Add Another never carry Important or a follow-up", () => {
    const f = newForm(ctx);
    expect([f.important, f.follow_up.state]).toEqual([false, "none"]);
    f.important = true;
    f.follow_up = { ...blankFollowUp(), state: "open", note: "call back" };
    expect(carrySession(f, newForm(ctx)).follow_up.state).toBe("none");
    expect(carrySession(f, newForm(ctx)).important).toBe(false);
    const saved = { ...toEntryInput(f, "emergency", "utc"), id: "x" } as unknown as Parameters<
      typeof formCopyOf
    >[0];
    const copy = formCopyOf(
      { ...(saved as object), attachments: [], ham: null, emergency: null } as never,
      ctx,
    );
    expect([copy.important, copy.follow_up.state]).toEqual([false, "none"]);
  });
  it("a cleared follow-up is sent without its due time or note", () => {
    const f = newForm(ctx);
    f.follow_up = { state: "none", due_utc: "2026-11-02T22:00:00Z", note: "stale", resolved_utc: null };
    expect(toEntryInput(f, "emergency", "utc").follow_up).toEqual(blankFollowUp());
    f.follow_up = { state: "open", due_utc: "2026-11-02T22:00:00Z", note: "check", resolved_utc: null };
    expect(toEntryInput(f, "emergency", "utc").follow_up?.note).toBe("check");
  });
  it("matches directory items by identifier, name or alias, ignoring case and spacing", () => {
    const base = {
      kind: "tactical_identifier" as const,
      agency: "",
      channel: "TAC 2",
      frequency_hz: null,
      mode: "",
      tags: [],
      notes: "",
      last_used_utc: null,
      last_entry_id: null,
      created_at_utc: "",
      updated_at_utc: "",
    };
    const items = [
      { ...base, id: "1", label: "Shelter 2", identifier: "SHELTER 2", aliases: ["Lake HS"] },
      { ...base, id: "2", label: "Net control", identifier: "W4CLB", aliases: [] },
    ];
    expect(matchDirectory(items, "  shelter   2 ")?.id).toBe("1");
    expect(matchDirectory(items, "lake hs")?.id).toBe("1");
    expect(matchDirectory(items, "w4clb")?.id).toBe("2");
    expect(matchDirectory(items, "Shelter")).toBeNull();
  });
});
