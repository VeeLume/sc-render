<script lang="ts">
	/**
	 * The hangar render: a `Hangar` on a canvas that fills this component's
	 * box (give it a height). Drag orbits, right-drag pans, the wheel zooms
	 * (three's OrbitControls); a click without a drag goes to `onpick` with
	 * the canvas pixel. The hangar is bound out for the page to draw with.
	 */
	import { onMount } from 'svelte';
	import { tauriSource } from '@veelume/sc-render/tauri';
	import { Hangar } from '$lib/hangar';

	let {
		hangar = $bindable(null),
		busy = false,
		onpick
	}: {
		hangar?: Hangar | null;
		busy?: boolean;
		onpick?: (x: number, y: number) => void;
	} = $props();

	let canvas: HTMLCanvasElement;
	let box: HTMLDivElement;
	let down: { x: number; y: number; id: number } | null = null;

	onMount(() => {
		const h = new Hangar(canvas, tauriSource());
		hangar = h;
		const resize = () => h.resize(box.clientWidth, box.clientHeight);
		resize();
		const observer = new ResizeObserver(resize);
		observer.observe(box);
		return () => {
			observer.disconnect();
			h.dispose();
			hangar = null;
		};
	});
</script>

<div bind:this={box} class="relative h-full min-h-0 w-full overflow-hidden rounded-lg border bg-[#1c2632]">
	<canvas
		bind:this={canvas}
		class="absolute inset-0 block size-full touch-none"
		title="Drag to orbit, right-drag to pan, wheel to zoom, click to act"
		onpointerdown={(e) => {
			if (e.button === 0) down = { x: e.clientX, y: e.clientY, id: e.pointerId };
		}}
		onpointerup={(e) => {
			if (down && e.pointerId === down.id && Math.hypot(e.clientX - down.x, e.clientY - down.y) < 4) onpick?.(e.offsetX, e.offsetY);
			down = null;
		}}
		oncontextmenu={(e) => e.preventDefault()}
	></canvas>
	{#if busy}<div class="absolute top-2 right-3 text-xs text-muted-foreground">rendering…</div>{/if}
</div>
