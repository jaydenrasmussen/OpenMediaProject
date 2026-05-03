<script>
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  onMount(() => goto("/discover", { replaceState: true }));
</script>
