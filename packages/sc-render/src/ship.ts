// A ship on a canvas: its loadout and interior drawn as a hologram
// (see-through hull, what is mounted inside) or with the item materials,
// orbited freely, sliced by a plane on demand. Everything is in the ship
// frame (x right, y forward, z up, metres); the model sits there
// untransformed. A frame is drawn only when something changed.
//
// The holo reads no textures and merges each port's meshes into one draw,
// so a capital ship stays cheap. The materials are built on first use.
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { buildScene } from './cell';
import { HOLO_BACKGROUND, HOLO_EDGE, HOLO_GRID, HOLO_HULL, holoMaterial, setHoloOpacity } from './holo';
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
}

const AXES = { x: new THREE.Vector3(1, 0, 0), y: new THREE.Vector3(0, 1, 0), z: new THREE.Vector3(0, 0, 1) };

/** Above this many hull triangles the holo skips its crease lines. */
const EDGE_LIMIT = 1_500_000;

/**
 * Crease line opacity per pixel per metre at the framed view. The lines add
 * up, so a capital ship (a tenth of a fighter's pixels per metre) goes white
 * at a fighter's opacity. Checked by eye from an Arrow to an Idris at
 * 520 x 346.
 */
const EDGE_OPACITY_PER_PPM = 0.0417;
const EDGE_OPACITY = [0.04, 0.45] as const;

/** A distinct colour per damage part (golden-angle hues). */
function partColour(i: number): THREE.Color {
	return new THREE.Color().setHSL((i * 0.618034) % 1, 0.55, 0.55);
}

/** Lets the page breathe during long synchronous work. */
function breather(): () => Promise<void> {
	let last = performance.now();
	return async () => {
		if (performance.now() - last < 12) return;
		await new Promise((r) => setTimeout(r));
		last = performance.now();
	};
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

/**
 * `meshes` as one geometry in world space: only the vertices they index
 * (every submesh of a file shares the file's whole vertex buffer), positions
 * and normals only.
 */
function merge(meshes: THREE.Mesh[]): THREE.BufferGeometry {
	let largest = 0;
	for (const m of meshes) largest = Math.max(largest, m.geometry.getAttribute('position').count);
	const slot = new Int32Array(largest);
	const seen = new Uint32Array(largest);
	let vertices = 0;
	let indices = 0;
	let stamp = 0;
	for (const m of meshes) {
		const idx = m.geometry.getIndex()!;
		stamp++;
		for (let i = 0; i < idx.count; i++) {
			const v = idx.getX(i);
			if (seen[v] !== stamp) {
				seen[v] = stamp;
				vertices++;
			}
		}
		indices += idx.count;
	}
	const positions = new Float32Array(vertices * 3);
	const normals = new Float32Array(vertices * 3);
	const index = new Uint32Array(indices);
	const p = new THREE.Vector3();
	const n = new THREE.Vector3();
	const normalMatrix = new THREE.Matrix3();
	let written = 0;
	let at = 0;
	let allNormals = true;
	for (const m of meshes) {
		const idx = m.geometry.getIndex()!;
		const pos = m.geometry.getAttribute('position');
		const nor = m.geometry.getAttribute('normal');
		if (!nor) allNormals = false;
		normalMatrix.getNormalMatrix(m.matrixWorld);
		stamp++;
		for (let i = 0; i < idx.count; i++) {
			const v = idx.getX(i);
			if (seen[v] !== stamp) {
				seen[v] = stamp;
				slot[v] = written;
				p.fromBufferAttribute(pos, v).applyMatrix4(m.matrixWorld).toArray(positions, written * 3);
				if (nor) n.fromBufferAttribute(nor, v).applyMatrix3(normalMatrix).normalize().toArray(normals, written * 3);
				written++;
			}
			index[at++] = slot[v];
		}
	}
	const g = new THREE.BufferGeometry();
	g.setAttribute('position', new THREE.BufferAttribute(positions, 3));
	g.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
	g.setIndex(new THREE.BufferAttribute(index, 1));
	if (!allNormals) g.computeVertexNormals();
	g.computeBoundingBox();
	g.computeBoundingSphere();
	return g;
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

/** One port's meshes merged into one draw. */
interface PortDraw {
	path: string;
	mesh: THREE.Mesh;
}

/** The merged holo model. */
interface HoloModel {
	root: THREE.Group;
	/** The hull first, when there is one. */
	ports: PortDraw[];
	/** Hull crease lines, when not skipped. */
	edges: THREE.LineSegments | null;
}

interface HullMesh {
	mesh: THREE.Mesh;
	original: THREE.Material | THREE.Material[];
	part: string | null;
}

/** The materials model. */
interface DetailModel {
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
	private detail: DetailModel | null = null;
	private detailBuilding: Promise<void> | null = null;
	private lit = false;

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
		this.fitEdges();
		this.render();
	}

	/** Draw `ship`, replacing what is shown; returns the render log. */
	async show(ship: ShipScene): Promise<string[]> {
		const log: string[] = [];
		const generation = ++this.generation;
		for (const n of ship.notes) log.push(`note: ${n}`);
		const holo = this.mode === 'holo' ? await this.buildHolo(ship, log) : null;
		const detail = this.mode === 'holo' ? null : await this.buildDetail(ship, log);
		if (generation !== this.generation || this.disposed) {
			if (holo) this.discardHolo(holo);
			if (detail) disposeTree(detail.root);
			log.push('superseded by a newer render');
			return log;
		}
		this.clearModels();
		this.ship = ship;
		this.parts = [...new Set(ship.node_parts.map((n) => n.part))].filter((p) => p);
		this.holo = holo;
		this.detail = detail;
		if (holo) this.scene.add(holo.root);
		if (detail) this.scene.add(detail.root);
		this.measure();
		this.buildGrid();
		this.applyMode();
		this.frame();
		await this.ensureInterior(ship, log, generation);
		if (generation !== this.generation) return log;
		this.applyMode();
		log.push(this.stats());
		return log;
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
			if (generation !== this.generation || this.disposed) return this.discardHolo(holo);
			this.holo = holo;
			this.scene.add(holo.root);
			this.measure();
		} else if (mode !== 'holo' && !this.detail) {
			this.detailBuilding ??= this.buildDetail(ship, log).then((detail) => {
				this.detailBuilding = null;
				if (generation !== this.generation || this.disposed) return disposeTree(detail.root);
				this.detail = detail;
				this.scene.add(detail.root);
			});
			await this.detailBuilding;
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
	 * The port path under canvas pixel (`x`, `y`): the nearest mounted item
	 * the view ray meets, among the coloured ports when any are set. The
	 * see-through hull does not block it. Holo mode; `null` when none.
	 */
	at(x: number, y: number): string | null {
		const ports = this.holo?.ports.filter((p) => p.path && (!this.portColours.size || this.portColours.has(p.path))) ?? [];
		const ray = this.rayAt(x, y);
		const hit = new THREE.Raycaster(ray.origin, ray.direction).intersectObjects(
			ports.map((p) => p.mesh),
			false
		)[0];
		return hit ? (ports.find((p) => p.mesh === hit.object)?.path ?? null) : null;
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
		const m = this.materials.get('holo-hull') as THREE.ShaderMaterial | undefined;
		if (m) setHoloOpacity(m, opacity);
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
		if (!this.disposed) this.renderer.render(this.scene, this.camera);
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

	private async buildHolo(ship: ShipScene, log: string[]): Promise<HoloModel> {
		const source = await buildScene(this.lib, ship.root, log, true);
		const groups = byPort(source);
		const breathe = breather();
		const root = new THREE.Group();
		const hull = groups.get('') ?? [];
		const hullTriangles = hull.reduce((n, m) => n + m.geometry.getIndex()!.count / 3, 0);
		const edges = hullTriangles <= EDGE_LIMIT ? await this.buildEdges(hull, breathe) : null;
		if (edges) root.add(edges);
		else if (hull.length) log.push(`holo: ${(hullTriangles / 1e6).toFixed(2)} M hull triangles, no crease lines`);
		const ports: PortDraw[] = [];
		for (const path of [...groups.keys()].sort()) {
			const mesh = new THREE.Mesh(merge(groups.get(path)!));
			mesh.matrixAutoUpdate = false;
			root.add(mesh);
			ports.push({ path, mesh });
			await breathe();
		}
		log.push(`holo: ${ports.length} draws from ${[...groups.values()].reduce((n, l) => n + l.length, 0)} meshes`);
		return { root, ports, edges };
	}

	/** The hull's crease lines (angle over 30 degrees) as one draw, world space. */
	private async buildEdges(hull: THREE.Mesh[], breathe: () => Promise<void>): Promise<THREE.LineSegments | null> {
		const parts: Float32Array[] = [];
		let length = 0;
		const v = new THREE.Vector3();
		for (const mesh of hull) {
			const edges = new THREE.EdgesGeometry(mesh.geometry, 30);
			const pos = edges.getAttribute('position');
			const out = new Float32Array(pos.count * 3);
			for (let i = 0; i < pos.count; i++) v.fromBufferAttribute(pos, i).applyMatrix4(mesh.matrixWorld).toArray(out, i * 3);
			edges.dispose();
			parts.push(out);
			length += out.length;
			await breathe();
		}
		if (!length) return null;
		const all = new Float32Array(length);
		let at = 0;
		for (const p of parts) {
			all.set(p, at);
			at += p.length;
		}
		const g = new THREE.BufferGeometry();
		g.setAttribute('position', new THREE.BufferAttribute(all, 3));
		const lines = new THREE.LineSegments(
			g,
			new THREE.LineBasicMaterial({
				color: HOLO_EDGE,
				transparent: true,
				opacity: 0.45,
				depthWrite: false,
				blending: THREE.AdditiveBlending,
				clippingPlanes: this.planes,
				toneMapped: false
			})
		);
		lines.matrixAutoUpdate = false;
		return lines;
	}

	private async buildDetail(ship: ShipScene, log: string[]): Promise<DetailModel> {
		const root = await buildScene(this.lib, ship.root, log);
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
			const source = await buildScene(this.lib, part, quiet).catch((e) => {
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

	private discardHolo(holo: HoloModel) {
		disposeTree(holo.root);
	}

	/** Remove and free everything of the last ship. */
	private clearModels() {
		for (const root of [this.holo?.root, this.detail?.root]) {
			if (!root) continue;
			this.scene.remove(root);
		}
		// The merged meshes' materials are shared and live in `materials`.
		if (this.holo) {
			for (const p of this.holo.ports) p.mesh.geometry.dispose();
			if (this.holo.edges) disposeTree(this.holo.edges);
		}
		if (this.detail) {
			// Back on their own materials, so those are freed too.
			for (const h of this.detail.hull) h.mesh.material = h.original;
			disposeTree(this.detail.root);
		}
		this.holo = null;
		this.detail = null;
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
			const hull = this.holo.ports.find((p) => p.path === '');
			for (const p of hull ? [hull] : this.holo.ports) box.union(p.mesh.geometry.boundingBox!);
		} else if (this.detail) {
			const v = new THREE.Box3();
			this.detail.root.updateMatrixWorld(true);
			for (const h of this.detail.hull) box.union(v.copy(h.mesh.geometry.boundingBox!).applyMatrix4(h.mesh.matrixWorld));
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

	/** The crease lines' strength for how many pixels a metre covers when framed. */
	private fitEdges() {
		const edges = this.holo?.edges;
		if (!edges) return;
		// The framed view is about 1.6 radii tall.
		const pixelsPerMetre = this.canvas.clientHeight / (1.6 * this.radius);
		(edges.material as THREE.LineBasicMaterial).opacity = THREE.MathUtils.clamp(pixelsPerMetre * EDGE_OPACITY_PER_PPM, ...EDGE_OPACITY);
	}

	private applyMode() {
		const holo = this.mode === 'holo';
		if (!holo) this.ensureLit();
		this.fitEdges();
		this.updateInterior();
		if (this.holo) this.holo.root.visible = holo;
		if (this.detail) this.detail.root.visible = !holo;
		if (this.grid) this.grid.visible = holo;
		this.scene.background = holo && this.showBackground ? new THREE.Color(HOLO_BACKGROUND) : null;
		const planes = this.planes;
		const interiorHolo = this.material('holo-interior', () => holoMaterial(HOLO_HULL, { opacity: 0.1, base: 0.35, rim: 0.7, planes }));
		for (const b of this.batches) b.mesh.material = holo ? interiorHolo : b.original;
		this.colourPorts();
		for (const h of this.detail?.hull ?? []) {
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
		const hull = this.material('holo-hull', () => holoMaterial(HOLO_HULL, { opacity: this.holoOpacity, planes }));
		const solid = (colour: THREE.ColorRepresentation) =>
			this.material(`holo-solid-${new THREE.Color(colour).getHex()}`, () => holoMaterial(colour, { opacity: 1, base: 0.7, rim: 0.6, planes }));
		for (const { path, mesh } of this.holo.ports) {
			const lit = this.highlights.some((h) => path === h || path.startsWith(`${h}/`));
			const colour = lit ? this.highlightColour : this.portColours.get(path);
			mesh.material = colour !== undefined ? solid(colour) : hull;
		}
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
