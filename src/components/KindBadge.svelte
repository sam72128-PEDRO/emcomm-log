<script lang="ts">
  import type { EntryKind } from "../lib/types";
  import { KIND_BADGE } from "../lib/entryForm";
  let { kind }: { kind: EntryKind } = $props();
</script>

<span class="badge kind-{kind}">{KIND_BADGE[kind]}</span>
