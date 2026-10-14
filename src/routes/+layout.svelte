<script lang="ts">
  import "$lib/theme/theme.css";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { setLocale } from "$lib/i18n";

  let { children } = $props();

  onMount(async () => {
    setLocale(navigator.language);
    const info = await api.appInfo();
    document.documentElement.dataset.os = info.os;
  });
</script>

<div class="popover">
  {@render children()}
</div>

<style>
  .popover {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-window);
    border-radius: var(--radius-window);
    overflow: hidden;
  }

  :global(:root[data-os="linux"]) .popover {
    border: 1px solid var(--border);
  }
</style>
