<script lang="ts">
	/**
	 * Player ships with their stock loadout and interior, drawn in the hangar
	 * view, sliced on demand, with the damage model on top: colour the hull by
	 * damage part or see through it,
	 * show the component boxes, click the hull to fire the chosen gun at that
	 * point (shield, armour, hull part, penetration, components reached), or
	 * pick a component and sweep the directions it can be hit from.
	 */
	import { untrack } from 'svelte';
	import { onMount } from 'svelte';
	import * as THREE from 'three';
	import { Placeholder, Segmented, Surface, createBrowseState, storedValue, type Row, type SortDef } from '@veelume/ui';
	import type { ShipMode, SliceAxis } from '@veelume/sc-render';
	import type { Hangar } from '$lib/hangar';
	import HangarView from '$lib/components/HangarView.svelte';
	import Slider from '$lib/components/Slider.svelte';
	import Facts from '$lib/components/Facts.svelte';
	import RenderLog from '$lib/components/RenderLog.svelte';
	import {
		commands,
		type ExposureDirection,
		type ShipComponent,
		type HangarShip,
		type ShotResult,
		type WeaponSummary
	} from '$lib/bindings';

	type ShipRow = Row & { guid: string; record: string; listing: string };

	const browse = createBrowseState({
		q: { kind: 'text' },
		sort: { kind: 'one', default: 'name', narrows: false },
		ship: { kind: 'one', default: '', narrows: false }
	});
	let rows = $state<ShipRow[]>([]);
	let status = $state<'loading' | 'ready' | 'error'>('loading');
	const rowByKey = $derived(new Map(rows.map((r) => [r.key, r])));
	const sorts: SortDef<ShipRow>[] = [
		{ value: 'name', label: 'Name', compare: (a, b) => a.title.localeCompare(b.title) }
	];
	const descriptor = $derived({
		sources: () => rows,
		derive: (rs: ShipRow[]) => rs,
		searchIn: (r: ShipRow) => [r.title, r.record],
		facets: [],
		sorts
	});
	const active = $derived(browse.values.ship || null);

	let hangar = $state<Hangar | null>(null);
	let view = $state<HangarShip | null>(null);
	let log = $state<string[]>([]);
	let busy = $state(false);
	let error = $state<string | null>(null);

	let mode = $state<ShipMode>('holo');
	let holoOpacity = $state(0.22);
	let showBoxes = $state(false);
	let allBoxes = $state(false);
	let showShell = $state(false);
	let showInterior = $state(true);
	let sliceAxis = $state<SliceAxis>('off');
	let sliceAt = $state(0.5);
	let sliceFlip = $state(false);
	const applySlice = () => hangar?.setSlice(sliceAxis, sliceAt, sliceFlip);
	/** What a click does. */
	let tool = $state<'shoot' | 'select'>('shoot');

	let weapons = $state<WeaponSummary[]>([]);
	const weaponPick = storedValue('ships.weapon', '', (v): v is string => typeof v === 'string');
	const weapon = $derived(weapons.find((w) => w.record === weaponPick.value) ?? null);
	let armor = $state(1);
	let shield = $state(0);

	let shot = $state<ShotResult | null>(null);
	let selected = $state<ShipComponent | null>(null);
	let exposure = $state<ExposureDirection[] | null>(null);
	/** Penetration the exposure colours by: the gun's, else 1.7 m (a size-5 cannon). */
	const reach = $derived(weapon?.penetration ?? 1.7);

	const paneOpen = storedValue('ships.pane.open', true, (v): v is boolean => typeof v === 'boolean');
	const paneWidth = storedValue<number | null>(
		'ships.pane.width',
		null,
		(v): v is number | null => v === null || typeof v === 'number'
	);

	let request = 0;
	async function render(row: ShipRow) {
		if (!hangar) return;
		const mine = ++request;
		busy = true;
		error = null;
		shot = null;
		selected = null;
		exposure = null;
		try {
			const res = await commands.shipView(row.guid);
			if (res.status !== 'ok') throw new Error(res.error);
			if (mine !== request || !hangar) return;
			log = await hangar.show(res.data);
			view = res.data;
			hangar.setHoloOpacity(holoOpacity);
			await hangar.setMode(mode);
			hangar.setBoxes(showBoxes);
			hangar.setAllBoxes(allBoxes);
			hangar.setShell(showShell);
			hangar.setInterior(showInterior);
			applySlice();
		} catch (e) {
			error = String(e);
		} finally {
			if (mine === request) busy = false;
		}
	}

	let lastActive: string | null = null;
	$effect(() => {
		const key = active;
		const row = key ? rowByKey.get(key) : undefined;
		const h = hangar;
		untrack(() => {
			if (!row || !h || key === lastActive) return;
			lastActive = key;
			render(row);
		});
	});

	async function pick(x: number, y: number) {
		if (!hangar || !view) return;
		if (tool === 'select') {
			const i = hangar.componentAt(x, y);
			selectComponent(i == null ? null : (view.components.find((c) => c.index === i) ?? null));
			return;
		}
		if (!weapon) {
			error = 'Pick a gun to shoot with.';
			return;
		}
		const ray = hangar.rayAt(x, y);
		const res = await commands.shipHit(view.scene.id, {
			origin: ray.origin.toArray() as [number, number, number],
			direction: ray.direction.toArray() as [number, number, number],
			weapon: weapon.record,
			armor,
			shield
		});
		if (res.status !== 'ok') {
			error = res.error;
			return;
		}
		error = null;
		shot = res.data;
		exposure = null;
		hangar.showShot(ray.origin, ray.direction, res.data);
	}

	function selectComponent(c: ShipComponent | null) {
		selected = c;
		exposure = null;
		hangar?.select(c?.index ?? null);
	}

	async function sweep() {
		if (!hangar || !view || !selected) return;
		busy = true;
		const res = await commands.shipExposure(view.scene.id, selected.index, 1500);
		busy = false;
		if (res.status !== 'ok') {
			error = res.error;
			return;
		}
		exposure = res.data;
		shot = null;
		hangar.showExposure(selected, res.data, reach);
	}

	onMount(() => {
		(async () => {
			const [ships, guns] = await Promise.all([commands.listShips(), commands.shipWeapons()]);
			if (ships.status !== 'ok') {
				error = ships.error;
				status = 'error';
				return;
			}
			rows = ships.data.map((s) => ({
				key: s.guid,
				title: s.label,
				subtitle: `${s.listing} · ${s.record}`,
				guid: s.guid,
				record: s.record,
				listing: s.listing
			}));
			if (guns.status === 'ok') weapons = guns.data;
			status = 'ready';
		})();
	});

	const fmt = (n: number, d = 1) => n.toLocaleString(undefined, { maximumFractionDigits: d, minimumFractionDigits: d });
	const hullHp = $derived(view ? view.parts.reduce((s, p) => s + p.hp, 0) : 0);
	const facts = $derived(
		view
			? [
					{ label: 'Name', value: view.scene.label },
					{ label: 'Hull HP', value: fmt(hullHp, 0) },
					{ label: 'Damage parts', value: String(view.parts.filter((p) => p.name).length) },
					{ label: 'Components', value: `${view.components.filter((c) => c.health != null).length} damageable of ${view.components.length}` }
				]
			: []
	);
	const partSwatch = (name: string) => {
		const i = hangar?.parts.indexOf(name) ?? -1;
		return i < 0 ? '#6a6f78' : `#${new THREE.Color().setHSL((i * 0.618034) % 1, 0.55, 0.55).getHexString()}`;
	};
	const sizes = $derived([...new Set(weapons.map((w) => w.size))].sort((a, b) => a - b));
	const best = $derived(exposure ? exposure.slice(0, 5) : []);
	const within = $derived(exposure ? exposure.filter((e) => e.depth <= reach).length : 0);
</script>

<Surface.Root {descriptor} {browse} selected={active}>
	<Surface.Split>
		{#snippet list()}
			<Surface.List {status} onselect={(r) => browse.set('ship', r.key)} />
		{/snippet}

		{#snippet detail()}
			<Surface.Supporting
				title="Damage"
				open={paneOpen.value}
				onopen={(next) => (paneOpen.value = next)}
				width={paneWidth.value}
				onresize={(next) => (paneWidth.value = next)}
			>
				{#snippet main()}
					<div class="relative h-full">
						<HangarView bind:hangar {busy} onpick={pick} />
						{#if !view && !busy}
							<div class="pointer-events-none absolute inset-0 grid">
								<Placeholder>Pick a ship to render it.</Placeholder>
							</div>
						{/if}
					</div>
				{/snippet}
				{#snippet pane()}
					<div class="flex h-full min-w-0 flex-col gap-5 overflow-y-auto text-sm">
						{#if view}<Facts {facts} />{/if}

						<section class="grid gap-3">
							<h3 class="font-medium">Hull</h3>
							<Segmented
								options={[
									{ value: 'holo', label: 'Holo' },
									{ value: 'materials', label: 'Materials' },
									{ value: 'parts', label: 'Parts' },
									{ value: 'xray', label: 'X-ray' }
								]}
								value={mode}
								onchange={async (v) => {
									mode = v as ShipMode;
									busy = true;
									await hangar?.setMode(mode);
									busy = false;
								}}
							/>
							{#if mode === 'holo'}
								<Slider
									label="Hull opacity"
									bind:value={holoOpacity}
									min={0}
									max={1}
									step={0.01}
									oninput={() => hangar?.setHoloOpacity(holoOpacity)}
								/>
							{/if}
							<label class="flex items-center gap-2">
								<input type="checkbox" bind:checked={showBoxes} onchange={() => hangar?.setBoxes(showBoxes)} />
								Component boxes
							</label>
							<label class="flex items-center gap-2 pl-5">
								<input type="checkbox" bind:checked={allBoxes} disabled={!showBoxes} onchange={() => hangar?.setAllBoxes(allBoxes)} />
								Every item, not only the damageable
							</label>
							<label class="flex items-center gap-2">
								<input type="checkbox" bind:checked={showShell} onchange={() => hangar?.setShell(showShell)} />
								Collision shell (what shots hit)
							</label>
							{#if view && mode !== 'materials'}
								<ul class="grid max-h-40 gap-0.5 overflow-y-auto text-xs">
									{#each view.parts.filter((p) => p.name) as p (p.name)}
										<li class="flex items-center gap-2">
											<span class="size-3 shrink-0 rounded-sm" style="background: {partSwatch(p.name)}"></span>
											<span class="truncate">{p.name}</span>
											<span class="ml-auto text-muted-foreground tabular-nums">{fmt(p.hp, 0)} HP</span>
										</li>
									{/each}
								</ul>
							{/if}
						</section>

						<section class="grid gap-3">
							<h3 class="font-medium">Slice</h3>
							<label class="flex items-center gap-2">
								<input type="checkbox" bind:checked={showInterior} onchange={() => hangar?.setInterior(showInterior)} />
								Interior <span class="text-xs text-muted-foreground">(in X-ray or when sliced)</span>
							</label>
							<Segmented
								options={[
									{ value: 'off', label: 'Off' },
									{ value: 'z', label: 'Deck' },
									{ value: 'y', label: 'Frame' },
									{ value: 'x', label: 'Side' }
								]}
								value={sliceAxis}
								onchange={(v) => {
									sliceAxis = v as SliceAxis;
									applySlice();
								}}
							/>
							{#if sliceAxis !== 'off'}
								<Slider label="Cut at" bind:value={sliceAt} min={0} max={1} step={0.005} digits={3} oninput={applySlice} />
								<label class="flex items-center gap-2">
									<input type="checkbox" bind:checked={sliceFlip} onchange={applySlice} />
									Keep the other side
								</label>
							{/if}
						</section>

						<section class="grid gap-3">
							<h3 class="font-medium">Click</h3>
							<Segmented
								options={[
									{ value: 'shoot', label: 'Shoot' },
									{ value: 'select', label: 'Select component' }
								]}
								value={tool}
								onchange={(v) => (tool = v as 'shoot' | 'select')}
							/>
							<label class="grid gap-1">
								<span>Gun</span>
								<select class="h-9 rounded-md border bg-background px-2" bind:value={weaponPick.value}>
									<option value="">—</option>
									{#each sizes as s (s)}
										<optgroup label="Size {s}">
											{#each weapons.filter((w) => w.size === s) as w (w.record)}
												<option value={w.record}>{w.label}{w.penetration ? ` · ${fmt(w.penetration, 2)} m` : ''}</option>
											{/each}
										</optgroup>
									{/each}
								</select>
							</label>
							<Slider label="Armour" bind:value={armor} min={0} max={1} step={0.01} />
							<Slider label="Shield face" bind:value={shield} min={0} max={1} step={0.01} />
						</section>

						{#if shot}
							<section class="grid gap-2">
								<h3 class="font-medium">Shot</h3>
								{#if shot.point}
									<Facts
										facts={[
											{ label: 'Part', value: shot.part || '—' },
											{ label: 'Shield took', value: fmt(shot.shield_taken, 2) },
											{ label: 'Passed', value: fmt(shot.passed, 2) },
											{ label: 'Past armour', value: fmt(shot.after_armor, 2) },
											{ label: 'Armour wear', value: fmt(shot.armor_wear, 2) },
											{ label: 'Hull damage', value: fmt(shot.hull_damage, 2) },
											{ label: 'Penetration', value: `${fmt(shot.depth, 3)} m` }
										]}
									/>
									{#each shot.components as c (c.index)}
										<p class="text-xs">
											<span class="text-[#ff5a4f]">■</span>
											{c.label} at {fmt(c.depth, 2)} m: {fmt(c.damage, 1)}
										</p>
									{:else}
										<p class="text-xs text-muted-foreground">No component reached.</p>
									{/each}
								{:else}
									<p class="text-muted-foreground">Missed the hull.</p>
								{/if}
							</section>
						{/if}

						{#if selected}
							<section class="grid gap-2">
								<h3 class="font-medium">Component</h3>
								<Facts
									facts={[
										{ label: 'Name', value: selected.label },
										{ label: 'Port', value: selected.path, mono: true },
										{ label: 'Type', value: selected.kind || '—' },
										{ label: 'HP', value: selected.health != null ? fmt(selected.health, 0) : 'not damageable' }
									]}
								/>
								<button class="h-9 rounded-md border px-3 hover:bg-accent" onclick={sweep}>Attack directions</button>
								{#if exposure}
									<p class="text-xs text-muted-foreground">
										{within} of {exposure.length} directions within {fmt(reach, 2)} m (green); shallowest:
									</p>
									{#each best as e, i (i)}
										<p class="text-xs tabular-nums">
											{fmt(e.depth, 2)} m through {e.part ?? '—'}, from ({e.direction.map((c) => fmt(-c, 2)).join(', ')})
										</p>
									{/each}
								{/if}
							</section>
						{/if}

						<RenderLog lines={log} {error} />
					</div>
				{/snippet}
			</Surface.Supporting>
		{/snippet}
	</Surface.Split>
</Surface.Root>
