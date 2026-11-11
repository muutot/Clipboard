<script lang="ts">
  import { createOperationMonitor } from "#lib/services/background-operations.svelte.ts";
  import BackgroundOperationStatus from "./BackgroundOperationStatus.svelte";
  const operation = createOperationMonitor("sync");
</script>

<BackgroundOperationStatus {operation} />
