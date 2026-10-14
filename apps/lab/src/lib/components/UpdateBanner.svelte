<script lang="ts">
	import { updater } from '$lib/stores/updater.svelte';
	import { X } from 'lucide-svelte';
</script>

{#if updater.available}
	<div
		class="flex shrink-0 items-center gap-3 border-b bg-accent px-4 py-2 text-sm text-accent-foreground"
	>
		<span class="min-w-0 flex-1 truncate">
			Version {updater.available.version} is available.
			{#if updater.installing}
				Downloading… {Math.round(updater.progress * 100)}%
			{/if}
		</span>

		{#if updater.error}
			<span class="text-destructive">{updater.error}</span>
		{/if}

		<button
			class="rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground disabled:opacity-50"
			disabled={updater.installing}
			onclick={() => updater.installAndRelaunch()}
		>
			Install &amp; restart
		</button>

		<button
			class="rounded-md p-1 hover:bg-background/40"
			aria-label="Dismiss"
			onclick={() => updater.dismiss()}
		>
			<X size={16} />
		</button>
	</div>
{/if}
