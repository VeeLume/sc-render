<script lang="ts">
	import { Settings } from '@veelume/ui';
	import { settingsCategories } from '$lib/settingsNav';

	let { children } = $props();
</script>

<Settings.Root categories={settingsCategories}>
	{@render children()}
</Settings.Root>
