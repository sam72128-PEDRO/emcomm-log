import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/svelte";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { tick } from "svelte";
import EntryEditor from "../components/EntryEditor.svelte";
import { app } from "../lib/state.svelte";
import { runAction } from "../lib/actions";
import { setupActive, savedEntry } from "./fixtures";

type Call = { cmd: string; args: Record<string, unknown> };
let calls: Call[] = [];
let handler: (cmd: string, args: Record<string, unknown>) => unknown;

beforeEach(() => {
  calls = [];
  handler = () => null;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return handler(cmd, (args ?? {}) as Record<string, unknown>);
  });
  setupActive("general");
});
afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

const saves = () => calls.filter((c) => c.cmd === "save_entry");
const flush = async () => {
  for (let i = 0; i < 5; i++) {
    await tick();
    await Promise.resolve();
  }
};

describe("Quick Entry editor", () => {
  it("blocks save without a summary and marks the field invalid", async () => {
    render(EntryEditor, { mode: "new" });
    await fireEvent.click(screen.getByRole("button", { name: /Save Entry/ }));
    await flush();
    expect(saves()).toHaveLength(0);
    expect(screen.getByLabelText(/Summary/)).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Summary is required")).toBeInTheDocument();
  });

  it("shows Entry Saved only after the commit returns, then starts a fresh form", async () => {
    let resolve!: (v: unknown) => void;
    handler = (cmd) => (cmd === "save_entry" ? new Promise((r) => (resolve = r)) : null);
    render(EntryEditor, { mode: "new" });
    const summary = screen.getByLabelText(/Summary/) as HTMLInputElement;
    await fireEvent.input(summary, { target: { value: "Bridge out on CR-12" } });
    await fireEvent.click(screen.getByRole("button", { name: /Save Entry/ }));
    await flush();
    expect(saves()).toHaveLength(1);
    expect(app.saveStatus.kind).toBe("saving");
    expect(summary.value).toBe("Bridge out on CR-12");
    // Second click while in flight must not submit again.
    await fireEvent.click(screen.getByRole("button", { name: /Save Entry/ }));
    await flush();
    expect(saves()).toHaveLength(1);
    resolve({ entry: savedEntry(7, "Bridge out on CR-12"), duplicate: false, unchanged: false });
    await flush();
    expect(app.saveStatus).toMatchObject({ kind: "saved", seq: 7 });
    expect((screen.getByLabelText(/Summary/) as HTMLInputElement).value).toBe("");
    const args = saves()[0].args as { input: { subject: string }; requestId: string };
    expect(args.input.subject).toBe("Bridge out on CR-12");
    expect(args.requestId).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("keeps the content on failure, reports Save Failed, and retries with the same request id", async () => {
    handler = (cmd) => {
      if (cmd === "save_entry") throw { code: "DISK_FULL", message: "The disk holding the logbook is full" };
      if (cmd === "save_draft") return "2026-11-02T19:00:00.000Z";
      return null;
    };
    render(EntryEditor, { mode: "new" });
    await fireEvent.input(screen.getByLabelText(/Summary/), { target: { value: "Generator refuel" } });
    await fireEvent.click(screen.getByRole("button", { name: /Save Entry/ }));
    await flush();
    expect(app.saveStatus.kind).toBe("failed");
    expect(screen.getByText(/Save Failed:/)).toBeInTheDocument();
    expect((screen.getByLabelText(/Summary/) as HTMLInputElement).value).toBe("Generator refuel");
    expect(calls.some((c) => c.cmd === "save_draft")).toBe(true);
    // Storage faults block further final saves until storage is rechecked.
    expect(app.storageBlocked?.code).toBe("DISK_FULL");
    expect(screen.getByRole("button", { name: /Save Entry/ })).toBeDisabled();
    app.storageBlocked = null;
    await flush();
    await fireEvent.click(screen.getByRole("button", { name: /Save Entry/ }));
    await flush();
    const ids = saves().map((c) => (c.args as { requestId: string }).requestId);
    expect(ids).toHaveLength(2);
    expect(ids[0]).toBe(ids[1]);
  });

  it("autosaves a draft about two seconds after typing stops", { timeout: 8000 }, async () => {
    handler = (cmd) => (cmd === "save_draft" ? "2026-11-02T19:00:00.000Z" : null);
    render(EntryEditor, { mode: "new" });
    await flush();
    await fireEvent.input(screen.getByLabelText(/Summary/), { target: { value: "half typed" } });
    await new Promise((r) => setTimeout(r, 1500));
    expect(calls.filter((c) => c.cmd === "save_draft")).toHaveLength(0);
    await new Promise((r) => setTimeout(r, 800));
    await flush();
    const d = calls.filter((c) => c.cmd === "save_draft");
    expect(d).toHaveLength(1);
    expect((d[0].args as { content: { form: { subject: string } } }).content.form.subject).toBe("half typed");
    expect(app.draftStatus.kind).toBe("saved");
    expect(app.saveStatus.kind).toBe("idle");
  });

  it("Ctrl/Cmd+Enter action saves through the visible editor", async () => {
    handler = (cmd) =>
      cmd === "save_entry" ? { entry: savedEntry(1, "x"), duplicate: false, unchanged: false } : null;
    render(EntryEditor, { mode: "new" });
    await fireEvent.input(screen.getByLabelText(/Summary/), { target: { value: "via shortcut" } });
    await runAction("save_entry");
    await flush();
    expect(saves()).toHaveLength(1);
  });

  it("ignores incompatible shortcuts while a dialog is open", async () => {
    app.modalDepth = 1;
    app.screen = "dashboard";
    app.assertiveMessage = "";
    await runAction("nav_traffic_log");
    await flush();
    expect(app.screen).toBe("dashboard");
    expect(app.assertiveMessage).toMatch(/not available while a dialog is open/);
    app.modalDepth = 0;
  });
});
