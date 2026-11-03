<script lang="ts">
  import {
    House,
    Zap,
    List,
    Mail,
    BookOpen,
    ChartColumn,
    Search,
    Settings,
    ChevronDown,
  } from "@lucide/svelte";
  import { app } from "../lib/state.svelte";
  import type { Screen } from "../lib/state.svelte";
  import { runAction, modKey } from "../lib/actions";

  const pendingCount = $derived(
    app.stats
      ? (app.stats.handling.pending ?? 0) +
          (app.stats.handling.awaiting_ack ?? 0) +
          (app.stats.handling.sent ?? 0)
      : 0,
  );
  type Item = {
    screen: Screen;
    label: string;
    icon: typeof House;
    key?: string;
    book: boolean;
    action?: string;
  };
  const items: Item[] = [
    { screen: "dashboard", label: "Dashboard", icon: House, key: "1", book: true },
    { screen: "quick", label: "Quick Entry", icon: Zap, key: "2", book: true },
    { screen: "log", label: "Traffic Log", icon: List, key: "3", book: true },
    { screen: "messages", label: "Messages", icon: Mail, key: "4", book: true },
    { screen: "home", label: "Logbooks", icon: BookOpen, book: false },
    { screen: "reports", label: "Reports", icon: ChartColumn, key: "5", book: true },
    { screen: "log", label: "Search", icon: Search, key: "F", book: true, action: "find" },
    { screen: "settings", label: "Settings", icon: Settings, key: ",", book: false },
  ];
  const visible = $derived(items.filter((i) => !i.book || app.active));

  function go(i: Item) {
    if (i.action) runAction(i.action, "menu");
    else app.navigate(i.screen);
  }
</script>

<nav class="sidebar" aria-label="Main">
  <ul>
    {#each visible as i (i.label)}
      <li>
        <button
          class="nav"
          aria-current={app.screen === i.screen && !i.action ? "page" : undefined}
          onclick={() => go(i)}
          title={i.key ? `${i.label} (${modKey()}${i.key})` : i.label}
        >
          <i.icon size={22} strokeWidth={1.8} aria-hidden="true" />
          <span class="grow">{i.label}</span>
          {#if i.screen === "messages" && pendingCount > 0}
            <span class="count" aria-label="{pendingCount} messages need handling">{pendingCount}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  <div class="spacer"></div>
  <button class="current" onclick={() => app.navigate("home")} title="Switch logbook">
    <span class="dot" class:on={!!app.active} aria-hidden="true"></span>
    <span class="ellipsis grow">
      {#if app.active}Logbook: {app.active.info.name}{:else}No logbook open{/if}
    </span>
    <ChevronDown size={16} aria-hidden="true" />
  </button>
</nav>

<style>
  .sidebar {
    width: 228px;
    flex: none;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 14px 10px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .nav {
    width: 100%;
    border: 1px solid transparent;
    background: transparent;
    font-size: 16px;
    padding: 10px 14px;
    gap: 14px;
    border-radius: 9px;
    color: var(--text);
    min-height: 46px;
  }
  .nav :global(svg) {
    color: var(--text-2);
  }
  .nav[aria-current="page"] {
    background: linear-gradient(180deg, var(--accent), var(--accent-2));
    color: #fff;
    font-weight: 600;
  }
  .nav[aria-current="page"] :global(svg) {
    color: #fff;
  }
  .count {
    background: var(--danger);
    color: #fff;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 700;
    min-width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0 6px;
  }
  .current {
    width: 100%;
    font-size: 13px;
    border-color: var(--border);
    background: var(--panel);
    gap: 8px;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .dot.on {
    background: var(--ok);
  }
</style>
