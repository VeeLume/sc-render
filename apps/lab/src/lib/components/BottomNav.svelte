<script lang="ts">
	import { page } from '$app/state';
	import { nav } from '$lib/nav.svelte';

	let items = $derived(nav.bottomItems);

	function isActive(path: string, pathname: string): boolean {
		return pathname === path || pathname.startsWith(path + '/');
	}
</script>

<!-- The inset padding keeps the bar clear of the gesture handle on Android. -->
<nav class="flex border-t bg-background" style="padding-bottom: env(safe-area-inset-bottom)">
	{#each items as item (item.path)}
		{@const active = isActive(item.path, page.url.pathname)}
		{@const Icon = item.icon}
		<a
			href={item.path}
			class="flex flex-1 flex-col items-center gap-1 py-2 text-xs font-medium transition-colors"
			class:text-foreground={active}
			class:text-muted-foreground={!active}
		>
			<span
				class="flex h-8 w-16 items-center justify-center rounded-full transition-colors"
				class:bg-accent={active}
			>
				<Icon size={20} />
			</span>
			<span>{item.label}</span>
		</a>
	{/each}
</nav>
