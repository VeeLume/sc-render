<script lang="ts">
	/**
	 * The render log: takes the supporting pane's remaining height and is the
	 * only part of the pane that scrolls (both ways; paths are long).
	 */
	let { lines, error = null }: { lines: string[]; error?: string | null } = $props();
</script>

<section class="flex min-h-0 flex-1 flex-col gap-2">
	<h3 class="font-medium">Render log</h3>
	{#if error}<p class="text-xs break-all text-destructive">{error}</p>{/if}
	<pre
		class="min-h-24 flex-1 overflow-auto rounded-md border bg-muted/30 p-2 text-xs whitespace-pre text-muted-foreground">{lines.join(
			'\n'
		)}</pre>
</section>
