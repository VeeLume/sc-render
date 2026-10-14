<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	// `/` exists only to land somewhere. Point it at whatever the app's real
	// entry destination is.
	onMount(() => {
		goto('/home', { replaceState: true });
	});
</script>
