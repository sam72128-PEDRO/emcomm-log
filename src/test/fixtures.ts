import type * as T from "../lib/types";
import { app } from "../lib/state.svelte";

export const preset: T.PresetConfig = {
  default_kind: null,
  default_direction: null,
  default_priority: null,
  default_channel: "",
  default_mode: "",
  hidden_fields: [],
  field_labels: {},
  quick_tags: ["INFO"],
  channels: [],
  templates: [],
};

export function setupActive(type: T.LogbookType = "general") {
  app.settings = {
    schema_version: 1,
    wizard_completed: true,
    time_display: "utc",
    clock: "24h",
    theme: "dark",
    default_logbook_parent: "/tmp",
    backup: {
      destination: null,
      interval_minutes: 15,
      on_close: true,
      keep_recent: 8,
      keep_daily: 7,
      keep_weekly: 4,
    },
    active_profile_id: "p1",
    default_preset_ids: {},
  };
  app.profiles = [
    {
      id: "p1",
      name: "Test Op",
      callsign: "KJ4MRC",
      license_class: "",
      organization: "",
      position: "",
      grid: "",
      active: true,
      created_at_utc: "",
      updated_at_utc: "",
    },
  ];
  app.storageBlocked = null;
  app.saveStatus = { kind: "idle" };
  app.draftStatus = { kind: "idle" };
  app.modalDepth = 0;
  app.setActive({
    info: {
      id: "11111111-1111-4111-8111-111111111111",
      logbook_type: type,
      name: "Test Book",
      incident: {
        incident_name: "Storm",
        incident_number: "",
        operational_period: "OP1",
        agency: "",
        location: "",
      },
      created_at_utc: "",
      updated_at_utc: "",
      archived_at_utc: null,
      next_sequence: 1,
      schema_version: 1,
      origin_profile_snapshot: {
        profile_id: "p1",
        name: "Test Op",
        callsign: "",
        license_class: "",
        organization: "",
        position: "",
        grid: "",
      },
      provenance: [],
      path: "/tmp/book",
      read_only: false,
    },
    settings: {
      time_display_override: null,
      preset_id: null,
      preset_name: "",
      preset_version: 1,
      preset,
      operational_notes: "",
    },
    health: {
      ok: true,
      path: "/tmp/book",
      journal_mode: "wal",
      synchronous: 2,
      foreign_keys: true,
      error: null,
    },
    notices: [],
    drafts: [],
    backup: {
      configured: false,
      destination: null,
      interval_minutes: 15,
      running: false,
      last: null,
      last_success: null,
      message: null,
    },
  });
}

export function savedEntry(seq: number, subject: string): T.Entry {
  return {
    id: `e${seq}`,
    sequence_no: seq,
    kind: "traffic",
    event_at_utc: "2026-11-02T19:00:00.000Z",
    event_timezone: "UTC",
    event_utc_offset_min: 0,
    recorded_at_utc: "2026-11-02T19:00:01.000Z",
    updated_at_utc: "2026-11-02T19:00:01.000Z",
    status: "active",
    voided_at_utc: null,
    void_reason: null,
    direction: "received",
    priority: "routine",
    handling: null,
    flagged: false,
    from_text: "",
    to_text: "",
    subject,
    body: "",
    channel: "",
    location: "",
    tags: [],
    operator_snapshot: {
      profile_id: "p1",
      name: "Test Op",
      callsign: "",
      license_class: "",
      organization: "",
      position: "",
      grid: "",
    },
    current_revision: 1,
    favorite: false,
    ham: null,
    emergency: null,
    attachments: [],
  };
}
