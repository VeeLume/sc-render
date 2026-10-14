<script lang="ts">
	import { onMount } from 'svelte';
	import { Cell, type ToneMap } from '$lib/render/scene';
	import { commands, type ChildSpec, type InventoryNode, type ItemSummary, type SceneItem } from '$lib/bindings';

	/**
	 * Dev regression set: items calibrated against in-game screenshots,
	 * rendered and snapshotted on start (record names; `inventory:` takes the
	 * first inventory item with that label, with its real attachments).
	 */
	const DEV_SET = [
		'behr_rifle_ballistic_01_sf01',
		'gmni_pistol_ballistic_01_pink_cian01',
		'POWR_AMRS_S02_JS400_SCItem',
		'can_drink_3_energy_a',
		'inventory:Bosco Weapon Display Rack'
	];

	let canvas: HTMLCanvasElement;
	let cell: Cell | null = null;
	let current = $state<SceneItem | null>(null);
	let log = $state<string[]>([]);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let lightScale = $state(2.5);
	let ambient = $state(0.15);
	let toneMap = $state<ToneMap>('linear');
	let rimScale = $state(1);
	let env = $state(2);

	let query = $state('');
	let results = $state<ItemSummary[]>([]);
	let searching = $state(false);
	let inventory = $state<InventoryNode[] | null>(null);
	let inventoryFilter = $state('');

	const aspect = () => current?.aspect ?? 1;

	/** Canvas sized to the item's cell (width / height = aspect). */
	function fit() {
		if (!cell) return;
		const width = Math.min(canvas.parentElement!.clientWidth, 640 * aspect());
		cell.resize(width, width / aspect());
		cell.render();
	}

	function safeName(s: string) {
		return s.replace(/[^A-Za-z0-9_-]+/g, '_');
	}

	async function show(guid: string, children: ChildSpec[] | null = null, snapshotName?: string) {
		busy = true;
		error = null;
		try {
			const res = await commands.itemScene(guid, children);
			if (res.status !== 'ok') throw new Error(res.error);
			current = res.data;
			fit();
			log = await cell!.show(res.data, { lightScale, ambient, toneMap, rimScale, env });
			const name = snapshotName ?? safeName(res.data.root.record);
			await snapshot(name);
			await commands.saveLog(name, log.join('\n'));
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	/** An inventory item's real attachments, as the scene's children. */
	function childrenOf(n: InventoryNode): ChildSpec[] {
		return n.attached
			.filter((c) => c.port && c.guid)
			.map((c) => ({ port: c.port!, guid: c.guid!, children: childrenOf(c) }));
	}

	function showNode(n: InventoryNode, snapshotName?: string) {
		if (n.guid) return show(n.guid, childrenOf(n), snapshotName);
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
		if (!cell || !current) return;
		cell.setLights(current, { lightScale, ambient, toneMap, rimScale, env });
		cell.render();
	}

	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	function onSearch() {
		clearTimeout(searchTimer);
		searchTimer = setTimeout(async () => {
			if (query.trim().length < 2) {
				results = [];
				return;
			}
			searching = true;
			const res = await commands.searchItems(query);
			searching = false;
			if (res.status === 'ok') results = res.data;
			else error = res.error;
		}, 250);
	}

	async function loadInventory() {
		const res = await commands.loadInventory(null);
		if (res.status === 'ok') inventory = res.data;
		else error = res.error;
	}

	/** Inventory items (at any depth) whose label matches the filter. */
	function matching(nodes: InventoryNode[], filter: string, out: InventoryNode[] = []): InventoryNode[] {
		for (const n of nodes) {
			if (out.length >= 150) break;
			if (n.guid && (!filter || n.label.toLowerCase().includes(filter))) out.push(n);
			// Hangar decorations hang on the hangar's ports, so search both.
			matching(n.attached, filter, out);
			matching(n.contents, filter, out);
		}
		return out;
	}
	const inventoryHits = $derived(inventory ? matching(inventory, inventoryFilter.trim().toLowerCase()) : []);

	async function devRun() {
		for (const entry of DEV_SET) {
			if (entry.startsWith('inventory:')) {
				if (!inventory) await loadInventory();
				const label = entry.slice('inventory:'.length).toLowerCase();
				const node = inventory && matching(inventory, label)[0];
				if (node) await showNode(node, safeName(`inv_${node.label}`));
				continue;
			}
			const res = await commands.searchItems(entry);
			const hit = res.status === 'ok' ? res.data.find((h) => h.record.toLowerCase() === entry.toLowerCase()) : null;
			if (hit) await show(hit.guid);
		}
	}

	onMount(() => {
		cell = new Cell(canvas);
		fit();
		window.addEventListener('resize', fit);
		if (import.meta.env.DEV) devRun();
		return () => window.removeEventListener('resize', fit);
	});
</script>

<div class="flex h-full gap-4 p-[var(--density-padding)]">
	<aside class="flex w-72 shrink-0 flex-col gap-3 overflow-hidden">
		<input
			class="w-full rounded border bg-transparent px-3 py-2 text-sm"
			placeholder="Search items…"
			bind:value={query}
			oninput={onSearch}
		/>
		<div class="min-h-0 flex-1 space-y-0.5 overflow-y-auto">
			{#if searching}<div class="px-3 text-xs text-muted-foreground">searching (the catalog loads on start)…</div>{/if}
			{#each results as r (r.guid)}
				<button
					class="block w-full rounded px-3 py-1.5 text-left text-sm hover:bg-muted {current?.id === r.guid
						? 'bg-muted font-medium'
						: ''}"
					onclick={() => show(r.guid)}
				>
					{r.label}
					<span class="block text-xs text-muted-foreground">{r.kind} · {r.record}</span>
				</button>
			{/each}
			<div class="mt-4 border-t pt-3">
				{#if inventory}
					<input
						class="mb-2 w-full rounded border bg-transparent px-3 py-1.5 text-sm"
						placeholder="Filter inventory…"
						bind:value={inventoryFilter}
					/>
					{#each inventoryHits as n (n.geid)}
						<button
							class="block w-full rounded px-3 py-1.5 text-left text-sm hover:bg-muted"
							onclick={() => showNode(n)}
						>
							{n.label}
							{#if n.attached.length}<span class="block text-xs text-muted-foreground"
									>{n.attached.map((a) => a.label).join(', ')}</span
								>{/if}
						</button>
					{/each}
				{:else}
					<button class="w-full rounded border px-3 py-1.5 text-sm hover:bg-muted" onclick={loadInventory}
						>Load Starlume inventory</button
					>
				{/if}
			</div>
		</div>
		<div class="space-y-3 text-xs">
			<label class="block"
				>Light scale {lightScale.toFixed(2)}
				<input type="range" min="0.1" max="10" step="0.05" bind:value={lightScale} oninput={relight} class="w-full" />
			</label>
			<label class="block"
				>Ambient {ambient.toFixed(3)}
				<input type="range" min="0" max="1" step="0.005" bind:value={ambient} oninput={relight} class="w-full" />
			</label>
			<label class="block"
				>Reflections {env.toFixed(2)}
				<input type="range" min="0" max="4" step="0.05" bind:value={env} oninput={relight} class="w-full" />
			</label>
		</div>
	</aside>
	<main class="min-w-0 flex-1 overflow-y-auto">
		<div class="relative rounded-xl border bg-[#1c2632]" style="max-width: {640 * aspect()}px">
			<canvas bind:this={canvas} class="block w-full" style="aspect-ratio: {aspect()}"></canvas>
			{#if busy}<div class="absolute inset-0 grid place-items-center text-sm text-muted-foreground">loading…</div>{/if}
		</div>
		{#if current}<h2 class="mt-3 text-sm font-medium">{current.label}</h2>{/if}
		{#if error}<pre class="mt-3 text-xs whitespace-pre-wrap text-red-400">{error}</pre>{/if}
		<pre class="mt-3 text-xs whitespace-pre-wrap text-muted-foreground">{log.join('\n')}</pre>
	</main>
</div>
