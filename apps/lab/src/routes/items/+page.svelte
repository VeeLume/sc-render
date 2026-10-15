<script module lang="ts">
	let devRunDone = false;
</script>

<script lang="ts">
	/**
	 * Every renderable item, and the items of the Starlume inventory dump with
	 * their real attachments, as one list. The selected item renders in the
	 * free space; beside it the supporting pane shows its inventory icon (the
	 * game's framing, untouched by the view controls), facts, render controls
	 * and the render log.
	 */
	import { onMount, untrack } from 'svelte';
	import {
		Placeholder,
		Segmented,
		Surface,
		createBrowseState,
		storedValue,
		type FacetDef,
		type Row,
		type SortDef
	} from '@veelume/ui';
	import { AXIS_LOCKS, VIEW_HINT, type AxisLock, type Cell } from '@veelume/sc-render';
	import { Viewport } from '@veelume/sc-render/svelte';
	import { tauriSource } from '@veelume/sc-render/tauri';
	import Slider from '$lib/components/Slider.svelte';
	import Facts from '$lib/components/Facts.svelte';
	import RenderLog from '$lib/components/RenderLog.svelte';
	import { commands, type Body, type ChildSpec, type InventoryNode, type SceneItem } from '$lib/bindings';

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
		'inventory:Bosco Weapon Display Rack',
		'srvl_combat_medium_core_03_01_01',
		'qrt_specialist_medium_helmet_01_01_16',
		// Valerie's validation pass (2026-10-14)
		'inventory:Cataby Backpack',
		'inventory:Sol-III Flight Suit',
		'grin_multitool_01_black01',
		'rsi_deckcrew_armor_light_helmet_01_01_10',
		'eld_shirt_04_drake_04',
		'Harvestable_Mineral_1H_Beradom',
		'Carryable_1H_CY_Physical_Currency_Scrip_Merc_1',
		'none_pistol_ballistic_01',
		'vgl_flightsuit_helmet_01_03_01',
		'crlf_consumable_healing_01',
		// The player's own character: the actor's skeleton, body, head, hair
		// and everything worn.
		'inventory:Player',
		// The head alone (hair, brows, lashes, eyes) for checking the face.
		'inventory:PU_Protos_Head',
		// Both records are named "Invictus Flight Jacket"; the dark brown
		// leather one seen in game is _04.
		'dmc_jacket_13_01_01',
		'dmc_jacket_04_01_01'
	];

	/** Icon pixels per inventory grid unit. */
	const ICON_UNIT = 56;

	type ItemRow = Row & {
		guid: string;
		record: string;
		/** `Type/SubType`. */
		kind: string;
		/** Set for an inventory item: rendered with its real attachments. */
		node?: InventoryNode;
	};

	const browse = createBrowseState({
		q: { kind: 'text' },
		source: { kind: 'one', default: 'all' },
		type: { kind: 'one', default: 'all' },
		sort: { kind: 'one', default: 'name', narrows: false },
		item: { kind: 'one', default: '', narrows: false }
	});

	let catalogRows = $state<ItemRow[]>([]);
	let inventoryRows = $state<ItemRow[]>([]);
	let status = $state<'loading' | 'ready' | 'error'>('loading');
	const rows = $derived([...inventoryRows, ...catalogRows]);
	const rowByKey = $derived(new Map(rows.map((r) => [r.key, r])));

	const types = $derived([...new Set(catalogRows.map((r) => r.kind.split('/')[0]).filter(Boolean))].sort());
	const sorts: SortDef<ItemRow>[] = [
		{ value: 'name', label: 'Name', compare: (a, b) => a.title.localeCompare(b.title) },
		{ value: 'type', label: 'Type', compare: (a, b) => a.kind.localeCompare(b.kind) || a.title.localeCompare(b.title) }
	];
	const facets: FacetDef<ItemRow>[] = $derived([
		{
			id: 'source',
			label: 'Source',
			mode: 'one',
			options: [
				{ value: 'all', label: 'All' },
				{ value: 'inventory', label: 'My inventory', test: (r) => !!r.node },
				{ value: 'catalog', label: 'Game data', test: (r) => !r.node }
			]
		},
		{
			id: 'type',
			label: 'Type',
			mode: 'one',
			options: [
				{ value: 'all', label: 'All' },
				...types.map((t) => ({ value: t, label: t, test: (r: ItemRow) => r.kind.split('/')[0] === t }))
			]
		}
	]);
	const descriptor = $derived({
		sources: () => rows,
		derive: (rs: ItemRow[]) => rs,
		searchIn: (r: ItemRow) => [r.title, r.record, r.kind],
		facets,
		sorts
	});

	const active = $derived(browse.values.item || null);

	let cell = $state<Cell | null>(null);
	let current = $state<SceneItem | null>(null);
	let shownRow = $state<ItemRow | null>(null);
	let icon = $state<string | null>(null);
	let log = $state<string[]>([]);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let axisLock = $state<AxisLock>('free');
	/** Armor and clothing are shown for this body. */
	let body = $state<Body>('Female');
	let lightScale = $state(2);
	let ambient = $state(0.15);
	let env = $state(2);
	const lights = () => ({ lightScale, ambient, toneMap: 'linear' as const, rimScale: 1, env });

	const paneOpen = storedValue('items.pane.open', true, (v): v is boolean => typeof v === 'boolean');
	const paneWidth = storedValue<number | null>(
		'items.pane.width',
		null,
		(v): v is number | null => v === null || typeof v === 'number'
	);

	function safeName(s: string) {
		return s.replace(/[^A-Za-z0-9_-]+/g, '_');
	}

	/** An inventory item's real attachments, as the scene's children. */
	function childrenOf(n: InventoryNode): ChildSpec[] {
		return n.attached
			.filter((c) => c.port && c.guid)
			.map((c) => ({ port: c.port!, guid: c.guid!, children: childrenOf(c) }));
	}

	/** Bumped per render; a slower earlier render does not overwrite a newer one. */
	let request = 0;
	async function render(row: ItemRow, snapshotName?: string) {
		if (!cell) return;
		const mine = ++request;
		busy = true;
		error = null;
		try {
			const res = await commands.itemScene(row.guid, row.node ? childrenOf(row.node) : null, body);
			if (res.status !== 'ok') throw new Error(res.error);
			if (mine !== request || !cell) return;
			current = res.data;
			shownRow = row;
			log = await cell.show(res.data, lights());
			refreshIcon();
			if (snapshotName) {
				await snapshot(snapshotName);
				await commands.saveLog(snapshotName, log.join('\n'));
			}
		} catch (e) {
			error = String(e);
			// Dev runs read the logs, not the window.
			if (snapshotName) await commands.saveLog(snapshotName, [...log, `ERROR ${error}`].join('\n'));
		} finally {
			if (mine === request) busy = false;
		}
	}

	function refreshIcon() {
		if (!cell || !current) return;
		const units = current.grid?.[1] ?? 2;
		icon = cell.capture(units * ICON_UNIT * window.devicePixelRatio).toDataURL('image/png');
	}

	/** Dev aid: the inventory framing as a PNG under the app data folder. */
	async function snapshot(name: string) {
		if (!cell || !current) return;
		const shot = cell.capture(Math.min(640, 1280 / current.aspect));
		const blob = await new Promise<Blob | null>((r) => shot.toBlob(r, 'image/png'));
		if (!blob) return;
		const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
		const res = await commands.saveSnapshot(name, bytes);
		if (res.status === 'ok') log = [...log, `snapshot ${res.data}`];
	}

	function relight(icon = false) {
		if (!cell || !current) return;
		cell.setLights(current, lights());
		cell.render();
		if (icon) refreshIcon();
	}

	// Selecting a row renders it.
	let lastActive: string | null = null;
	$effect(() => {
		const key = active;
		const row = key ? rowByKey.get(key) : undefined;
		untrack(() => {
			if (!row || !cell || key === lastActive) return;
			lastActive = key;
			render(row);
		});
	});

	async function load() {
		const [items, inventory] = await Promise.all([commands.listItems(), commands.loadInventory(null)]);
		if (items.status !== 'ok') {
			error = items.error;
			status = 'error';
			return;
		}
		const kindOf = new Map(items.data.map((i) => [i.guid, i.kind]));
		catalogRows = items.data.map((i) => ({
			key: i.guid,
			title: i.label,
			subtitle: `${i.kind} · ${i.record}`,
			guid: i.guid,
			record: i.record,
			kind: i.kind
		}));
		if (inventory.status === 'ok') {
			const out: ItemRow[] = [];
			// Depth first, like the inventory: attached parts after their holder.
			const walk = (nodes: InventoryNode[]) => {
				for (const n of nodes) {
					if (n.guid) {
						const kind = kindOf.get(n.guid) ?? '';
						out.push({
							key: `inv:${n.geid}`,
							title: n.label,
							subtitle: n.attached.length ? n.attached.map((a) => a.label).join(', ') : `${kind} · ${n.record}`,
							badge: 'Inventory',
							guid: n.guid,
							record: n.record,
							kind,
							node: n
						});
					}
					// Hangar decorations hang on the hangar's ports, so walk both.
					walk(n.attached);
					walk(n.contents);
				}
			};
			walk(inventory.data);
			inventoryRows = out;
		}
		status = 'ready';
	}

	async function devRun() {
		for (const entry of DEV_SET) {
			// Leaving the page (cell disposed) ends the run.
			if (!cell) return;
			if (entry.startsWith('inventory:')) {
				const label = entry.slice('inventory:'.length).toLowerCase();
				const row = inventoryRows.find((r) => r.title.toLowerCase().includes(label) || r.record.toLowerCase().includes(label));
				if (row) await render(row, safeName(`inv_${row.title}`));
				continue;
			}
			const row = catalogRows.find((r) => r.record.toLowerCase() === entry.toLowerCase());
			if (row) await render(row, safeName(row.record));
		}
	}

	onMount(() => {
		(async () => {
			await load();
			// Once per app session, not on every visit to this page.
			if (import.meta.env.DEV && !devRunDone) {
				devRunDone = true;
				await devRun();
			}
		})();
	});

	const facts = $derived(
		current && shownRow
			? [
					{ label: 'Name', value: current.label },
					{ label: 'Record', value: shownRow.record, mono: true },
					{ label: 'Type', value: shownRow.kind || '—' },
					{ label: 'Inventory cell', value: current.grid ? `${current.grid[0]} × ${current.grid[1]}` : '—' },
					{ label: 'GUID', value: shownRow.guid, mono: true }
				]
			: []
	);
</script>

<Surface.Root {descriptor} {browse} selected={active}>
	<Surface.Split>
		{#snippet list()}
			<Surface.List {status} onselect={(r) => browse.set('item', r.key)} />
		{/snippet}

		{#snippet detail()}
			<Surface.Supporting
				title="Details"
				open={paneOpen.value}
				onopen={(next) => (paneOpen.value = next)}
				width={paneWidth.value}
				onresize={(next) => (paneWidth.value = next)}
			>
				{#snippet main()}
					<div class="relative h-full">
						<Viewport source={tauriSource()} bind:cell {axisLock} {busy} class="rounded-lg border bg-[#1c2632]" />
						{#if !current && !busy}
							<div class="pointer-events-none absolute inset-0 grid">
								<Placeholder>Pick an item to render it.</Placeholder>
							</div>
						{/if}
					</div>
				{/snippet}
				{#snippet pane()}
					<!-- A fixed column: nothing here scrolls but the log. -->
					<div class="flex h-full min-w-0 flex-col gap-5 overflow-hidden text-sm">
						{#if current}
							<section class="grid gap-2">
								<h3 class="font-medium">Inventory icon</h3>
								{#if icon}
									<img
										src={icon}
										alt="{current.label} as its inventory cell shows it"
										class="max-w-full rounded-md border bg-[#1c2632]"
										style="width: {(current.grid?.[0] ?? 2 * current.aspect) * ICON_UNIT}px"
									/>
								{/if}
							</section>
							<Facts {facts} />
						{/if}

						<section class="grid gap-3">
							<h3 class="font-medium">View</h3>
							<p class="text-xs text-muted-foreground">{VIEW_HINT}.</p>
							<Segmented
								options={AXIS_LOCKS.map((l) => ({ value: l.value, label: l.label }))}
								value={axisLock}
								onchange={(v) => (axisLock = v as AxisLock)}
							/>
							<Segmented
								options={[
									{ value: 'Female', label: 'Female' },
									{ value: 'Male', label: 'Male' }
								]}
								value={body}
								onchange={(v) => {
									body = v as Body;
									if (shownRow) render(shownRow);
								}}
							/>
						</section>

						<section class="grid gap-3">
							<h3 class="font-medium">Lighting</h3>
							<Slider
								label="Light scale"
								bind:value={lightScale}
								min={0.1}
								max={10}
								step={0.05}
								oninput={() => relight()}
								onchange={() => relight(true)}
							/>
							<Slider
								label="Ambient"
								bind:value={ambient}
								min={0}
								max={1}
								step={0.005}
								digits={3}
								oninput={() => relight()}
								onchange={() => relight(true)}
							/>
							<Slider
								label="Reflections"
								bind:value={env}
								min={0}
								max={4}
								step={0.05}
								oninput={() => relight()}
								onchange={() => relight(true)}
							/>
						</section>

						<RenderLog lines={log} {error} />
					</div>
				{/snippet}
			</Surface.Supporting>
		{/snippet}
	</Surface.Split>
</Surface.Root>
