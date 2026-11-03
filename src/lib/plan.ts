// Channel plan helpers (V2): how a planned channel is written into an entry and described.
import type * as T from "./types";

/** Text stored in an entry's Channel field when a planned channel is chosen, e.g. "TAC 2 (155.4750 MHz)". */
export function channelValue(r: T.ChannelRow): string {
  const name = r.channel_name.trim() || [r.zone_group, r.channel_number].filter(Boolean).join(" ch ");
  const f = r.rx_freq.trim();
  if (!name) return f ? `${f} MHz` : "";
  return f ? `${name} (${f} MHz)` : name;
}

/** Short description shown next to a suggestion: function, assignment, mode. */
export function channelHint(r: T.ChannelRow): string {
  const mode = { A: "analog", D: "digital", M: "mixed" }[r.mode as "A" | "D" | "M"] ?? "";
  return [r.function, r.assignment, mode].filter(Boolean).join(" · ");
}

export function blankChannel(): T.ChannelRow {
  return {
    zone_group: "",
    channel_number: "",
    function: "",
    channel_name: "",
    assignment: "",
    rx_freq: "",
    rx_nw: "",
    rx_tone: "",
    tx_freq: "",
    tx_nw: "",
    tx_tone: "",
    mode: "",
    remarks: "",
  };
}
