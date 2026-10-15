<script lang="ts">
	/**
	 * The interactive render: a `Cell` on a canvas that fills this component's
	 * box (give it a height), with turn / pan / zoom (see viewControls). The
	 * cell is bound out for the page to draw with; it exists from mount on.
	 */
	import { onMount } from 'svelte';
	import { Cell } from './scene';
	import { VIEW_HINT, bindWheelZoom, viewControls, type AxisLock } from './viewControls';

	let {
		cell = $bindable(null),
		axisLock = 'free',
		busy = false
	}: {
		cell?: Cell | null;
		axisLock?: AxisLock;
		/** Shows a quiet "rendering…" mark over the view. */
		busy?: boolean;
	} = $props();

	let canvas: HTMLCanvasElement;
	let box: HTMLDivElement;

	const controls = viewControls(
		() => canvas,
		() => cell,
		() => axisLock
	);

	onMount(() => {
		const c = new Cell(canvas);
		cell = c;
		const resize = () => c.resize(box.clientWidth, box.clientHeight);
		resize();
		const observer = new ResizeObserver(resize);
		observer.observe(box);
		const unbindWheel = bindWheelZoom(canvas, () => cell);
		return () => {
			observer.disconnect();
			unbindWheel();
			c.dispose();
			cell = null;
		};
	});
</script>

<div bind:this={box} class="relative h-full min-h-0 w-full overflow-hidden rounded-lg border bg-[#1c2632]">
	<canvas
		bind:this={canvas}
		class="absolute inset-0 block size-full cursor-grab touch-none active:cursor-grabbing"
		title={VIEW_HINT}
		{...controls}
	></canvas>
	{#if busy}<div class="absolute top-2 right-3 text-xs text-muted-foreground">rendering…</div>{/if}
</div>
