<script lang="ts">
	/**
	 * The interactive render: a `Cell` on a canvas that fills this component's
	 * box (give it a height), with turn / pan / zoom (see viewControls). The
	 * cell is bound out for the page to draw with; it exists from mount on.
	 * `class` styles the frame (border, background); the component itself
	 * brings no stylesheet.
	 */
	import { onMount } from 'svelte';
	import { Cell } from '../cell';
	import type { RenderSource } from '../library';
	import { VIEW_HINT, bindWheelZoom, viewControls, type AxisLock } from '../viewControls';

	let {
		source,
		cell = $bindable(null),
		axisLock = 'free',
		busy = false,
		class: className = ''
	}: {
		source: RenderSource;
		cell?: Cell | null;
		axisLock?: AxisLock;
		/** Shows a quiet "rendering…" mark over the view. */
		busy?: boolean;
		class?: string;
	} = $props();

	let canvas: HTMLCanvasElement;
	let box: HTMLDivElement;

	const controls = viewControls(
		() => canvas,
		() => cell,
		() => axisLock
	);

	onMount(() => {
		const c = new Cell(canvas, source);
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

<div
	bind:this={box}
	class={className}
	style="position: relative; width: 100%; height: 100%; min-height: 0; overflow: hidden"
>
	<canvas
		bind:this={canvas}
		style="position: absolute; inset: 0; display: block; width: 100%; height: 100%; touch-action: none; cursor: grab"
		title={VIEW_HINT}
		{...controls}
	></canvas>
	{#if busy}
		<div style="position: absolute; top: 0.5rem; right: 0.75rem; font-size: 0.75rem; opacity: 0.6">
			rendering…
		</div>
	{/if}
</div>
