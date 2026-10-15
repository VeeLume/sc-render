<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	// `/` exists only to land somewhere: the item browser.
	onMount(() => {
		goto('/items', { replaceState: true });
	});
</script>
