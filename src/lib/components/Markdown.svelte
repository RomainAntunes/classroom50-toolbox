<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { MarkdownBlock, MarkdownInline } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";

  /**
   * An assignment's description, parsed by Rust (`markdown.rs`): plain markup only, never
   * `{@html}`. Links open in the browser instead of navigating; math shows its LaTeX source.
   */
  let { blocks }: { blocks: MarkdownBlock[] } = $props();

  // Rust's heading levels 1 to 6 sit under the class's h2 and the row.
  const heading = (level: number) => `h${Math.min(level + 2, 6)}`;

  function open(event: MouseEvent, href: string) {
    event.preventDefault();
    void openUrl(href);
  }

  // A middle click fires `auxclick`, not `click`: without this the webview would follow
  // the link or open a window of its own.
  function openAux(event: MouseEvent, href: string) {
    if (event.button === 1) open(event, href);
  }
</script>

{#snippet inlines(content: MarkdownInline[])}
  {#each content as inline, i (i)}
    {#if inline.kind === "text"}{inline.text}{:else if inline.kind === "code"}<code dir="ltr"
        >{inline.text}</code
      >{:else if inline.kind === "math"}<code class="math" dir="ltr">{inline.source}</code
      >{:else if inline.kind === "emphasis"}<em>{@render inlines(inline.content)}</em
      >{:else if inline.kind === "strong"}<strong>{@render inlines(inline.content)}</strong
      >{:else if inline.kind === "strike"}<s>{@render inlines(inline.content)}</s
      >{:else if inline.kind === "link"}<a
        href={inline.href}
        title={inline.href}
        onclick={(event) => open(event, inline.href)}
        onauxclick={(event) => openAux(event, inline.href)}>{@render inlines(inline.content)}</a
      >{:else if inline.kind === "break"}<br />{:else if inline.kind === "footnoteRef"}<sup
        >{inline.number}</sup
      >{/if}
  {/each}
{/snippet}

{#snippet list(content: MarkdownBlock[])}
  {#each content as block, i (i)}
    {#if block.kind === "paragraph"}
      <p>{@render inlines(block.content)}</p>
    {:else if block.kind === "heading"}
      <svelte:element this={heading(block.level)}>{@render inlines(block.content)}</svelte:element>
    {:else if block.kind === "list"}
      <svelte:element this={block.ordered ? "ol" : "ul"} start={block.ordered ? block.start : null}>
        {#each block.items as item, j (j)}
          <li class:task={item.task !== null}>
            {#if item.task !== null}<input type="checkbox" checked={item.task} disabled />{/if}
            {@render list(item.blocks)}
          </li>
        {/each}
      </svelte:element>
    {:else if block.kind === "code"}
      <pre dir="ltr"><code>{block.text}</code></pre>
    {:else if block.kind === "math"}
      <pre class="math" dir="ltr"><code>{block.source}</code></pre>
    {:else if block.kind === "table"}
      <div class="scroll">
        <table>
          <thead>
            <tr>
              {#each block.head as cell, c (c)}
                <th style:text-align={block.align[c]}>{@render inlines(cell)}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each block.rows as row, r (r)}
              <tr>
                {#each row as cell, c (c)}
                  <td style:text-align={block.align[c]}>{@render inlines(cell)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if block.kind === "quote"}
      <blockquote>{@render list(block.blocks)}</blockquote>
    {:else if block.kind === "rule"}
      <hr />
    {:else if block.kind === "footnotes"}
      <section class="footnotes" aria-label={t("markdown.footnotes")}>
        <ol>
          {#each block.notes as note (note.number)}
            <li value={note.number}>{@render list(note.blocks)}</li>
          {/each}
        </ol>
      </section>
    {/if}
  {/each}
{/snippet}

<div class="markdown">{@render list(blocks)}</div>

<style>
  /* Scoped by the root, so the elements inside need no class of their own. */
  .markdown :global {
    /* Each block follows the direction of its own text, whatever the UI's. */
    :is(p, li, h3, h4, h5, h6, th, td) {
      unicode-bidi: plaintext;
    }

    :is(p, ul, ol, pre, blockquote, .scroll, h3, h4, h5, h6) {
      margin: 0 0 6px;
    }

    :is(h3, h4, h5, h6) {
      font-size: 1em;
      color: var(--fg);
    }

    :is(ul, ol) {
      padding-inline-start: 18px;
    }

    li > :last-child {
      margin-block-end: 2px;
    }

    .task {
      list-style: none;
    }

    .task > input {
      margin: 0;
      margin-inline: -16px 4px;
      float: inline-start;
      accent-color: var(--accent);
    }

    code {
      padding: 0 3px;
      border-radius: var(--radius-control);
      background: var(--bg-control);
      color: var(--fg);
      font: 0.92em var(--font-mono);
      overflow-wrap: anywhere;
    }

    pre,
    .scroll {
      overflow: auto;
    }

    pre {
      padding: 6px 8px;
      max-height: 16em;
      border: 1px solid var(--border);
      border-radius: var(--radius-control);
      background: var(--bg-control);
    }

    pre code {
      padding: 0;
      background: none;
      overflow-wrap: normal;
    }

    table {
      border-collapse: collapse;
    }

    th,
    td {
      padding: 2px 6px;
      border: 1px solid var(--border);
      text-align: start;
    }

    blockquote {
      margin-inline: 0;
      padding-inline-start: 8px;
      border-inline-start: 3px solid var(--border);
    }

    hr {
      border: none;
      border-block-start: 1px solid var(--border);
    }

    a {
      color: var(--accent-text);
    }
  }
</style>
