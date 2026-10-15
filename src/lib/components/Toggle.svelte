<script lang="ts">
  let {
    label,
    hint,
    checked,
    onchange,
  }: {
    label: string;
    hint?: string;
    checked: boolean;
    onchange: (checked: boolean) => void;
  } = $props();
</script>

<label class="row">
  <span class="text">
    <span>{label}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </span>
  <input
    type="checkbox"
    role="switch"
    {checked}
    onchange={(event) => onchange(event.currentTarget.checked)}
  />
</label>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .hint {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  /* Switch geometry per platform: macOS 26x15, Windows 40x20 with outline when off,
     GNOME 46x26. Colours come from the theme tokens. */
  input {
    --w: 26px;
    --h: 15px;
    appearance: none;
    margin: 0;
    flex: none;
    position: relative;
    width: var(--w);
    height: var(--h);
    border-radius: calc(var(--h) / 2);
    background: var(--bg-control-hover);
    transition: background 0.15s;
  }

  input::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: calc(var(--h) - 4px);
    height: calc(var(--h) - 4px);
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.3);
    transition: transform 0.15s;
  }

  input:checked {
    background: var(--accent);
  }

  input:checked::after {
    transform: translateX(calc(var(--w) - var(--h)));
  }

  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :global(:root[data-os="windows"]) input {
    --w: 40px;
    --h: 20px;
    background: transparent;
    border: 1px solid var(--fg-muted);
  }

  :global(:root[data-os="windows"]) input::after {
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    background: var(--fg-muted);
    box-shadow: none;
  }

  :global(:root[data-os="windows"]) input:checked {
    background: var(--accent);
    border-color: var(--accent);
  }

  :global(:root[data-os="windows"]) input:checked::after {
    background: var(--accent-fg);
  }

  :global(:root[data-os="linux"]) input {
    --w: 46px;
    --h: 26px;
  }
</style>
