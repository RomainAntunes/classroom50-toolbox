<!-- A small ring before a busy button's label, in the label's color; `alone` when it
     stands in for an icon. -->
<script lang="ts">
  let { alone = false }: { alone?: boolean } = $props();
</script>

<span class="spinner" class:alone aria-hidden="true"></span>

<style>
  .spinner {
    display: inline-block;
    width: 0.85em;
    height: 0.85em;
    margin-inline-end: 6px;
    vertical-align: -0.1em;
    box-sizing: border-box;
    border: 1.5px solid currentColor;
    border-block-start-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .alone {
    margin-inline-end: 0;
  }

  /* theme.css stops the spin: a whole faded ring, like the bars that stand still. */
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      border-block-start-color: currentColor;
      opacity: 0.5;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
