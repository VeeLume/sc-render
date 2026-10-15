<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { Loading, Shell, setKitContext } from '@veelume/ui';
	import { Settings } from 'lucide-svelte';
	import { nav } from '$lib/nav.svelte';
	import { appearance } from '$lib/stores/appearance.svelte';
	import { settings } from '$lib/stores/settings.svelte';
	import { updater } from '$lib/stores/updater.svelte';
	import UpdateBanner from '$lib/components/UpdateBanner.svelte';

	let { children } = $props();

	// The kit's only channel into app state: English text, numbers and dates
	// as the system formats them.
	setKitContext({
		messageLocale: () => 'en',
		formattingLocale: () => (typeof navigator !== 'undefined' ? navigator.language : 'en')
	});

	onMount(async () => {
		// Apply the saved density / colour scheme before anything else renders.
		appearance.init();
		await settings.init();
		// Fire-and-forget: populates the updater store, which drives the banner.
		updater.check();
	});
</script>

{#if settings.loading}
	<div class="flex h-svh items-center justify-center">
		<Loading />
	</div>
{:else}
	<!-- A desktop lab tool: rail only, no bottom bar. -->
	<Shell.Root groups={nav.groups}>
		<Shell.Rail>
			{#snippet footer()}
				<Shell.SettingsFooter icon={Settings} />
			{/snippet}
		</Shell.Rail>
		<Shell.Content>
			{#snippet banner()}
				<UpdateBanner />
			{/snippet}
			{@render children()}
		</Shell.Content>
	</Shell.Root>
{/if}
