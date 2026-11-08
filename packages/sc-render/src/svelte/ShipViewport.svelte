<script lang="ts">
	/**
	 * A `ShipView` on a canvas that fills this component's box (give it a
	 * height). The view is bound out for the page to draw with; it exists from
	 * mount on and its GPU memory goes with the component. Drag orbits, the
	 * wheel zooms, a right drag pans. `class` styles the frame; the component
	 * itself brings no stylesheet.
	 */
	import { onMount } from 'svelte';
	import type { RenderSource } from '../library';
	import { ShipView, type ShipViewOptions } from '../ship';

	let {
		source,
		view = $bindable(null),
		options = {},
		busy = false,
		class: className = ''
	}: {
		source: RenderSource;
		view?: ShipView | null;
		/** Read once, at mount. */
		options?: ShipViewOptions;
		/** Shows a quiet "rendering…" mark over the view. */
		busy?: boolean;
		class?: string;
	} = $props();

	let canvas: HTMLCanvasElement;
	let box: HTMLDivElement;

	onMount(() => {
		const v = new ShipView(canvas, source, options);
		view = v;
		const resize = () => v.resize(box.clientWidth, box.clientHeight);
		resize();
		const observer = new ResizeObserver(resize);
		observer.observe(box);
		return () => {
			observer.disconnect();
			v.dispose();
			view = null;
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
	></canvas>
	{#if busy}
		<div style="position: absolute; top: 0.5rem; right: 0.75rem; font-size: 0.75rem; opacity: 0.6">
			rendering…
		</div>
	{/if}
</div>
