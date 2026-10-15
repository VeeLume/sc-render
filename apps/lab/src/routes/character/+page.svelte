<script lang="ts">
	import { onMount } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import { Cell } from '$lib/render/scene';
	import { AXIS_LOCKS, VIEW_HINT, bindWheelZoom, viewControls, type AxisLock } from '$lib/render/viewControls';
	import { commands, type Body, type ChildSpec, type InventoryNode } from '$lib/bindings';

	/**
	 * The player's character from the inventory dump (actor, body, head, hair,
	 * everything worn) as a tree; unticking a node hides it and everything
	 * attached to it from the render.
	 */
	let canvas: HTMLCanvasElement;
	let cell: Cell | null = null;
	let character = $state<InventoryNode | null>(null);
	let body = $state<Body>('Female');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let log = $state<string[]>([]);
	let showLog = $state(false);
	/** Hidden nodes, by GEID. */
	const hidden = new SvelteSet<string>();
	/** Collapsed tree branches, by GEID. */
	const collapsed = new SvelteSet<string>();

	/** The render's width / height: a standing figure. */
	const VIEW_ASPECT = 3 / 4;
	const LIGHTS = { lightScale: 2, ambient: 0.15, toneMap: 'linear' as const, rimScale: 1, env: 2 };

	function childrenOf(n: InventoryNode): ChildSpec[] {
		return n.attached
			.filter((c) => c.port && c.guid && !hidden.has(c.geid))
			.map((c) => ({ port: c.port!, guid: c.guid!, children: childrenOf(c) }));
	}

	/** Bumped per request; a slower earlier render does not overwrite a newer one. */
	let request = 0;
	let rendered = false;
	async function render() {
		if (!cell || !character?.guid) return;
		const mine = ++request;
		busy = true;
		error = null;
		try {
			const res = await commands.itemScene(character.guid, childrenOf(character), body);
			if (res.status !== 'ok') throw new Error(res.error);
			if (mine !== request) return;
			// Frame the figure in this view, not in an inventory cell.
			log = await cell.show({ ...res.data, aspect: VIEW_ASPECT }, LIGHTS, rendered);
			rendered = true;
		} catch (e) {
			error = String(e);
		} finally {
			if (mine === request) busy = false;
		}
	}

	let renderTimer: ReturnType<typeof setTimeout> | undefined;
	/** Ticking several boxes quickly renders once. */
	function scheduleRender() {
		clearTimeout(renderTimer);
		renderTimer = setTimeout(render, 250);
	}

	function toggle(n: InventoryNode) {
		if (hidden.has(n.geid)) hidden.delete(n.geid);
		else hidden.add(n.geid);
		scheduleRender();
	}

	function showAll() {
		hidden.clear();
		scheduleRender();
	}

	function setBody(b: Body) {
		body = b;
		rendered = false;
		scheduleRender();
	}

	function fit() {
		if (!cell) return;
		const box = canvas.parentElement!;
		const width = Math.min(box.clientWidth, (window.innerHeight - 120) * VIEW_ASPECT);
		cell.resize(width, width / VIEW_ASPECT);
		cell.render();
	}

	/** Turn, pan and zoom the render (see viewControls); a figure mostly turns. */
	let axisLock = $state<AxisLock>('turn');
	const controls = viewControls(
		() => canvas,
		() => cell,
		() => axisLock
	);

	onMount(() => {
		cell = new Cell(canvas);
		fit();
		window.addEventListener('resize', fit);
		const unbindWheel = bindWheelZoom(canvas, () => cell);
		(async () => {
			busy = true;
			const res = await commands.loadCharacter(null);
			busy = false;
			if (res.status !== 'ok') {
				error = res.error;
				return;
			}
			character = res.data;
			if (!character) {
				error = 'The inventory dump has no player character.';
				return;
			}
			await render();
		})();
		return () => {
			window.removeEventListener('resize', fit);
			unbindWheel();
			clearTimeout(renderTimer);
			cell?.dispose();
			cell = null;
		};
	});
</script>

{#snippet node(n: InventoryNode, depth: number, parentHidden: boolean)}
	{@const isHidden = parentHidden || hidden.has(n.geid)}
	{@const drawable = !!n.guid}
	{@const isRoot = depth === 0}
	<li>
		<div class="flex items-center gap-1.5 py-0.5" style="padding-left: {depth * 14}px">
			{#if n.attached.length}
				<button
					class="w-4 text-muted-foreground hover:text-foreground"
					aria-label={collapsed.has(n.geid) ? 'Expand' : 'Collapse'}
					onclick={() => (collapsed.has(n.geid) ? collapsed.delete(n.geid) : collapsed.add(n.geid))}
					>{collapsed.has(n.geid) ? '▸' : '▾'}</button
				>
			{:else}
				<span class="w-4"></span>
			{/if}
			<input
				type="checkbox"
				checked={!hidden.has(n.geid)}
				disabled={isRoot || !drawable || parentHidden}
				onchange={() => toggle(n)}
				title={isRoot ? 'The character itself' : drawable ? 'Show or hide' : 'Not drawn (no geometry)'}
			/>
			<span class="truncate {isHidden || !drawable ? 'text-muted-foreground' : ''}" title={n.record}>
				{n.label}
			</span>
			{#if n.port}<span class="truncate text-xs text-muted-foreground/70">{n.port}</span>{/if}
		</div>
		{#if n.attached.length && !collapsed.has(n.geid)}
			<ul>
				{#each n.attached as c (c.geid)}
					{@render node(c, depth + 1, isHidden)}
				{/each}
			</ul>
		{/if}
	</li>
{/snippet}

<div class="flex h-full gap-4 p-[var(--density-padding)]">
	<aside class="w-96 shrink-0 overflow-y-auto rounded-xl border p-3 text-sm">
		<div class="mb-2 flex items-center justify-between">
			<h1 class="font-medium">Character</h1>
			<button class="rounded border px-2 py-0.5 text-xs hover:bg-muted" onclick={showAll} disabled={!hidden.size}
				>Show all</button
			>
		</div>
		{#if character}
			<ul>{@render node(character, 0, false)}</ul>
		{:else if !error}
			<p class="text-muted-foreground">Loading the inventory…</p>
		{/if}
	</aside>
	<main class="min-w-0 flex-1 overflow-y-auto">
		<div class="relative w-fit rounded-xl border bg-[#1c2632]">
			<canvas
				bind:this={canvas}
				class="block cursor-grab touch-none active:cursor-grabbing"
				title={VIEW_HINT}
				{...controls}
			></canvas>
			{#if busy}<div class="absolute top-2 right-3 text-xs text-muted-foreground">rendering…</div>{/if}
		</div>
		<div class="mt-2 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
			<span>{VIEW_HINT}.</span>
			<div class="flex overflow-hidden rounded border" role="radiogroup" aria-label="Rotation axis">
				{#each AXIS_LOCKS as lock (lock.value)}
					<button
						role="radio"
						aria-checked={axisLock === lock.value}
						title={lock.hint}
						class="px-2 py-0.5 {axisLock === lock.value ? 'bg-muted text-foreground' : 'hover:bg-muted/50'}"
						onclick={() => (axisLock = lock.value)}>{lock.label}</button
					>
				{/each}
			</div>
			<div class="flex overflow-hidden rounded border" role="radiogroup" aria-label="Body">
				{#each ['Female', 'Male'] as const as b (b)}
					<button
						role="radio"
						aria-checked={body === b}
						class="px-2 py-0.5 {body === b ? 'bg-muted text-foreground' : 'hover:bg-muted/50'}"
						onclick={() => setBody(b)}>{b}</button
					>
				{/each}
			</div>
			<button class="underline-offset-2 hover:underline" onclick={() => (showLog = !showLog)}
				>{showLog ? 'Hide' : 'Show'} render log</button
			>
		</div>
		{#if error}<pre class="mt-3 text-xs whitespace-pre-wrap text-red-400">{error}</pre>{/if}
		{#if showLog}<pre class="mt-3 text-xs whitespace-pre-wrap text-muted-foreground">{log.join('\n')}</pre>{/if}
	</main>
</div>
