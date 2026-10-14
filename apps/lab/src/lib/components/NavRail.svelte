<script lang="ts">
	import { page } from '$app/state';
	import { Settings } from 'lucide-svelte';
	import { nav, activeNavPath } from '$lib/nav.svelte';

	let { showLabels }: { showLabels: boolean } = $props();

	let groups = $derived(nav.groups);
	let active = $derived(
		activeNavPath(
			page.url.pathname,
			groups.flatMap((g) => g.items.map((i) => i.path))
		)
	);
	let settingsActive = $derived(page.url.pathname.startsWith('/settings'));

	const rowBase = 'flex items-center gap-3 rounded-full px-3 text-sm font-medium transition-colors';
</script>

<nav
	class="flex flex-col border-r bg-background py-3"
	class:w-56={showLabels}
	class:px-3={showLabels}
	class:w-20={!showLabels}
	class:items-center={!showLabels}
	class:px-2={!showLabels}
>
	<div class="flex min-h-0 flex-1 flex-col gap-1 overflow-auto" class:w-full={showLabels}>
		{#each groups as group, i (group.label ?? i)}
			{#if group.label && showLabels}
				<div
					class="px-3 pt-3 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase"
				>
					{group.label}
				</div>
			{:else if i > 0}
				<div
					class="my-1 h-px self-center bg-border"
					class:w-full={showLabels}
					class:w-6={!showLabels}
				></div>
			{/if}

			{#each group.items as item (item.path)}
				{@const isActive = active === item.path}
				{@const Icon = item.icon}
				<a
					href={item.path}
					class={rowBase}
					class:w-full={showLabels}
					class:justify-center={!showLabels}
					class:bg-accent={isActive}
					class:text-accent-foreground={isActive}
					class:text-muted-foreground={!isActive}
					class:hover:bg-accent={!isActive}
					style="height: var(--density-target)"
					title={!showLabels ? item.label : undefined}
				>
					<Icon size={20} class="shrink-0" />
					{#if showLabels}<span class="truncate">{item.label}</span>{/if}
				</a>
			{/each}
		{/each}
	</div>

	<div
		class="mt-2 flex flex-col gap-1 border-t pt-2"
		class:w-full={showLabels}
		class:items-center={!showLabels}
	>
		<a
			href="/settings"
			class={rowBase}
			class:w-full={showLabels}
			class:justify-center={!showLabels}
			class:bg-accent={settingsActive}
			class:text-accent-foreground={settingsActive}
			class:text-muted-foreground={!settingsActive}
			class:hover:bg-accent={!settingsActive}
			style="height: var(--density-target)"
			title={!showLabels ? 'Settings' : undefined}
		>
			<Settings size={20} class="shrink-0" />
			{#if showLabels}<span class="truncate">Settings</span>{/if}
		</a>
	</div>
</nav>
