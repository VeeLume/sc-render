<script lang="ts">
	import { onMount } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import { Button, Segmented, Surface, storedValue } from '@veelume/ui';
	import { AXIS_LOCKS, VIEW_HINT, type AxisLock, type Cell } from '@veelume/sc-render';
	import { Viewport } from '@veelume/sc-render/svelte';
	import { tauriSource } from '@veelume/sc-render/tauri';
	import RenderLog from '$lib/components/RenderLog.svelte';
	import {
		commands,
		type Body,
		type CharacterFile,
		type ChildSpec,
		type InventoryNode,
		type PartLook
	} from '$lib/bindings';

	/**
	 * The player's character from the inventory dump (actor, body, head, hair,
	 * everything worn) as a tree; unticking a node hides it and everything
	 * attached to it from the render.
	 */
	let cell = $state<Cell | null>(null);
	let character = $state<InventoryNode | null>(null);
	let body = $state<Body>('Female');
	/** Natural standing pose, or the skeleton's bind (A-) pose. */
	let natural = $state(true);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let log = $state<string[]>([]);
	/** Saved characters (`.chf`) to take hair, brow, eye and skin colours from. */
	let characterFiles = $state<CharacterFile[]>([]);
	/** The chosen file's path; empty for the materials' own colours. */
	let lookPath = $state('');
	let looks = new Map<string, PartLook>();

	async function loadLook() {
		looks = new Map();
		if (lookPath) {
			const res = await commands.characterLook(lookPath);
			if (res.status === 'ok') looks = new Map(res.data.parts.map((p) => [p.item, p]));
			else error = res.error;
		}
		scheduleRender();
	}

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
			log = await cell.show({ ...res.data, aspect: VIEW_ASPECT }, LIGHTS, rendered, looks, natural);
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

	/** A figure mostly turns. */
	let axisLock = $state<AxisLock>('turn');

	const paneOpen = storedValue('character.pane.open', true, (v): v is boolean => typeof v === 'boolean');
	const paneWidth = storedValue<number | null>(
		'character.pane.width',
		null,
		(v): v is number | null => v === null || typeof v === 'number'
	);

	onMount(() => {
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
			const files = await commands.listCharacterFiles();
			if (files.status === 'ok') {
				characterFiles = files.data;
				lookPath = files.data[0]?.path ?? '';
			}
			await loadLook();
		})();
		return () => clearTimeout(renderTimer);
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

<Surface.Supporting
	title="Character"
	open={paneOpen.value}
	onopen={(next) => (paneOpen.value = next)}
	width={paneWidth.value}
	onresize={(next) => (paneWidth.value = next)}
>
	{#snippet main()}
		<div class="flex h-full min-h-0 gap-3 p-3">
			<aside class="flex w-96 shrink-0 flex-col overflow-hidden rounded-lg border bg-card text-sm">
				<div class="flex h-14 shrink-0 items-center justify-between border-b px-3">
					<h2 class="font-semibold">Parts</h2>
					<Button variant="outline" size="chrome" onclick={showAll} disabled={!hidden.size}>Show all</Button>
				</div>
				<div class="min-h-0 flex-1 overflow-y-auto p-3">
					{#if character}
						<ul>{@render node(character, 0, false)}</ul>
					{:else if !error}
						<p class="text-muted-foreground">Loading the inventory…</p>
					{/if}
				</div>
			</aside>
			<div class="min-w-0 flex-1">
				<Viewport source={tauriSource()} bind:cell {axisLock} {busy} class="rounded-lg border bg-[#1c2632]" />
			</div>
		</div>
	{/snippet}
	{#snippet pane()}
		<!-- A fixed column: nothing here scrolls but the log. -->
		<div class="flex h-full min-w-0 flex-col gap-5 overflow-hidden text-sm">
			<section class="grid gap-3">
				<h3 class="font-medium">View</h3>
				<p class="text-xs text-muted-foreground">{VIEW_HINT}.</p>
				<Segmented
					options={AXIS_LOCKS.map((l) => ({ value: l.value, label: l.label }))}
					value={axisLock}
					onchange={(v) => (axisLock = v as AxisLock)}
				/>
			</section>
			<section class="grid gap-3">
				<h3 class="font-medium">Figure</h3>
				<Segmented
					options={[
						{ value: 'natural', label: 'Natural' },
						{ value: 'bind', label: 'Bind pose' }
					]}
					value={natural ? 'natural' : 'bind'}
					onchange={(v) => {
						natural = v === 'natural';
						rendered = false;
						scheduleRender();
					}}
				/>
				<Segmented
					options={[
						{ value: 'Female', label: 'Female' },
						{ value: 'Male', label: 'Male' }
					]}
					value={body}
					onchange={(v) => setBody(v as Body)}
				/>
				<label class="grid gap-1">
					<span>Colours</span>
					<select class="h-9 rounded-md border bg-background px-2" bind:value={lookPath} onchange={loadLook}>
						<option value="">Material defaults</option>
						{#each characterFiles as f (f.path)}
							<option value={f.path}>{f.name} ({new Date(f.modified * 1000).toLocaleDateString()})</option>
						{/each}
					</select>
				</label>
			</section>
			<RenderLog lines={log} {error} />
		</div>
	{/snippet}
</Surface.Supporting>
