// How many in-popover dialogs (`Modal.svelte`) are open: while one is, Escape is theirs and
// never hides the popover.

export const modals = $state({ open: 0 });
