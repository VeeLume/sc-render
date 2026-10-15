// One inventory cell, set up the way the game's item window is
// (ui/buildingblocks/inventory/components/item/inventory_panel_item_itemwindow):
// a Y-up UI scene, camera fieldOfView 10 looking along +Z with screen-up -Y,
// the item rotated by defaultOrientation (90,0,0) + its category offset and
// fitted to 70% of the cell's width, height and depth, lit by its category's
// light group (rotated (90,180,0), projectors along local +X).
import * as THREE from 'three';
import {
	commands,
	type BoneInfo,
	type ModelInfo,
	type SceneItem,
	type ScenePaletteEntry,
	type ScenePart,
	type SubMaterialInfo, type PartLook } from '$lib/bindings';
import { fetchModelBuffer } from './protocol';
import { buildMaterial, loadMaterial } from './materials';
/** A tint palette: entries A, B, C. */
export type Palette = ScenePaletteEntry[];

const DEG = Math.PI / 180;
const FOV = 10;
/**
 * The item's share of the cell. The widget data says 70% (ItemModel sizing);
 * measured against in-game cells the PIPS can (upright, shallow, so only this
 * factor matters) fills ~66%, and with 0.66 the JS-400 (depth-limited) lands
 * within ~7%. The extra margin may be the item window's own inset; the depth
 * reference (taken = cell height) is not stated in the data.
 */
const FIT = 0.66;
/** Cryengine Ang3 → matrix Rz·Ry·Rx, which three calls order 'ZYX'. */
const ang3 = (d: [number, number, number]) => new THREE.Euler(d[0] * DEG, d[1] * DEG, d[2] * DEG, 'ZYX');

export type ToneMap = 'agx' | 'neutral' | 'aces' | 'linear';

/**
 * Calibrated against in-game cells (JS-400, PIPS can): linear tone mapping
 * (the game does not compress highlights like AgX), rig intensity x 2 (2.5
 * while metals were mis-shaded; the Invictus jacket's colour matches at 2),
 * ambient 0.15 (the shadow sides stay dark in game; brightness comes from the
 * key light, not ambient).
 */
export interface CellOptions {
	lightScale: number;
	ambient: number;
	toneMap?: ToneMap;
	/** Extra factor on the rig's rim light (tuning aid). */
	rimScale?: number;
	/**
	 * Strength of a studio environment for reflections: a dark room with one
	 * softbox above and toward the camera. The light groups name `cm_inventory_probe_cm` but
	 * switch it off (and it decodes mostly black with magenta blobs), yet in
	 * game glossy tops shine white (P6-LR) and dark metal shows a sheen on top
	 * (power plants); this stands in for that. Reflection only (no diffuse
	 * irradiance). 2 (Valerie's pick) lifts glossy tops like the P6-LR's
	 * while the JS-400, P4-AR and PIPS keep their colours; the game lights
	 * items brighter still from the front and above, but recognisability
	 * matters more than a 1:1 match. 0 = off.
	 */
	env?: number;
}

const TONE_MAPS: Record<ToneMap, THREE.ToneMapping> = {
	agx: THREE.AgXToneMapping,
	neutral: THREE.NeutralToneMapping,
	aces: THREE.ACESFilmicToneMapping,
	linear: THREE.LinearToneMapping
};

/** A saved character's colour overrides by item GUID (see `characterLook`). */
export type Looks = Map<string, PartLook>;

export class Cell {
	readonly renderer: THREE.WebGLRenderer;
	readonly scene = new THREE.Scene();
	readonly camera: THREE.PerspectiveCamera;
	private item: THREE.Object3D | null = null;
	/** Turntable around the item's bounds centre; identity is the game's pose. */
	private turn: THREE.Group | null = null;
	/** The last show's fit (scale and bounds centre), for `keepView`. */
	private frame: { scale: number; centre: THREE.Vector3 } | null = null;
	/** Bumped per `show`; a render that finishes after a newer one started is dropped. */
	private generation = 0;
	private lights: THREE.Object3D[] = [];
	private ambient = new THREE.AmbientLight(0x9aa6b8, 0.05);
	private envMap: THREE.Texture | null = null;
	private shaderErrors: string[] = [];

	/** Cell width / height; set per item in `show`. */
	aspect = 1;

	constructor(readonly canvas: HTMLCanvasElement) {
		this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
		this.renderer.outputColorSpace = THREE.SRGBColorSpace;
		this.renderer.toneMapping = THREE.LinearToneMapping;
		this.renderer.shadowMap.enabled = true;
		this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
		// A patched material that fails to compile just disappears; its errors
		// go into the render log (once per program, on first use).
		this.renderer.debug.onShaderError = (gl, program, vs, fs) => {
			const link = gl.getProgramInfoLog(program)?.trim();
			if (link) this.shaderErrors.push(`shader error (link): ${link}`);
			for (const [stage, sh] of [['vertex', vs], ['fragment', fs]] as const) {
				const info = gl.getShaderInfoLog(sh);
				if (info?.trim()) this.shaderErrors.push(`shader error (${stage}): ${info.trim()}`);
			}
		};
		this.camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 100);
		// A cell is 1 unit high; back off until it fills the vertical FOV.
		this.camera.position.set(0, 0, -0.5 / Math.tan((FOV / 2) * DEG));
		this.camera.up.set(0, -1, 0);
		this.camera.lookAt(0, 0, 0);
		this.scene.add(this.ambient);
		const pmrem = new THREE.PMREMGenerator(this.renderer);
		this.envMap = pmrem.fromEquirectangular(studioEnvironment()).texture;
		pmrem.dispose();
	}

	resize(width: number, height: number) {
		this.renderer.setPixelRatio(window.devicePixelRatio);
		this.renderer.setSize(width, height, false);
		this.camera.aspect = width / height;
		this.camera.updateProjectionMatrix();
	}

	/**
	 * Draw `fx`. With `keepView`, the previous item's fit and rotation stay
	 * (the same character with parts hidden or shown does not jump).
	 */
	async show(fx: SceneItem, opts: CellOptions, keepView = false, looks: Looks = new Map()): Promise<string[]> {
		const log: string[] = [];
		const generation = ++this.generation;

		this.aspect = fx.aspect;
		for (const n of fx.notes) log.push(`note: ${n}`);
		const model = await buildItem(fx.root, log, 0, new CharacterBones(fx.root), looks);

		// Pose: defaultOrientation (90,0,0) + category offset, about the bounds centre.
		const rot = new THREE.Group();
		rot.rotation.copy(ang3([90 + fx.offset[0], fx.offset[1], fx.offset[2]]));
		rot.add(model);
		// Viewing elevation: turns the posed item about the screen's horizontal
		// axis, i.e. the camera looks from above without the item leaning.
		const view = new THREE.Group();
		view.rotation.x = fx.elevation * DEG;
		view.add(rot);
		const pivot = new THREE.Group();
		pivot.add(view);
		pivot.updateMatrixWorld(true);
		const box = visibleBounds(pivot);
		const size = box.getSize(new THREE.Vector3());
		const centre = box.getCenter(new THREE.Vector3());
		// ItemModel sizing: a share of the cell's width, height and depth (depth = height).
		let s = FIT * Math.min(this.aspect / size.x, 1 / size.y, 1 / size.z);
		if (keepView && this.frame) {
			s = this.frame.scale;
			centre.copy(this.frame.centre);
		} else {
			this.frame = { scale: s, centre: centre.clone() };
		}
		rot.rotation.set(0, 0, 0);
		pivot.updateMatrixWorld(true);
		const local = visibleBounds(pivot).getSize(new THREE.Vector3());
		rot.rotation.copy(ang3([90 + fx.offset[0], fx.offset[1], fx.offset[2]]));
		const f = (v: THREE.Vector3) => `${v.x.toFixed(3)} ${v.y.toFixed(3)} ${v.z.toFixed(3)}`;
		log.push(`extents rotated ${f(size)} | unrotated ${f(local)} | scale ${s.toFixed(4)}`);
		pivot.scale.setScalar(s);
		pivot.position.copy(centre).multiplyScalar(-s);
		pivot.traverse((o) => {
			const mesh = o as THREE.Mesh;
			if (!mesh.isMesh) return;
			// Glass and other see-through surfaces cast no shadow.
			mesh.castShadow = !(mesh.material as THREE.Material).userData.noShadow;
			mesh.receiveShadow = true;
		});
		if (generation !== this.generation) {
			log.push('superseded by a newer render');
			return log;
		}
		if (this.item) this.scene.remove(this.item);
		// The pivot puts the bounds centre at the origin, so the turntable
		// turns the item in place.
		const turn = new THREE.Group();
		if (keepView && this.turn) turn.quaternion.copy(this.turn.quaternion);
		if (!keepView) {
			this.zoom = 1;
			this.pan.set(0, 0);
			this.camera.zoom = 1;
			this.camera.updateProjectionMatrix();
			this.camera.position.set(0, 0, -0.5 / Math.tan((FOV / 2) * DEG));
		}
		turn.add(pivot);
		this.turn = turn;
		this.item = turn;
		this.scene.add(turn);

		this.setLights(fx, opts);
		this.render();
		log.push(...this.shaderErrors.splice(0));
		return log;
	}

	setLights(fx: SceneItem, opts: CellOptions) {
		for (const l of this.lights) this.scene.remove(l);
		this.lights = [];
		const group = new THREE.Group();
		group.rotation.copy(ang3([90, 180, 0]));
		// One shadow map at most, from the strongest caster: each costs a
		// sampler, and LayerBlend needs 13 of the 16 (see materials.ts).
		const caster = fx.rig.filter((l) => l.casts_shadows).sort((a, b) => b.intensity - a.intensity)[0];
		for (const l of fx.rig) {
			const spot = new THREE.SpotLight(
				new THREE.Color(...l.color),
				l.intensity * opts.lightScale * (l.name === 'rim' ? (opts.rimScale ?? 1) : 1),
				l.radius,
				(l.fov / 2) * DEG,
				0.75,
				2
			);
			const holder = new THREE.Object3D();
			holder.position.set(...l.pos);
			holder.rotation.copy(ang3(l.rot));
			holder.add(spot);
			spot.position.set(0, 0, 0);
			spot.target.position.set(1, 0, 0);
			holder.add(spot.target);
			spot.castShadow = l === caster;
			spot.shadow.mapSize.set(2048, 2048);
			spot.shadow.bias = -0.0005;
			group.add(holder);
		}
		this.scene.add(group);
		this.lights.push(group);
		this.ambient.intensity = opts.ambient;
		this.scene.environment = opts.env ? this.envMap : null;
		// The cell's up is -Y (camera.up); turn the studio so its ceiling is above the item.
		this.scene.environmentRotation.set(Math.PI, 0, 0);
		this.scene.environmentIntensity = opts.env ?? 0;
		this.renderer.toneMapping = TONE_MAPS[opts.toneMap ?? 'linear'];
	}

	/**
	 * Turn the item by a pointer drag of `dx`, `dy` pixels: right turns its
	 * front to the right (about the screen's vertical), down tips it towards
	 * the bottom. The light rig stays put.
	 */
	rotate(dx: number, dy: number) {
		if (!this.turn) return;
		const perPixel = Math.PI / this.canvas.clientHeight;
		this.camera.updateMatrixWorld();
		const right = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 0);
		const up = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 1);
		const q = new THREE.Quaternion()
			.setFromAxisAngle(up, dx * perPixel)
			.multiply(new THREE.Quaternion().setFromAxisAngle(right, dy * perPixel));
		this.turn.quaternion.premultiply(q);
		this.render();
	}

	/** Back to the game's pose. */
	resetRotation() {
		if (!this.turn) return;
		this.turn.quaternion.identity();
		this.render();
	}

	/** Magnification (1 = the item fits the cell) and the view centre's offset
	 * in the item plane, in cell units (the cell is 1 high). */
	private zoom = 1;
	private pan = new THREE.Vector2();

	/**
	 * Zoom by `factor` keeping the point under the canvas pixel (`x`, `y`)
	 * where it is, so the wheel zooms towards the cursor.
	 */
	zoomAt(factor: number, x: number, y: number) {
		const next = THREE.MathUtils.clamp(this.zoom * factor, 1, 16);
		const at = this.planeOffset(x, y);
		// The plane point under the cursor before and after must agree.
		this.pan.addScaledVector(at, 1 / this.zoom - 1 / next);
		this.zoom = next;
		if (next === 1) this.pan.set(0, 0);
		this.applyView();
	}

	/** Move the view by a pointer drag of `dx`, `dy` pixels (the item follows). */
	panBy(dx: number, dy: number) {
		const perPixel = 1 / (this.canvas.clientHeight * this.zoom);
		this.pan.x -= dx * perPixel;
		this.pan.y += dy * perPixel;
		this.applyView();
	}

	/** Back to the whole item: rotation, zoom and pan. */
	resetView() {
		this.zoom = 1;
		this.pan.set(0, 0);
		this.applyView();
		this.resetRotation();
	}

	/** A canvas pixel's offset from the centre, in cell units at zoom 1. */
	private planeOffset(x: number, y: number): THREE.Vector2 {
		const h = this.canvas.clientHeight;
		return new THREE.Vector2((x - this.canvas.clientWidth / 2) / h, -(y - h / 2) / h);
	}

	private applyView() {
		this.camera.zoom = this.zoom;
		this.camera.updateProjectionMatrix();
		const right = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 0);
		const up = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 1);
		this.camera.position
			.set(0, 0, -0.5 / Math.tan((FOV / 2) * DEG))
			.addScaledVector(right, this.pan.x)
			.addScaledVector(up, this.pan.y);
		this.camera.updateMatrixWorld();
		this.render();
	}

	/** Free the WebGL context (a page leaving must call this; contexts are few). */
	dispose() {
		this.renderer.dispose();
		this.renderer.forceContextLoss();
	}

	render() {
		this.renderer.render(this.scene, this.camera);
	}
}

/**
 * Equirectangular studio for reflections: a dark room, a little brighter
 * above, with one large softbox up and toward the camera. Glossy surfaces
 * show a defined highlight (the white blocks on the game's P6-LR icon) and
 * keep their colour elsewhere. The room stays dark: the game's inventory has
 * no reflection probe, and a grey room laid a grey film over every surface
 * (black parts of the Black Cherry multitool never got below 31/255); an
 * all-bright sky washed mirror-glossy paint out to white. +Y is up and +Z
 * points at the camera (the cell turns it).
 */
function studioEnvironment(): THREE.DataTexture {
	const w = 128;
	const h = 64;
	const data = new Float32Array(w * h * 4);
	const box = new THREE.Vector3(0, Math.sin(40 * DEG), Math.cos(40 * DEG));
	const dir = new THREE.Vector3();
	for (let y = 0; y < h; y++) {
		// A DataTexture's row 0 is v = 0, which three's equirect lookup puts at -Y.
		const elev = (((y + 0.5) / h) * 180 - 90) * DEG;
		for (let x = 0; x < w; x++) {
			const az = ((x + 0.5) / w - 0.5) * 2 * Math.PI;
			dir.set(Math.cos(elev) * Math.cos(az), Math.sin(elev), Math.cos(elev) * Math.sin(az));
			const room = 0.02 + 0.08 * THREE.MathUtils.smoothstep(elev / DEG, -30, 60);
			const angle = dir.angleTo(box) / DEG;
			const softbox = 2 * (1 - THREE.MathUtils.smoothstep(angle, 22, 32));
			const v = room + softbox;
			data.set([v, v, v, 1], (y * w + x) * 4);
		}
	}
	const tex = new THREE.DataTexture(data, w, h, THREE.RGBAFormat, THREE.FloatType);
	tex.mapping = THREE.EquirectangularReflectionMapping;
	tex.needsUpdate = true;
	return tex;
}

/**
 * Bounds of the triangles that draw, in world space. Every submesh shares the
 * file's whole vertex buffer, so three's own bounding boxes would include
 * other nodes' and proxies' vertices; walk the indices instead.
 */
function visibleBounds(root: THREE.Object3D): THREE.Box3 {
	const box = new THREE.Box3();
	const v = new THREE.Vector3();
	root.updateMatrixWorld(true);
	root.traverse((o) => {
		const mesh = o as THREE.Mesh;
		if (!mesh.isMesh || !mesh.visible) return;
		const pos = mesh.geometry.getAttribute('position');
		const idx = mesh.geometry.getIndex();
		if (!idx) return;
		for (let i = 0; i < idx.count; i++) {
			box.expandByPoint(v.fromBufferAttribute(pos, idx.getX(i)).applyMatrix4(mesh.matrixWorld));
		}
	});
	return box;
}

/**
 * Bones that worn skins add to a character's skeleton. A skin may carry
 * joints the base `.chr` lacks, which the engine merges in: the Novikov
 * undersuit's skin brings `magazine_attach_1_override` and friends that its
 * armor core's ports name. Searched only when a part's own skeleton lacks a
 * bone; skins load on first need.
 */
class CharacterBones {
	private readonly skins: string[] = [];
	private readonly loaded = new Map<string, Promise<BoneInfo[]>>();

	constructor(root: ScenePart) {
		const walk = (p: ScenePart) => {
			for (const piece of p.pieces) if (piece.geometry.toLowerCase().endsWith('.skin')) this.skins.push(piece.geometry);
			for (const a of p.attachments) walk(a.part);
		};
		walk(root);
	}

	async find(name: string): Promise<BoneInfo | undefined> {
		for (const skin of this.skins) {
			let bones = this.loaded.get(skin);
			if (!bones) {
				bones = commands.loadSkeleton(skin).then((r) => (r.status === 'ok' ? r.data : []));
				this.loaded.set(skin, bones);
			}
			const hit = (await bones).find((b) => b.name.toLowerCase() === name);
			if (hit) return hit;
		}
		return undefined;
	}
}

/**
 * An item and, recursively, what is attached to it. Each attachment hangs on
 * a bone of the item's skeleton (or one a worn skin adds to it) or, failing
 * that, a node of its geometry (a .cga helper); it is drawn with its own
 * palette, not its parent's.
 */
async function buildItem(
	item: ScenePart,
	log: string[],
	depth: number,
	character: CharacterBones,
	looks: Looks
): Promise<THREE.Group> {
	const pad = '  '.repeat(depth);
	log.push(`${pad}${item.record}`);
	const group = new THREE.Group();
	for (const piece of item.pieces) {
		const res = await commands.loadModel(piece.geometry);
		if (res.status !== 'ok') throw new Error(res.error);
		const info = res.data;
		const mtlPath = piece.material ?? info.material;
		const mtl = mtlPath ? await loadMaterial(mtlPath) : null;
		log.push(`${pad}  ${piece.geometry.split('/').pop()}: ${info.vertex_count} vertices, ${info.index_count / 3} triangles`);
		log.push(`${pad}    material ${mtlPath ?? '—'}: ${mtl?.submaterials.length ?? 0} submaterials`);
		const look = looks.get(item.guid);
		group.add(await buildModel(info, mtl?.submaterials ?? [], depth ? [] : log, item.palette ?? undefined, look));
	}
	if (!item.attachments.length) return group;
	let bones: BoneInfo[] = [];
	if (item.skeleton) {
		const res = await commands.loadSkeleton(item.skeleton);
		if (res.status !== 'ok') throw new Error(res.error);
		bones = res.data;
	}
	for (const at of item.attachments) {
		const name = at.bone.toLowerCase();
		let bone = name ? bones.find((b) => b.name.toLowerCase() === name) : undefined;
		// Only a part on a character skeleton can use a worn skin's bones.
		if (name && !bone && item.skeleton) bone = await character.find(name);
		let anchor: THREE.Object3D | undefined;
		if (!name) {
			// Skinned to this part's skeleton: bind pose, at its origin.
			anchor = group;
		} else if (bone) {
			anchor = new THREE.Object3D();
			anchor.position.set(...bone.position);
			anchor.quaternion.set(...bone.rotation);
			group.add(anchor);
		} else {
			group.traverse((o) => {
				if (!anchor && !(o as THREE.Mesh).isMesh && o.name.toLowerCase() === name) anchor = o;
			});
		}
		if (!anchor) {
			log.push(`${pad}  ${at.part.record}: no bone or node ${at.bone}`);
			continue;
		}
		const on = name ? `${at.bone} (${bone ? 'bone' : 'node'})` : 'its skeleton (skinned)';
		log.push(`${pad}  on ${on}${at.item_helper ? ` by its ${at.item_helper}` : ''}:`);
		const child = await buildItem(at.part, log, depth + 1, character, looks);
		// The port's offset, then the child moved so its own helper (not its
		// origin) sits on the port.
		const holder = new THREE.Group();
		holder.position.set(...at.offset_position);
		holder.quaternion.set(...at.offset_rotation);
		if (at.item_helper) {
			const h = await helperMatrix(at.part, child, at.item_helper);
			if (h) {
				child.matrix.copy(h.invert());
				child.matrixAutoUpdate = false;
			} else log.push(`${pad}  ${at.part.record}: no helper ${at.item_helper}`);
		}
		holder.add(child);
		anchor.add(holder);
	}
	return group;
}

/**
 * An item's helper (a bone of its skeleton, else a node of its geometry) in
 * the item's own space.
 */
async function helperMatrix(part: ScenePart, group: THREE.Object3D, name: string): Promise<THREE.Matrix4 | null> {
	const lower = name.toLowerCase();
	if (part.skeleton) {
		const res = await commands.loadSkeleton(part.skeleton);
		const bone = res.status === 'ok' ? res.data.find((b) => b.name.toLowerCase() === lower) : undefined;
		if (bone)
			return new THREE.Matrix4().compose(
				new THREE.Vector3(...bone.position),
				new THREE.Quaternion(...bone.rotation),
				new THREE.Vector3(1, 1, 1)
			);
	}
	let node: THREE.Object3D | undefined;
	group.traverse((o) => {
		if (!node && !(o as THREE.Mesh).isMesh && o.name.toLowerCase() === lower) node = o;
	});
	if (!node) return null;
	group.updateMatrixWorld(true);
	return new THREE.Matrix4().copy(group.matrixWorld).invert().multiply(node.matrixWorld);
}

async function buildModel(info: ModelInfo, subs: SubMaterialInfo[], log: string[], palette?: Palette, look?: PartLook) {
	const buf = await fetchModelBuffer(info.path);
	const n = info.vertex_count;
	const geomAttrs: Record<string, THREE.BufferAttribute> = {
		position: new THREE.BufferAttribute(new Float32Array(buf, info.positions_offset, n * 3), 3)
	};
	if (info.normals_offset != null)
		geomAttrs.normal = new THREE.BufferAttribute(new Float32Array(buf, info.normals_offset, n * 3), 3);
	if (info.uvs_offset != null) geomAttrs.uv = new THREE.BufferAttribute(new Float32Array(buf, info.uvs_offset, n * 2), 2);
	// Raw vertex colour under its own name: `color` would make three tint the
	// diffuse with it; LayerBlend decodes decal coordinates from it instead.
	if (info.colors_offset != null)
		geomAttrs.scColor = new THREE.BufferAttribute(new Uint8Array(buf, info.colors_offset, n * 4), 4, true);
	const indices = new Uint32Array(buf, info.indices_offset, info.index_count);

	// Node tree; transforms are parent-relative 3×4 row-major.
	const nodes = info.nodes.map((nd) => {
		const o = new THREE.Object3D();
		o.name = nd.name;
		const t = nd.transform;
		o.matrix.set(t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], 0, 0, 0, 1);
		o.matrixAutoUpdate = false;
		return o;
	});
	const root = new THREE.Group();
	nodes.forEach((o, i) => {
		const p = info.nodes[i].parent;
		(p != null && nodes[p] ? nodes[p] : root).add(o);
	});

	const materialCache = new Map<number, Promise<THREE.Material | null>>();
	let skipped = 0;
	for (const sm of info.submeshes) {
		const sub = subs[sm.material];
		if (!materialCache.has(sm.material)) {
			materialCache.set(sm.material, sub ? buildMaterial(sub, palette, look) : Promise.resolve(null));
		}
		const material = await materialCache.get(sm.material)!;
		if (!material) {
			skipped++;
			continue;
		}
		const g = new THREE.BufferGeometry();
		for (const [k, a] of Object.entries(geomAttrs)) g.setAttribute(k, a);
		g.setIndex(new THREE.BufferAttribute(indices.subarray(sm.first_index, sm.first_index + sm.index_count), 1));
		const mesh = new THREE.Mesh(g, material);
		mesh.name = sub?.name ?? `sub${sm.material}`;
		(sm.node != null && nodes[sm.node] ? nodes[sm.node] : root).add(mesh);
	}
	log.push(`${info.submeshes.length - skipped} submeshes drawn, ${skipped} skipped (proxy / NoDraw)`);
	for (const [i, sub] of subs.entries()) log.push(`  [${i}] ${sub.name} · ${sub.shader} · ${sub.features.join(' ')}`);
	return root;
}
