// A hologram whose parts change detail as the view closes in. Every part is
// a geometry at a placement: each file is held once per level, and a part
// changes level by swapping the geometry under the same placement, so only
// what is on screen and close up ever costs more than the coarsest level.
//
// A swap never stalls a frame for long: finer geometry is fetched once the
// view settles, uploaded a bounded amount per frame, and each part's outlines
// cross-fade. Max blending keeps the fade clean: both sets at full strength
// are no brighter than one, so lines common to both levels never flicker.
import * as THREE from 'three';
import type { RenderSource } from './library';
import type { HoloGeometry, HoloInfo, HoloRequest, ScenePart } from './types';

/** How a view chooses detail: by what is on screen, or one level for every part (0 is finest). */
export type HoloDetail = 'auto' | 0 | 1 | 2;

/** What a hologram opens at, and what `auto` falls back to. */
export const COARSEST_LEVEL = 2;
/**
 * A part wants finer geometry once its typical triangle edge covers more
 * screen than this, CSS pixels. Picked by eye: a framed Gladius hull sits at
 * 4.5 on level 2 and a zoomed one near 7 on level 1, and both look right.
 */
const REFINE_PIXELS = 6;
/** Far past that it goes straight to the full mesh. */
const LEAP_FACTOR = 2.2;
/** It goes back to a coarser level once that level's edge is this far below the limit: no flicker at the threshold. */
const COARSEN_FACTOR = 0.6;
/** How much longer a level's triangle edges are than the next finer one's, for a level not held yet. */
const COARSER_EDGE = 1.6;
/** How long the view must rest before anything is fetched. */
const SETTLE_MS = 150;
const FADE_MS = 240;
/** Geometry uploaded to the GPU per frame; a single larger one still goes alone. */
const UPLOAD_PER_FRAME = 24e6;
/** Geometries per fetch, the ones that need it most first. */
const FETCH_LIMIT = 32;
/** Finer geometry no part uses is kept this far, for zooming back in. */
const COLD_BYTES = 96e6;
/** Rank classes of a geometry's lines; see `HoloGeometry.line_levels`. */
const LINE_CLASSES = 32;

interface Batch {
	release?: () => void;
	/** Geometries not uploaded yet. */
	waiting: number;
	/** The open's buffer stays: picking reads it. */
	keep: boolean;
}

interface Loaded {
	info: HoloGeometry;
	mesh: THREE.BufferGeometry;
	front: THREE.BufferGeometry | null;
	hidden: THREE.BufferGeometry | null;
	bytes: number;
	users: number;
	uploaded: boolean;
	batch: Batch;
}

/** One geometry at one place. */
export interface PortDraw {
	path: string;
	mesh: THREE.Mesh;
	/** Ship frame. */
	bounds: THREE.Box3;
	/** Ray tests read the open's geometry: finer buffers are let go once uploaded. */
	pick: THREE.Mesh;
}

interface Part extends PortDraw {
	depth: THREE.Mesh;
	at: THREE.Matrix4;
	radius: number;
	current: Loaded;
	base: Loaded;
	front: THREE.LineSegments | null;
	hidden: THREE.LineSegments | null;
	fading: boolean;
}

interface Fade {
	start: number;
	parts: Part[];
	leaving: THREE.LineSegments[];
	arriving: THREE.LineSegments[];
	materials: { leaving: [THREE.LineBasicMaterial, THREE.LineBasicMaterial]; arriving: [THREE.LineBasicMaterial, THREE.LineBasicMaterial] };
	previous: Loaded[];
}

/** What the view shares with its hologram. */
export interface HoloLook {
	/** Writes the skin to depth only. */
	depth: THREE.Material;
	/** Lines in front of the skin (`LessEqualDepth`) or behind it (`GreaterDepth`). */
	lines(depthFunc: THREE.DepthModes): THREE.LineBasicMaterial;
}

/** Build a geometry's three.js objects over `buffer`. */
function load(info: HoloGeometry, buffer: ArrayBuffer, batch: Batch): Loaded {
	const mesh = new THREE.BufferGeometry();
	mesh.setAttribute('position', new THREE.BufferAttribute(new Int16Array(buffer, info.positions_offset, info.vertex_count * 4), 4, true));
	mesh.setAttribute('normal', new THREE.BufferAttribute(new Int8Array(buffer, info.normals_offset, info.vertex_count * 4), 4, true));
	const index = info.wide_indices ? new Uint32Array(buffer, info.indices_offset, info.index_count) : new Uint16Array(buffer, info.indices_offset, info.index_count);
	mesh.setIndex(new THREE.BufferAttribute(index, 1));
	const centre = new THREE.Vector3(...info.centre);
	mesh.boundingBox = new THREE.Box3(new THREE.Vector3(...info.min), new THREE.Vector3(...info.max)).translate(centre.negate());
	mesh.boundingBox.min.divideScalar(info.scale);
	mesh.boundingBox.max.divideScalar(info.scale);
	mesh.boundingSphere = mesh.boundingBox.getBoundingSphere(new THREE.Sphere());
	let front: THREE.BufferGeometry | null = null;
	let hidden: THREE.BufferGeometry | null = null;
	if (info.line_vertex_count) {
		const position = new THREE.BufferAttribute(new Int16Array(buffer, info.lines_offset, info.line_vertex_count * 4), 4, true);
		// Two geometries over one attribute: each shows its own prefix.
		front = new THREE.BufferGeometry();
		hidden = new THREE.BufferGeometry();
		for (const g of [front, hidden]) {
			g.setAttribute('position', position);
			g.boundingBox = mesh.boundingBox;
			g.boundingSphere = mesh.boundingSphere;
		}
	}
	const bytes = info.vertex_count * 12 + info.index_count * (info.wide_indices ? 4 : 2) + info.line_vertex_count * 8;
	return { info, mesh, front, hidden, bytes, users: 0, uploaded: false, batch };
}

const name = (key: string, level: number) => `${key}@${level}`;

/** Sit `o`, whose positions are quantised to -1..1, at `at` in the file frame `info` gives. */
function place(o: THREE.Object3D, at: THREE.Matrix4, info: HoloGeometry) {
	o.matrix.copy(at).multiply(new THREE.Matrix4().makeTranslation(...info.centre)).scale(new THREE.Vector3().setScalar(info.scale));
	o.matrixAutoUpdate = false;
	o.matrixWorldNeedsUpdate = true;
}

export class HoloModel {
	readonly root = new THREE.Group();
	readonly radius: number;
	/** The hull first, when there is one. */
	readonly ports: PortDraw[];

	private readonly parts: Part[] = [];
	private readonly store = new Map<string, Loaded>();
	/** Levels a file does not have, to the level it answered with. */
	private readonly answered = new Map<string, string>();
	private readonly failed = new Set<string>();
	private readonly queued = new Map<Part, Loaded>();
	private readonly fades: Fade[] = [];
	private readonly edges: { front: THREE.LineBasicMaterial; hidden: THREE.LineBasicMaterial };
	private readonly justAdded: Loaded[] = [];
	private readonly opening: Batch;
	private readonly lastView = new THREE.Matrix4();
	private lastHeight = 0;
	private timer: ReturnType<typeof setTimeout> | null = null;
	private fetching = false;
	private disposed = false;
	private active = true;
	private camera: THREE.PerspectiveCamera | null = null;
	private height = 1;
	private viewDistance = 1;
	private waiters: (() => void)[] = [];

	constructor(
		private readonly source: RenderSource,
		private readonly scene: ScenePart,
		opened: { info: HoloInfo; buffer: ArrayBuffer; release?: () => void },
		private readonly look: HoloLook,
		private choice: HoloDetail
	) {
		const { info, buffer, release } = opened;
		this.radius = info.radius;
		this.opening = { release, waiting: info.geometries.length, keep: true };
		this.edges = { front: look.lines(THREE.LessEqualDepth), hidden: look.lines(THREE.GreaterDepth) };
		const held = info.geometries.map((g) => {
			const loaded = load(g, buffer, this.opening);
			this.store.set(name(g.key, g.level), loaded);
			return loaded;
		});
		const pickMaterial = new THREE.MeshBasicMaterial({ side: THREE.DoubleSide });
		for (const p of info.parts) {
			const base = held[p.geometry];
			const t = p.at;
			const at = new THREE.Matrix4().set(t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], 0, 0, 0, 1);
			const mesh = new THREE.Mesh(base.mesh);
			const depth = new THREE.Mesh(base.mesh, look.depth);
			depth.renderOrder = -1;
			const pick = new THREE.Mesh(base.mesh, pickMaterial);
			place(mesh, at, base.info);
			place(depth, at, base.info);
			place(pick, at, base.info);
			pick.matrixWorld.copy(pick.matrix);
			this.root.add(mesh, depth);
			const bounds = new THREE.Box3(new THREE.Vector3(...base.info.min), new THREE.Vector3(...base.info.max)).applyMatrix4(at);
			const radius = bounds.getSize(new THREE.Vector3()).length() / 2;
			const part: Part = { path: p.path, mesh, depth, bounds, pick, at, radius, current: base, base, front: null, hidden: null, fading: false };
			base.users += 2; // current, and the base it always keeps
			[part.front, part.hidden] = this.lines(part, base, this.edges.front, this.edges.hidden);
			this.parts.push(part);
		}
		this.ports = this.parts;
	}

	/** While false (another mode is drawn) nothing is fetched or uploaded. */
	setActive(active: boolean) {
		this.active = active;
		if (active) this.schedule(0);
		this.settle();
	}

	setChoice(choice: HoloDetail) {
		this.choice = choice;
		this.failed.clear();
		this.schedule(0);
	}

	/** Each drawn port's finest level now. */
	levels(): Map<string, number> {
		const out = new Map<string, number>();
		for (const p of this.parts) out.set(p.path, Math.min(out.get(p.path) ?? 9, p.current.info.level));
		return out;
	}

	get busy(): boolean {
		return this.queued.size > 0 || this.fades.length > 0;
	}

	/** Resolves once nothing is left to fetch, upload or fade. */
	settled(): Promise<void> {
		if (this.idle) return Promise.resolve();
		return new Promise((resolve) => this.waiters.push(resolve));
	}

	/** Before a frame: bring in what fits, advance the fades. */
	beforeFrame(now: number) {
		if (this.disposed || !this.active) return;
		this.swapIn(now);
		this.advance(now);
	}

	/** After a frame: what it uploaded can let go of its memory, and the view may have moved. */
	afterFrame(camera: THREE.PerspectiveCamera, height: number, viewDistance: number) {
		for (const g of this.justAdded.splice(0)) this.markUploaded(g);
		this.camera = camera;
		this.height = height;
		this.viewDistance = viewDistance;
		if (height === this.lastHeight && this.lastView.equals(camera.matrixWorld)) return;
		this.lastHeight = height;
		this.lastView.copy(camera.matrixWorld);
		this.schedule(SETTLE_MS);
	}

	/** Line strengths and how much line each may show (metres), this frame. */
	fitLines(front: number, hidden: number, frontBudget: number, hiddenBudget: number) {
		this.edges.front.opacity = front;
		this.edges.hidden.opacity = hidden;
		const totals = new Float64Array(LINE_CLASSES);
		for (const p of this.parts) {
			const levels = p.current.info.line_levels;
			if (!levels.length) continue;
			for (let c = 0; c < LINE_CLASSES; c++) totals[c] += levels[Math.min(c, levels.length - 1)].length;
		}
		const classFor = (budget: number) => {
			let found = 0;
			for (let c = 0; c < LINE_CLASSES; c++) if (totals[c] <= budget) found = c;
			return found;
		};
		const frontClass = classFor(frontBudget);
		const hiddenClass = classFor(hiddenBudget);
		for (const g of this.store.values()) {
			if (!g.front || !g.hidden || !g.users) continue;
			const levels = g.info.line_levels;
			const count = (c: number) => levels[Math.min(c, levels.length - 1)]?.vertex_count ?? 0;
			g.front.setDrawRange(0, count(frontClass));
			g.hidden.setDrawRange(0, count(hiddenClass));
		}
	}

	dispose() {
		if (this.disposed) return;
		this.disposed = true;
		if (this.timer !== null) clearTimeout(this.timer);
		for (const f of this.fades) this.disposeFade(f);
		for (const g of this.store.values()) this.free(g);
		this.opening.release?.();
		this.edges.front.dispose();
		this.edges.hidden.dispose();
		this.flush();
	}

	private get idle(): boolean {
		return this.disposed || !this.active || (!this.fetching && this.timer === null && !this.busy);
	}

	private settle() {
		if (this.idle) this.flush();
	}

	private flush() {
		for (const w of this.waiters.splice(0)) w();
	}

	private schedule(delay: number) {
		if (this.disposed || !this.active) return;
		if (this.timer !== null) clearTimeout(this.timer);
		this.timer = setTimeout(() => {
			this.timer = null;
			this.plan();
			this.settle();
		}, delay);
	}

	private lookup(key: string, level: number): Loaded | undefined {
		const wanted = name(key, level);
		return this.store.get(this.answered.get(wanted) ?? wanted);
	}

	private pixelsPerMetre(part: Part, camera: THREE.PerspectiveCamera): number {
		// Inside a hull's box the distance to the box is 0: what it shows is as
		// far away as the view is, or as big as the part, whichever is less.
		const distance = Math.max(part.bounds.distanceToPoint(camera.position), Math.min(this.viewDistance, part.radius), 0.5);
		const tall = (2 * distance * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2))) / camera.zoom;
		return this.height / tall;
	}

	/** The level `part` should be at now. */
	private target(part: Part, camera: THREE.PerspectiveCamera, frustum: THREE.Frustum): number {
		if (this.choice !== 'auto') return this.choice;
		const level = part.current.info.level;
		if (!frustum.intersectsBox(part.bounds)) return Math.max(level, part.base.info.level);
		const scale = this.pixelsPerMetre(part, camera);
		const edge = part.current.info.mean_edge * scale;
		if (level > 0 && edge > REFINE_PIXELS) return edge > REFINE_PIXELS * LEAP_FACTOR ? 0 : level - 1;
		if (level < part.base.info.level) {
			const limit = REFINE_PIXELS * COARSEN_FACTOR;
			if (part.base.info.mean_edge * scale < limit) return part.base.info.level;
			const coarser = this.lookup(part.base.info.key, level + 1);
			const there = coarser?.info.mean_edge ?? part.current.info.mean_edge * COARSER_EDGE;
			if (there * scale < limit) return level + 1;
		}
		return level;
	}

	/** What each part should be at: swap what is held, fetch the rest. */
	private plan() {
		const camera = this.camera;
		if (this.disposed || this.fetching || !this.active || !camera) return;
		camera.updateMatrixWorld();
		const frustum = new THREE.Frustum().setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
		const wanted = new Map<number, Map<string, number>>();
		for (const part of this.parts) {
			const level = this.target(part, camera, frustum);
			const key = part.base.info.key;
			if (level === part.current.info.level || this.failed.has(name(key, level))) {
				this.unqueue(part);
				continue;
			}
			const held = this.lookup(key, level);
			if (held) {
				if (held === part.current) this.unqueue(part);
				else this.queue(part, held);
				continue;
			}
			const need = part.current.info.mean_edge * this.pixelsPerMetre(part, camera);
			const list = wanted.get(level) ?? new Map<string, number>();
			list.set(key, Math.max(list.get(key) ?? 0, need));
			wanted.set(level, list);
		}
		const next = [...wanted.entries()].sort((a, b) => b[1].size - a[1].size)[0];
		if (next) void this.fetch(next[0], [...next[1].entries()].sort((a, b) => b[1] - a[1]).slice(0, FETCH_LIMIT).map(([key]) => key));
		if (this.busy) this.onWork?.();
	}

	/** Set by the view: something is ready to draw. */
	onWork?: () => void;

	private async fetch(level: number, keys: string[]) {
		this.fetching = true;
		const request: Partial<HoloRequest> = { level, geometries: keys, radius: this.radius };
		try {
			const { info, buffer, release } = await this.source.holo(this.scene, request);
			if (this.disposed) {
				release?.();
				return;
			}
			const batch: Batch = { release, waiting: info.geometries.length, keep: false };
			const got = new Set<string>();
			for (const g of info.geometries) {
				got.add(g.key);
				const loaded = load(g, buffer, batch);
				this.store.set(name(g.key, g.level), loaded);
				if (g.level !== level) this.answered.set(name(g.key, level), name(g.key, g.level));
			}
			for (const key of keys) if (!got.has(key)) this.failed.add(name(key, level));
			if (!batch.waiting) release?.();
		} catch (e) {
			console.warn('holo level', level, e);
			for (const key of keys) this.failed.add(name(key, level));
		} finally {
			this.fetching = false;
			if (!this.disposed) this.plan();
			this.trim();
			this.settle();
		}
	}

	private queue(part: Part, next: Loaded) {
		if (this.queued.get(part) === next) return;
		this.unqueue(part);
		next.users++;
		this.queued.set(part, next);
	}

	private unqueue(part: Part) {
		const held = this.queued.get(part);
		if (!held) return;
		held.users--;
		this.queued.delete(part);
	}

	/** Lines for `part` at `g`, added to the scene. */
	private lines(part: Part, g: Loaded, front: THREE.Material, hidden: THREE.Material): [THREE.LineSegments | null, THREE.LineSegments | null] {
		if (!g.front || !g.hidden) return [null, null];
		const out: [THREE.LineSegments, THREE.LineSegments] = [new THREE.LineSegments(g.front, front), new THREE.LineSegments(g.hidden, hidden)];
		for (const l of out) {
			l.renderOrder = 1;
			place(l, part.at, g.info);
			this.root.add(l);
		}
		return out;
	}

	/** Swap in as many parts as this frame's upload allows, nearest the end of the queue last. */
	private swapIn(now: number) {
		if (!this.queued.size) return;
		let budget = UPLOAD_PER_FRAME;
		const fade: Fade = {
			start: now,
			parts: [],
			leaving: [],
			arriving: [],
			materials: {
				leaving: [this.look.lines(THREE.LessEqualDepth), this.look.lines(THREE.GreaterDepth)],
				arriving: [this.look.lines(THREE.LessEqualDepth), this.look.lines(THREE.GreaterDepth)]
			},
			previous: []
		};
		for (const m of fade.materials.arriving) m.opacity = 0;
		for (const [part, next] of [...this.queued]) {
			if (part.fading) continue;
			const cost = next.uploaded || this.justAdded.includes(next) ? 0 : next.bytes;
			if (cost > budget && fade.parts.length) break;
			budget -= cost;
			this.queued.delete(part);
			const previous = part.current;
			part.current = next;
			part.mesh.geometry = next.mesh;
			part.depth.geometry = next.mesh;
			place(part.mesh, part.at, next.info);
			place(part.depth, part.at, next.info);
			if (!next.uploaded && !this.justAdded.includes(next)) this.justAdded.push(next);
			for (const old of [part.front, part.hidden]) {
				if (!old) continue;
				old.material = old === part.front ? fade.materials.leaving[0] : fade.materials.leaving[1];
				fade.leaving.push(old);
			}
			[part.front, part.hidden] = this.lines(part, next, fade.materials.arriving[0], fade.materials.arriving[1]);
			for (const line of [part.front, part.hidden]) if (line) fade.arriving.push(line);
			part.fading = true;
			fade.parts.push(part);
			fade.previous.push(previous);
		}
		if (fade.parts.length) this.fades.push(fade);
		else for (const m of [...fade.materials.leaving, ...fade.materials.arriving]) m.dispose();
	}

	/** Lines leaving hold full strength for the first half, lines arriving reach it by the middle. */
	private advance(now: number) {
		for (const fade of [...this.fades]) {
			const t = Math.min(1, (now - fade.start) / FADE_MS);
			const leaving = Math.min(1, 2 * (1 - t));
			const arriving = Math.min(1, 2 * t);
			const { front, hidden } = this.edges;
			fade.materials.leaving[0].opacity = front.opacity * leaving;
			fade.materials.leaving[1].opacity = hidden.opacity * leaving;
			fade.materials.arriving[0].opacity = front.opacity * arriving;
			fade.materials.arriving[1].opacity = hidden.opacity * arriving;
			if (t < 1) continue;
			this.fades.splice(this.fades.indexOf(fade), 1);
			for (const l of fade.leaving) this.root.remove(l);
			for (const part of fade.parts) {
				part.fading = false;
				if (part.front) part.front.material = front;
				if (part.hidden) part.hidden.material = hidden;
			}
			for (const g of fade.previous) g.users--;
			this.disposeFade(fade);
			this.trim();
			this.settle();
		}
	}

	private disposeFade(fade: Fade) {
		for (const m of [...fade.materials.leaving, ...fade.materials.arriving]) m.dispose();
	}

	private markUploaded(g: Loaded) {
		if (g.uploaded) return;
		g.uploaded = true;
		this.onUploaded(g);
	}

	private onUploaded(g: Loaded) {
		g.batch.waiting--;
		if (!g.batch.waiting && !g.batch.keep) g.batch.release?.();
	}

	private free(g: Loaded) {
		g.mesh.dispose();
		g.front?.dispose();
		g.hidden?.dispose();
	}

	/** Let go of the oldest finer geometry nothing uses, past `COLD_BYTES`. */
	private trim() {
		let cold = 0;
		for (const g of this.store.values()) if (!g.users) cold += g.bytes;
		for (const [key, g] of this.store) {
			if (cold <= COLD_BYTES) break;
			if (g.users) continue;
			this.store.delete(key);
			if (!g.uploaded) this.onUploaded(g);
			this.free(g);
			cold -= g.bytes;
		}
	}
}
