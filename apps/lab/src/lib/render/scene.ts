// One inventory cell, set up the way the game's item window is
// (ui/buildingblocks/inventory/components/item/inventory_panel_item_itemwindow):
// a Y-up UI scene, camera fieldOfView 10 looking along +Z with screen-up -Y,
// the item rotated by defaultOrientation (90,0,0) + its category offset and
// fitted to 70% of the cell's width, height and depth, lit by its category's
// light group (rotated (90,180,0), projectors along local +X).
import * as THREE from 'three';
import { commands, type ModelInfo, type SubMaterialInfo } from '$lib/bindings';
import { fetchModelBuffer } from './protocol';
import { buildMaterial, loadMaterial } from './materials';
import type { Fixture } from './fixtures';

const DEG = Math.PI / 180;
const FOV = 10;
/** Cryengine Ang3 → matrix Rz·Ry·Rx, which three calls order 'ZYX'. */
const ang3 = (d: [number, number, number]) => new THREE.Euler(d[0] * DEG, d[1] * DEG, d[2] * DEG, 'ZYX');

export interface CellOptions {
	lightScale: number;
	ambient: number;
}

export class Cell {
	readonly renderer: THREE.WebGLRenderer;
	readonly scene = new THREE.Scene();
	readonly camera: THREE.PerspectiveCamera;
	private item: THREE.Object3D | null = null;
	private lights: THREE.Object3D[] = [];
	private ambient = new THREE.AmbientLight(0x9aa6b8, 0.05);

	constructor(
		readonly canvas: HTMLCanvasElement,
		readonly aspect = 1
	) {
		this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
		this.renderer.outputColorSpace = THREE.SRGBColorSpace;
		this.renderer.toneMapping = THREE.AgXToneMapping;
		this.renderer.shadowMap.enabled = true;
		this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
		this.camera = new THREE.PerspectiveCamera(FOV, aspect, 0.1, 100);
		// A cell is 1 unit high; back off until it fills the vertical FOV.
		this.camera.position.set(0, 0, -0.5 / Math.tan((FOV / 2) * DEG));
		this.camera.up.set(0, -1, 0);
		this.camera.lookAt(0, 0, 0);
		this.scene.add(this.ambient);
	}

	resize(width: number, height: number) {
		this.renderer.setPixelRatio(window.devicePixelRatio);
		this.renderer.setSize(width, height, false);
		this.camera.aspect = width / height;
		this.camera.updateProjectionMatrix();
	}

	async show(fx: Fixture, opts: CellOptions): Promise<string[]> {
		const log: string[] = [];
		if (this.item) this.scene.remove(this.item);
		this.item = null;

		const res = await commands.loadModel(fx.geometry);
		if (res.status !== 'ok') throw new Error(res.error);
		const info = res.data;
		const mtlPath = fx.material ?? info.material;
		const mtl = mtlPath ? await loadMaterial(mtlPath) : null;
		log.push(`${info.vertex_count} vertices, ${info.index_count / 3} triangles, ${info.submeshes.length} submeshes`);
		log.push(`material ${mtlPath ?? '—'}: ${mtl?.submaterials.length ?? 0} submaterials`);

		const model = await buildModel(info, mtl?.submaterials ?? [], log);

		// Pose: defaultOrientation (90,0,0) + category offset, about the bounds centre.
		const rot = new THREE.Group();
		rot.rotation.copy(ang3([90 + fx.offset[0], fx.offset[1], fx.offset[2]]));
		rot.add(model);
		const pivot = new THREE.Group();
		pivot.add(rot);
		pivot.updateMatrixWorld(true);
		const box = visibleBounds(pivot);
		const size = box.getSize(new THREE.Vector3());
		const centre = box.getCenter(new THREE.Vector3());
		// ItemModel sizing: 70% of the cell's width, height and depth (depth = height).
		const s = 0.7 * Math.min(this.aspect / size.x, 1 / size.y, 1 / size.z);
		pivot.scale.setScalar(s);
		pivot.position.copy(centre).multiplyScalar(-s);
		pivot.traverse((o) => {
			if ((o as THREE.Mesh).isMesh) o.castShadow = o.receiveShadow = true;
		});
		this.item = pivot;
		this.scene.add(pivot);

		this.setLights(fx, opts);
		this.render();
		return log;
	}

	setLights(fx: Fixture, opts: CellOptions) {
		for (const l of this.lights) this.scene.remove(l);
		this.lights = [];
		const group = new THREE.Group();
		group.rotation.copy(ang3([90, 180, 0]));
		for (const l of fx.rig) {
			const spot = new THREE.SpotLight(
				new THREE.Color(...l.color),
				l.intensity * opts.lightScale,
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
			spot.castShadow = l.name !== 'fill';
			spot.shadow.mapSize.set(1024, 1024);
			spot.shadow.bias = -0.0005;
			group.add(holder);
		}
		this.scene.add(group);
		this.lights.push(group);
		this.ambient.intensity = opts.ambient;
	}

	render() {
		this.renderer.render(this.scene, this.camera);
	}
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

async function buildModel(info: ModelInfo, subs: SubMaterialInfo[], log: string[]) {
	const buf = await fetchModelBuffer(info.path);
	const n = info.vertex_count;
	const geomAttrs: Record<string, THREE.BufferAttribute> = {
		position: new THREE.BufferAttribute(new Float32Array(buf, info.positions_offset, n * 3), 3)
	};
	if (info.normals_offset != null)
		geomAttrs.normal = new THREE.BufferAttribute(new Float32Array(buf, info.normals_offset, n * 3), 3);
	if (info.uvs_offset != null) geomAttrs.uv = new THREE.BufferAttribute(new Float32Array(buf, info.uvs_offset, n * 2), 2);
	if (info.colors_offset != null)
		geomAttrs.color = new THREE.BufferAttribute(new Uint8Array(buf, info.colors_offset, n * 4), 4, true);
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
			materialCache.set(sm.material, sub ? buildMaterial(sub) : Promise.resolve(null));
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
