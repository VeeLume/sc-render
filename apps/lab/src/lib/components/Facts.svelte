<script lang="ts">
	/** Label / value pairs, one per line; long values wrap anywhere. */
	let { facts }: { facts: { label: string; value: string; mono?: boolean }[] } = $props();
</script>

<dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
	{#each facts as f (f.label)}
		<dt class="text-muted-foreground">{f.label}</dt>
		<dd class="break-all {f.mono ? 'font-mono text-xs leading-5' : ''}">{f.value}</dd>
	{/each}
</dl>
