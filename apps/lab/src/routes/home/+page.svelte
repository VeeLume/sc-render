<script lang="ts">
	import { onMount } from 'svelte';
	import { FIXTURES, type Fixture } from '$lib/render/fixtures';
	import { Cell } from '$lib/render/scene';
	import { commands } from '$lib/bindings';

	let canvas: HTMLCanvasElement;
	let cell: Cell | null = null;
	let current = $state<Fixture>(FIXTURES[0]);
	let log = $state<string[]>([]);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let lightScale = $state(1);
	let ambient = $state(0.05);

	async function show(fx: Fixture) {
		current = fx;
		if (!cell) return;
		busy = true;
		error = null;
		try {
			log = await cell.show(fx, { lightScale, ambient });
			await snapshot(fx.id);
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	/** Dev aid: the rendered cell as a PNG under the app data folder. */
	async function snapshot(name: string) {
		const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/png'));
		if (!blob) return;
		const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
		const res = await commands.saveSnapshot(name, bytes);
		if (res.status === 'ok') log = [...log, `snapshot ${res.data}`];
	}

	function relight() {
		if (!cell) return;
		cell.setLights(current, { lightScale, ambient });
		cell.render();
	}

	onMount(() => {
		cell = new Cell(canvas, 1);
		const fit = () => {
			const size = Math.min(canvas.parentElement!.clientWidth, 640);
			cell!.resize(size, size);
			cell!.render();
		};
		fit();
		window.addEventListener('resize', fit);
		(async () => {
			// Dev: render + snapshot every fixture once, then settle on the first.
			if (import.meta.env.DEV) for (const fx of FIXTURES.slice(1)) await show(fx);
			await show(FIXTURES[0]);
		})();
		return () => window.removeEventListener('resize', fit);
	});
</script>

<div class="flex h-full gap-4 p-[var(--density-padding)]">
	<aside class="w-64 shrink-0 space-y-1">
		<h1 class="mb-3 text-lg font-semibold">Items</h1>
		{#each FIXTURES as fx (fx.id)}
			<button
				class="block w-full rounded px-3 py-2 text-left text-sm hover:bg-muted {fx.id === current.id
					? 'bg-muted font-medium'
					: ''}"
				onclick={() => show(fx)}>{fx.label}</button
			>
		{/each}
		<div class="mt-6 space-y-3 text-xs">
			<label class="block"
				>Light scale {lightScale.toFixed(2)}
				<input type="range" min="0.1" max="10" step="0.05" bind:value={lightScale} oninput={relight} class="w-full" />
			</label>
			<label class="block"
				>Ambient {ambient.toFixed(3)}
				<input type="range" min="0" max="1" step="0.005" bind:value={ambient} oninput={relight} class="w-full" />
			</label>
		</div>
	</aside>
	<main class="min-w-0 flex-1">
		<div class="relative max-w-[640px] rounded-xl border bg-[#1c2632]">
			<canvas bind:this={canvas} class="block aspect-square w-full"></canvas>
			{#if busy}<div class="absolute inset-0 grid place-items-center text-sm text-muted-foreground">loading…</div>{/if}
		</div>
		{#if error}<pre class="mt-3 text-xs whitespace-pre-wrap text-red-400">{error}</pre>{/if}
		<pre class="mt-3 text-xs whitespace-pre-wrap text-muted-foreground">{log.join('\n')}</pre>
	</main>
</div>
