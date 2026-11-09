// A ship on a canvas: its loadout and interior drawn as a hologram
// (see-through hull, what is mounted inside) or with the item materials,
// orbited freely, sliced by a plane on demand. Everything is in the ship
// frame (x right, y forward, z up, metres); the model sits there
// untransformed. A frame is drawn only when something changed.
//
// The holo comes from the source in one go, built across every core
// (`Assets::holo`): no textures, one draw per placed part. Parts then change
// detail on their own as the view closes in (see `holo-model.ts`). The
// materials are built on first use.
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { buildScene } from './cell';
import { HOLO_BACKGROUND, HOLO_EDGE, HOLO_GRID, HOLO_HULL, MAX_BLEND, holoMaterial, setHoloOpacity } from './holo';
import { COARSEST_LEVEL, HoloModel, type HoloDetail } from './holo-model';
import { Library, type RenderSource } from './library';
import type { ScenePart, ShipScene } from './types';

/**
 * How the ship is drawn: a hologram, its materials, a colour per damage part
 * (needs the scene's `node_parts`), or see-through.
 */
export type ShipMode = 'holo' | 'materials' | 'parts' | 'xray';

/** The axis a slice cuts across; `off` draws the whole ship. */
export type SliceAxis = 'off' | 'x' | 'y' | 'z';

export interface ShipViewOptions {
	/** The first mode; `holo` by default. */
	mode?: ShipMode;
	/** The holo's own dark background; otherwise the canvas stays transparent. Default on. */
	background?: boolean;
	/** The holo's floor grid. Default on. */
	grid?: boolean;
	/** The holo's level of detail; `auto`, the default, follows the view. */
	detail?: HoloDetail;
}

const AXES = { x: new THREE.Vector3(1, 0, 0), y: new THREE.Vector3(0, 1, 0), z: new THREE.Vector3(0, 0, 1) };

/**
 * Crease line strength from how many pixels a metre covers right now: lines
 * packed tight merge into a mass unless they are dimmer, and get crisp as the
 * camera closes in. Checked by eye from a Gladius to an Idris at card size.
 */
const EDGE_STRENGTH = { base: 0.75, perPixelPerMetre: 0.03, max: 1 };
/**
 * How much line a view may show, as line pixels per pixel of the ship on
 * screen; the prefix covers the ship evenly (`spread_evenly`). Far out a
 * dense ship (a Centaurus) keeps only a share of its lines, and closing in
 * brings the detail back. Picked by eye at card size: from 4 up a Centaurus
 * turns back into a blob.
 */
const EDGE_INK = 1.6;
const HIDDEN_INK = 0.25;
/** What the hull's fill and lines keep while a part is lit. */
const FOCUS_HULL = 0.55;
/** Lines behind the ship's skin, as a share of those in front: depth at a glance. */
const HIDDEN_EDGES = 0.22;
/** A lit part's frame is at least this share of the ship's radius, each way. */
const LIT_FRAME_MIN = 0.035;

/** A distinct colour per damage part (golden-angle hues). */
function partColour(i: number): THREE.Color {
	return new THREE.Color().setHSL((i * 0.618034) % 1, 0.55, 0.55);
}

/** Run `job` over `items`, `limit` at a time. */
async function pool<T>(items: T[], limit: number, job: (item: T) => Promise<void>) {
	let next = 0;
	const worker = async () => {
		while (next < items.length) await job(items[next++]);
	};
	await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
}

/** The meshes under `root` by port path (`''`: the hull), matrices updated. */
function byPort(root: THREE.Object3D): Map<string, THREE.Mesh[]> {
	root.updateMatrixWorld(true);
	const out = new Map<string, THREE.Mesh[]>();
	const visit = (o: THREE.Object3D, path: string) => {
		if (o.userData.port) path = path ? `${path}/${o.userData.port}` : o.userData.port;
		const mesh = o as THREE.Mesh;
		if (mesh.isMesh) {
			const key = path.toLowerCase();
			const list = out.get(key);
			if (list) list.push(mesh);
			else out.set(key, [mesh]);
		}
		for (const c of o.children) visit(c, path);
	};
	visit(root, '');
	return out;
}

/** Free `root`'s geometries and materials on the GPU (textures stay the Library's). */
function disposeTree(root: THREE.Object3D) {
	root.traverse((o) => {
		const m = o as THREE.Mesh;
		if (!m.geometry) return;
		m.geometry.dispose();
		for (const mat of Array.isArray(m.material) ? m.material : [m.material]) mat?.dispose();
		(o as THREE.InstancedMesh).dispose?.();
	});
}

interface HullMesh {
	mesh: THREE.Mesh;
	original: THREE.Material | THREE.Material[];
	part: string | null;
}

/** The materials model. */
interface TexturedModel {
	root: THREE.Group;
	hull: HullMesh[];
}

/**
 * One submesh of an interior file at every place the containers put the
 * file: one instanced draw. `boxes` are the instances' bounds in the ship
 * frame, for dropping what the slice cuts away.
 */
interface InteriorBatch {
	mesh: THREE.InstancedMesh;
	original: THREE.Material | THREE.Material[];
	matrices: THREE.Matrix4[];
	boxes: THREE.Box3[];
}

/** A ship drawn on a canvas. See the module comment. */
export class ShipView {
	readonly renderer: THREE.WebGLRenderer;
	readonly scene = new THREE.Scene();
	readonly camera = new THREE.PerspectiveCamera(40, 1, 0.05, 5000);
	readonly controls: OrbitControls;
	/** What is drawn; `null` before the first `show` finishes. */
	ship: ShipScene | null = null;
	/** Damage parts in `node_parts` order; index = colour. */
	parts: string[] = [];
	/** The hull's box (the whole ship's when it has no hull of its own). */
	readonly bounds = new THREE.Box3();
	readonly centre = new THREE.Vector3();
	radius = 10;

	private readonly lib: Library;
	private readonly showBackground: boolean;
	private readonly showGrid: boolean;
	private mode: ShipMode;
	private generation = 0;
	private disposed = false;

	private holo: HoloModel | null = null;
	private holoDetail: HoloDetail;
	private frameQueued = false;
	private textured: TexturedModel | null = null;
	private texturedBuilding: Promise<void> | null = null;
	private lit = false;
	private warmth: { geometry: THREE.BufferGeometry; lines: THREE.Material } | null = null;
	private litFrame: THREE.LineSegments | null = null;

	private readonly interior = new THREE.Group();
	private batches: InteriorBatch[] = [];
	private interiorBuilt = false;
	private interiorWanted = true;

	private grid: THREE.GridHelper | null = null;
	private holoOpacity = 0.22;
	private sliceOn = false;
	/** The slice: one plane every ship material clips by. Parked far away when off. */
	private readonly slice = new THREE.Plane(new THREE.Vector3(0, 0, -1), 1e9);
	private readonly planes = [this.slice];

	private portColours = new Map<string, THREE.ColorRepresentation>();
	private highlights: string[] = [];
	private highlightColour: THREE.ColorRepresentation = 0xffffff;
	private readonly materials = new Map<string, THREE.Material>();

	constructor(
		readonly canvas: HTMLCanvasElement,
		source: RenderSource,
		options: ShipViewOptions = {}
	) {
		this.lib = Library.of(source);
		this.mode = options.mode ?? 'holo';
		this.showBackground = options.background ?? true;
		this.showGrid = options.grid ?? true;
		this.holoDetail = options.detail ?? 'auto';
		this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
		this.renderer.outputColorSpace = THREE.SRGBColorSpace;
		this.renderer.toneMapping = THREE.NeutralToneMapping;
		this.renderer.localClippingEnabled = true;
		this.camera.up.set(0, 0, 1);
		this.controls = new OrbitControls(this.camera, canvas);
		this.controls.enableDamping = false;
		this.controls.addEventListener('change', () => this.render());
		this.scene.add(this.interior);
	}

	resize(width: number, height: number) {
		if (width < 1 || height < 1 || this.disposed) return;
		this.renderer.setPixelRatio(window.devicePixelRatio);
		this.renderer.setSize(width, height, false);
		this.camera.aspect = width / height;
		this.camera.updateProjectionMatrix();
		this.render();
	}

	/** Draw `ship`, replacing what is shown; returns the render log. */
	async show(ship: ShipScene): Promise<string[]> {
		const log: string[] = [];
		const generation = ++this.generation;
		for (const n of ship.notes) log.push(`note: ${n}`);
		if (this.mode === 'holo') this.warmShaders();
		const holo = this.mode === 'holo' ? await this.buildHolo(ship, log) : null;
		const textured = this.mode === 'holo' ? null : await this.buildTextured(ship, log);
		if (generation !== this.generation || this.disposed) {
			holo?.dispose();
			if (textured) disposeTree(textured.root);
			log.push('superseded by a newer render');
			return log;
		}
		this.clearModels();
		this.ship = ship;
		this.parts = [...new Set(ship.node_parts.map((n) => n.part))].filter((p) => p);
		this.holo = holo;
		this.textured = textured;
		if (holo) this.scene.add(holo.root);
		if (textured) this.scene.add(textured.root);
		this.measure();
		this.buildGrid();
		this.applyMode();
		this.frame();
		this.coolShaders();
		await this.ensureInterior(ship, log, generation);
		if (generation !== this.generation) return log;
		this.applyMode();
		log.push(this.stats());
		return log;
	}

	/**
	 * How much geometry the holo draws: `auto` follows the view part by part,
	 * a number holds every part at that level (0 is the full mesh).
	 */
	setDetail(detail: HoloDetail) {
		this.holoDetail = detail;
		this.holo?.setChoice(detail);
	}

	/** Each drawn port's level right now, by port path (0 is the full mesh). */
	get detail(): Map<string, number> {
		return this.holo?.levels() ?? new Map();
	}

	/** Resolves once the holo has nothing left to fetch, upload or fade in. */
	settled(): Promise<void> {
		return this.holo?.settled() ?? Promise.resolve();
	}

	/** What the last frame cost. */
	stats(): string {
		const now = this.renderer.info.render;
		return `frame: ${now.calls} draws, ${(now.triangles / 1e6).toFixed(2)} M triangles`;
	}

	/**
	 * Switch how the ship is drawn. The first switch to or from the holo
	 * builds that model. Resolves once it is drawn.
	 */
	async setMode(mode: ShipMode): Promise<void> {
		this.mode = mode;
		const ship = this.ship;
		if (!ship) return;
		const generation = this.generation;
		const log: string[] = [];
		if (mode === 'holo' && !this.holo) {
			const holo = await this.buildHolo(ship, log);
			if (generation !== this.generation || this.disposed) return holo.dispose();
			this.holo = holo;
			this.scene.add(holo.root);
			this.measure();
		} else if (mode !== 'holo' && !this.textured) {
			this.texturedBuilding ??= this.buildTextured(ship, log).then((textured) => {
				this.texturedBuilding = null;
				if (generation !== this.generation || this.disposed) return disposeTree(textured.root);
				this.textured = textured;
				this.scene.add(textured.root);
			});
			await this.texturedBuilding;
		}
		if (generation === this.generation && !this.disposed) this.applyMode();
	}

	/**
	 * Light a port and everything mounted below it, by port path from the
	 * hull down (`hardpoint_x/gimbal/gun`, any case); `null` clears. Holo mode.
	 */
	highlight(path: string | string[] | null, colour: THREE.ColorRepresentation = 0xffffff) {
		this.highlights = (path === null ? [] : Array.isArray(path) ? path : [path]).map((p) => p.toLowerCase());
		this.highlightColour = colour;
		this.colourPorts();
		this.frameLit();
		this.render();
	}

	/**
	 * Draw these ports solid in their colours (holo mode), by exact port
	 * path; every other port is drawn like the hull. Replaces the last set.
	 */
	setPortColours(colours: Map<string, THREE.ColorRepresentation>) {
		this.portColours = new Map([...colours].map(([p, c]) => [p.toLowerCase(), c]));
		this.colourPorts();
		this.render();
	}

	/**
	 * Where a port and everything mounted below it sit, in the ship frame, by
	 * port path as for `highlight`; several paths give the box around them
	 * all. `null` when none is drawn. Holo mode.
	 */
	portBounds(path: string | string[]): THREE.Box3 | null {
		const wanted = (Array.isArray(path) ? path : [path]).map((p) => p.toLowerCase());
		const box = new THREE.Box3();
		for (const p of this.holo?.ports ?? []) {
			if (wanted.some((w) => p.path === w || p.path.startsWith(`${w}/`))) box.union(p.bounds);
		}
		return box.isEmpty() ? null : box;
	}

	/**
	 * The port path under canvas pixel (`x`, `y`): the nearest mounted item
	 * the view ray meets, among the coloured ports when any are set. The
	 * see-through hull does not block it. Holo mode; `null` when none.
	 */
	at(x: number, y: number): string | null {
		const ports = this.holo?.ports.filter((p) => p.path && (!this.portColours.size || this.portColours.has(p.path))) ?? [];
		const ray = this.rayAt(x, y);
		const hit = new THREE.Raycaster(ray.origin, ray.direction).intersectObjects(
			ports.map((p) => p.pick),
			false
		)[0];
		return hit ? (ports.find((p) => p.pick === hit.object)?.path ?? null) : null;
	}

	/** The view ray through canvas pixel (`x`, `y`), ship frame. */
	rayAt(x: number, y: number): THREE.Ray {
		const ndc = new THREE.Vector2((x / this.canvas.clientWidth) * 2 - 1, -(y / this.canvas.clientHeight) * 2 + 1);
		const r = new THREE.Raycaster();
		r.setFromCamera(ndc, this.camera);
		return r.ray;
	}

	/** The see-through hull's opacity in holo mode, 0..1. */
	setHoloOpacity(opacity: number) {
		this.holoOpacity = opacity;
		this.colourPorts();
		this.render();
	}

	/** Whether the interior shows where it can be seen (X-ray, or a slice). */
	setInterior(visible: boolean) {
		this.interiorWanted = visible;
		this.updateInterior();
		this.render();
	}

	/**
	 * Cut the ship across `axis` at `at` (0..1 of its extent along it),
	 * keeping the part below the cut (above with `flip`).
	 */
	setSlice(axis: SliceAxis, at: number, flip = false) {
		this.sliceOn = axis !== 'off' && !this.bounds.isEmpty();
		if (axis === 'off' || this.bounds.isEmpty()) {
			this.slice.set(new THREE.Vector3(0, 0, -1), 1e9);
		} else {
			const n = AXES[axis].clone().multiplyScalar(flip ? 1 : -1);
			const lo = this.bounds.min.getComponent('xyz'.indexOf(axis));
			const hi = this.bounds.max.getComponent('xyz'.indexOf(axis));
			const cut = lo + (hi - lo) * at;
			// Keeps points with n · p + c >= 0.
			this.slice.set(n, flip ? -cut : cut);
		}
		this.updateInterior();
		this.cullInterior();
		this.render();
	}

	/** Look at the whole ship from the front right, above. */
	frame() {
		const r = this.radius;
		this.controls.target.copy(this.centre);
		this.camera.position.copy(this.centre).add(new THREE.Vector3(r * 1.3, r * 1.5, r * 0.9));
		this.camera.near = Math.max(0.01, r / 500);
		this.camera.far = r * 50;
		this.camera.updateProjectionMatrix();
		this.controls.update();
		this.render();
	}

	render() {
		if (this.disposed) return;
		const holo = this.holo;
		holo?.beforeFrame(performance.now());
		this.fitEdges();
		this.renderer.render(this.scene, this.camera);
		if (!holo) return;
		holo.afterFrame(this.camera, this.canvas.clientHeight, this.camera.position.distanceTo(this.controls.target));
		if (holo.busy) this.queueFrame();
	}

	/** Another frame soon: the holo has work to bring in. */
	private queueFrame() {
		if (this.frameQueued) return;
		this.frameQueued = true;
		requestAnimationFrame(() => {
			this.frameQueued = false;
			this.render();
		});
	}

	/**
	 * Free the WebGL context and everything on the GPU (a page leaving must
	 * call this; contexts are few). The view is unusable afterwards.
	 * Textures belong to the shared `Library`: its `evict` frees those.
	 */
	dispose() {
		if (this.disposed) return;
		this.disposed = true;
		this.generation++;
		this.clearModels();
		this.grid?.dispose();
		this.scene.environment?.dispose();
		this.controls.dispose();
		this.renderer.dispose();
		this.renderer.forceContextLoss();
	}

	/** Where `ship`'s files are read: the install it was assembled against. */
	private libraryOf(ship: ShipScene): Library {
		const source = ship.install ? this.lib.source.at?.(ship.install) : undefined;
		return source ? Library.of(source) : this.lib;
	}

	private async buildHolo(ship: ShipScene, log: string[]): Promise<HoloModel> {
		const level = this.holoDetail === 'auto' ? COARSEST_LEVEL : this.holoDetail;
		const { source } = this.libraryOf(ship);
		const opened = await source.holo(ship.root, { level });
		log.push(...opened.info.notes);
		const look = { depth: this.depthMaterial(), lines: (depthFunc: THREE.DepthModes) => this.edgeMaterial(depthFunc) };
		const holo = new HoloModel(source, ship.root, opened, look, this.holoDetail);
		holo.onWork = () => this.queueFrame();
		return holo;
	}

	private async buildTextured(ship: ShipScene, log: string[]): Promise<TexturedModel> {
		const root = await buildScene(this.libraryOf(ship), ship.root, log);
		this.clipAll(root);
		const byNode = new Map(ship.node_parts.map((n) => [n.node.toLowerCase(), n.part]));
		const hull: HullMesh[] = [];
		for (const mesh of byPort(root).get('') ?? []) {
			let part: string | null = null;
			for (let p: THREE.Object3D | null = mesh.parent; p && !part; p = p.parent) {
				part = byNode.get(p.name.toLowerCase()) ?? null;
			}
			hull.push({ mesh, original: mesh.material, part });
		}
		log.push(`${hull.length} hull meshes`);
		return { root, hull };
	}

	/**
	 * Each interior geometry file built once (a few at a time), then drawn
	 * instanced: one draw per file submesh for all its placements.
	 */
	private async ensureInterior(ship: ShipScene, log: string[], generation: number) {
		if (this.interiorBuilt || !ship.interior.length) return;
		this.interiorBuilt = true;
		const placements = new Map<string, { geometry: string; at: THREE.Matrix4[] }>();
		for (const b of ship.interior) {
			const key = b.geometry.toLowerCase();
			const t = b.transform;
			const m = new THREE.Matrix4().set(t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], 0, 0, 0, 1);
			const entry = placements.get(key) ?? { geometry: b.geometry, at: [] };
			entry.at.push(m);
			placements.set(key, entry);
		}
		const quiet: string[] = [];
		await pool([...placements.values()], 8, async ({ geometry, at }) => {
			const part: ScenePart = {
				record: geometry.split('/').pop() ?? geometry,
				guid: '',
				pieces: [{ geometry, material: null }],
				skeleton: null,
				attachments: [],
				palette: null
			};
			const source = await buildScene(this.libraryOf(ship), part, quiet).catch((e) => {
				log.push(`interior ${geometry}: ${e}`);
				return null;
			});
			if (!source || generation !== this.generation) return;
			source.updateMatrixWorld(true);
			source.traverse((o) => {
				const sub = o as THREE.Mesh;
				if (!sub.isMesh) return;
				const matrices = at.map((p) => p.clone().multiply(sub.matrixWorld));
				const mesh = new THREE.InstancedMesh(sub.geometry, sub.material, matrices.length);
				matrices.forEach((m, i) => mesh.setMatrixAt(i, m));
				mesh.instanceMatrix.needsUpdate = true;
				mesh.computeBoundingSphere();
				mesh.name = sub.name;
				this.interior.add(mesh);
				this.batches.push({
					mesh,
					original: sub.material,
					matrices,
					boxes: matrices.map((m) => sub.geometry.boundingBox!.clone().applyMatrix4(m))
				});
			});
		});
		if (generation !== this.generation) return;
		this.clipAll(this.interior);
		log.push(`interior: ${placements.size} geometry files, ${this.batches.length} instanced draws`);
	}

	/** Remove and free everything of the last ship. */
	private clearModels() {
		this.clearLitFrame();
		for (const root of [this.holo?.root, this.textured?.root]) {
			if (!root) continue;
			this.scene.remove(root);
		}
		// The holo's materials are shared and live in `materials`.
		this.holo?.dispose();
		if (this.textured) {
			// Back on their own materials, so those are freed too.
			for (const h of this.textured.hull) h.mesh.material = h.original;
			disposeTree(this.textured.root);
		}
		this.holo = null;
		this.textured = null;
		for (const b of this.batches) {
			b.mesh.dispose();
			b.mesh.geometry.dispose();
			for (const mat of Array.isArray(b.original) ? b.original : [b.original]) mat.dispose();
		}
		this.interior.clear();
		this.batches = [];
		this.interiorBuilt = false;
		for (const m of this.materials.values()) m.dispose();
		this.materials.clear();
	}

	/** Bounds, centre and radius from whichever model exists; the hull's if it has one. */
	private measure() {
		const box = new THREE.Box3();
		if (this.holo) {
			const hull = this.holo.ports.filter((p) => p.path === '');
			for (const p of hull.length ? hull : this.holo.ports) box.union(p.bounds);
		} else if (this.textured) {
			const v = new THREE.Box3();
			this.textured.root.updateMatrixWorld(true);
			for (const h of this.textured.hull) box.union(v.copy(h.mesh.geometry.boundingBox!).applyMatrix4(h.mesh.matrixWorld));
		}
		this.bounds.copy(box);
		if (!box.isEmpty()) {
			box.getCenter(this.centre);
			this.radius = box.getSize(new THREE.Vector3()).length() / 2;
		}
	}

	/** The floor grid of the holo view, under the ship. */
	private buildGrid() {
		if (this.grid) {
			this.scene.remove(this.grid);
			this.grid.dispose();
			this.grid = null;
		}
		if (!this.showGrid) return;
		const size = Math.max(10, Math.ceil((this.radius * 2.6) / 10) * 10);
		const grid = new THREE.GridHelper(size, size / 5, HOLO_GRID[0], HOLO_GRID[1]);
		grid.rotation.x = Math.PI / 2;
		grid.position.set(this.centre.x, this.centre.y, this.bounds.isEmpty() ? 0 : this.bounds.min.z - 1);
		const m = grid.material as THREE.Material;
		m.transparent = true;
		m.opacity = 0.7;
		m.depthWrite = false;
		this.grid = grid;
		this.scene.add(grid);
	}

	/** The studio light the materials need; the holo is unlit. */
	private ensureLit() {
		if (this.lit) return;
		this.lit = true;
		const pmrem = new THREE.PMREMGenerator(this.renderer);
		this.scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
		this.scene.environmentIntensity = 0.6;
		pmrem.dispose();
		this.scene.add(new THREE.HemisphereLight(0xdfe8ff, 0x30343a, 1.2));
		const sun = new THREE.DirectionalLight(0xffffff, 2.2);
		sun.position.set(0.4, 0.3, 1);
		this.scene.add(sun);
		const fill = new THREE.DirectionalLight(0xbfd0ff, 0.6);
		fill.position.set(-0.6, -0.5, -0.3);
		this.scene.add(fill);
	}

	private material(key: string, make: () => THREE.Material): THREE.Material {
		let m = this.materials.get(key);
		if (!m) {
			m = make();
			this.materials.set(key, m);
		}
		return m;
	}

	/** The crease lines' strength for how many pixels a metre covers now, dimmed while a part is lit. */
	private fitEdges() {
		const holo = this.holo;
		if (!holo) return;
		const distance = this.camera.position.distanceTo(this.controls.target);
		const metresTall = (2 * distance * Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2))) / this.camera.zoom;
		const pixelsPerMetre = this.canvas.clientHeight / metresTall;
		const strength = Math.min(EDGE_STRENGTH.max, EDGE_STRENGTH.base + pixelsPerMetre * EDGE_STRENGTH.perPixelPerMetre);
		const focus = this.highlights.length ? FOCUS_HULL : 1;
		const size = this.bounds.getSize(new THREE.Vector3());
		// A box's mean projected area is a quarter of its surface.
		const area = Math.max(1e-3, (size.x * size.y + size.y * size.z + size.z * size.x) / 2);
		// Closer than the framed view, the budget opens up fast: a full panel
		// outline beats an even scatter of its pieces, and there is room for it.
		const framed = this.canvas.clientHeight / (1.6 * this.radius);
		const closeness = Math.max(1, pixelsPerMetre / framed) ** 2;
		const budget = (ink: number) => ink * closeness * area * pixelsPerMetre;
		holo.fitLines(strength * focus, strength * focus * HIDDEN_EDGES, budget(EDGE_INK), budget(HIDDEN_INK));
	}

	/** Writes the ship's skin to depth only, nudged back so lines on it stay in front. */
	private depthMaterial(): THREE.Material {
		return this.material('holo-depth', () =>
			new THREE.MeshBasicMaterial({
				colorWrite: false,
				side: THREE.DoubleSide,
				polygonOffset: true,
				polygonOffsetFactor: 1,
				polygonOffsetUnits: 1,
				clippingPlanes: this.planes
			})
		);
	}

	private edgeMaterial(depthFunc: THREE.DepthModes): THREE.LineBasicMaterial {
		return new THREE.LineBasicMaterial({
			...MAX_BLEND,
			color: HOLO_EDGE,
			transparent: true,
			premultipliedAlpha: true,
			depthFunc,
			depthWrite: false,
			clippingPlanes: this.planes,
			toneMapped: false
		});
	}

	private hullMaterial(): THREE.ShaderMaterial {
		return this.material('holo-hull', () =>
			holoMaterial(HOLO_HULL, { opacity: this.holoOpacity, ghost: true, base: 0.5, rim: 1.1, planes: this.planes })
		) as THREE.ShaderMaterial;
	}

	private clearLitFrame() {
		if (!this.litFrame) return;
		this.scene.remove(this.litFrame);
		disposeTree(this.litFrame);
		this.litFrame = null;
	}

	/**
	 * Corner brackets in the highlight colour around what is lit, never smaller
	 * than `LIT_FRAME_MIN`: a power plant is a few pixels at card size.
	 */
	private frameLit() {
		this.clearLitFrame();
		const box = this.highlights.length ? this.portBounds(this.highlights) : null;
		if (!box) return;
		const centre = box.getCenter(new THREE.Vector3());
		const least = this.radius * LIT_FRAME_MIN;
		const half = box.getSize(new THREE.Vector3()).multiplyScalar(0.5);
		half.set(Math.max(half.x, least), Math.max(half.y, least), Math.max(half.z, least));
		const tick = Math.min(half.x, half.y, half.z) * 0.6;
		const points: number[] = [];
		for (const sx of [-1, 1])
			for (const sy of [-1, 1])
				for (const sz of [-1, 1]) {
					const c = [centre.x + sx * half.x, centre.y + sy * half.y, centre.z + sz * half.z];
					points.push(...c, c[0] - sx * tick, c[1], c[2]);
					points.push(...c, c[0], c[1] - sy * tick, c[2]);
					points.push(...c, c[0], c[1], c[2] - sz * tick);
				}
		const g = new THREE.BufferGeometry();
		g.setAttribute('position', new THREE.Float32BufferAttribute(points, 3));
		const frame = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: this.highlightColour, depthTest: false, transparent: true, toneMapped: false }));
		frame.renderOrder = 3;
		this.litFrame = frame;
		this.scene.add(frame);
	}

	private applyMode() {
		const holo = this.mode === 'holo';
		if (!holo) this.ensureLit();
		this.updateInterior();
		if (this.holo) {
			this.holo.root.visible = holo;
			this.holo.setActive(holo);
		}
		if (this.textured) this.textured.root.visible = !holo;
		if (this.grid) this.grid.visible = holo;
		this.scene.background = holo && this.showBackground ? new THREE.Color(HOLO_BACKGROUND) : null;
		const planes = this.planes;
		const interiorHolo = this.material('holo-interior', () => holoMaterial(HOLO_HULL, { opacity: 0.1, ghost: true, base: 0.35, rim: 0.7, planes }));
		for (const b of this.batches) b.mesh.material = holo ? interiorHolo : b.original;
		this.colourPorts();
		for (const h of this.textured?.hull ?? []) {
			const i = h.part ? this.parts.indexOf(h.part) : -1;
			if (this.mode === 'materials' || holo) h.mesh.material = h.original;
			else if (this.mode === 'parts')
				h.mesh.material = this.material(`p${i}`, () =>
					new THREE.MeshStandardMaterial({
						color: i >= 0 ? partColour(i) : 0x6a6f78,
						roughness: 0.8,
						metalness: 0,
						side: THREE.DoubleSide,
						clippingPlanes: planes
					})
				);
			else
				h.mesh.material = this.material(`x${i}`, () =>
					new THREE.MeshBasicMaterial({
						color: i >= 0 ? partColour(i) : 0x8a8f98,
						transparent: true,
						opacity: 0.08,
						depthWrite: false,
						side: THREE.DoubleSide,
						clippingPlanes: planes
					})
				);
		}
		this.render();
	}

	/** The holo's material per port: highlighted, coloured, else the hull's. */
	private colourPorts() {
		if (!this.holo) return;
		const planes = this.planes;
		const hull = this.hullMaterial();
		setHoloOpacity(hull, this.holoOpacity * (this.highlights.length ? FOCUS_HULL : 1));
		const solid = (colour: THREE.ColorRepresentation) =>
			this.material(`holo-solid-${new THREE.Color(colour).getHex()}`, () => holoMaterial(colour, { opacity: 1, base: 0.7, rim: 0.6, overHull: true, planes }));
		const over = (colour: THREE.ColorRepresentation) =>
			this.material(`holo-over-${new THREE.Color(colour).getHex()}`, () =>
				holoMaterial(colour, { opacity: 1, base: 1, rim: 0.8, overHull: true, planes })
			);
		for (const { path, mesh } of this.holo.ports) {
			const lit = this.highlights.some((h) => path === h || path.startsWith(`${h}/`));
			const colour = this.portColours.get(path);
			// Lit and coloured parts come after the hull, which would cover them.
			mesh.material = lit ? over(this.highlightColour) : colour !== undefined ? solid(colour) : hull;
			mesh.renderOrder = lit ? 2 : colour !== undefined ? 1 : 0;
		}
	}

	/**
	 * Compile the hologram's programs while the geometry is still on its way,
	 * instead of after it: draws one empty triangle. The programs stay cached
	 * for the real materials until `coolShaders`.
	 */
	private warmShaders() {
		if (this.warmth) return;
		const g = new THREE.BufferGeometry();
		g.setAttribute('position', new THREE.BufferAttribute(new Int16Array(12), 4, true));
		g.setAttribute('normal', new THREE.BufferAttribute(new Int8Array(12), 4, true));
		g.setIndex([0, 1, 2]);
		const hull = this.hullMaterial();
		const lines = this.edgeMaterial(THREE.LessEqualDepth);
		const scene = new THREE.Scene();
		for (const o of [new THREE.Mesh(g, hull), new THREE.Mesh(g, this.depthMaterial()), new THREE.LineSegments(g, lines)]) {
			o.frustumCulled = false;
			scene.add(o);
		}
		this.renderer.render(scene, this.camera);
		this.warmth = { geometry: g, lines };
	}

	/** Let go of `warmShaders`' stand-ins, once the real materials use the programs. */
	private coolShaders() {
		this.warmth?.geometry.dispose();
		this.warmth?.lines.dispose();
	}

	/** The interior shows when wanted and visible: through an X-ray hull or a slice. */
	private updateInterior() {
		this.interior.visible = this.interiorWanted && (this.mode === 'xray' || this.sliceOn);
	}

	/** Submit only the instances the slice keeps some of. */
	private cullInterior() {
		const tmp = new THREE.Vector3();
		for (const b of this.batches) {
			let n = 0;
			for (let i = 0; i < b.matrices.length; i++) {
				const box = b.boxes[i];
				const gone = this.sliceOn && !this.slice.intersectsBox(box) && this.slice.distanceToPoint(box.getCenter(tmp)) < 0;
				if (!gone) b.mesh.setMatrixAt(n++, b.matrices[i]);
			}
			b.mesh.count = n;
			b.mesh.instanceMatrix.needsUpdate = true;
		}
	}

	/** Every material under `root` clips by the slice plane. */
	private clipAll(root: THREE.Object3D) {
		root.traverse((o) => {
			const mesh = o as THREE.Mesh;
			if (!mesh.isMesh) return;
			for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
				if (m.clippingPlanes !== this.planes) {
					m.clippingPlanes = this.planes;
					m.needsUpdate = true;
				}
			}
		});
	}
}
