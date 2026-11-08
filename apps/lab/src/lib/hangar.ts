// The lab's damage tooling on top of the library's `ShipView`: the collision
// shell shots hit, component boxes, and the results of shots and
// attack-direction sweeps. Everything is in the ship frame.
import * as THREE from 'three';
import { ShipView, type RenderSource, type ShipMode, type SliceAxis } from '@veelume/sc-render';
import type { ExposureDirection, HangarShip, ShipComponent, ShotResult } from '$lib/bindings';

/** Component colour by item type (`Type/SubType`'s type). */
const KIND_COLOURS: Record<string, number> = {
	PowerPlant: 0xffd23f,
	Cooler: 0x3fd7ff,
	Shield: 0x4f7dff,
	QuantumDrive: 0xb05cff,
	QuantumFuelTank: 0x8a6cff,
	FuelTank: 0x7fbf5a,
	FuelIntake: 0x5abf8a,
	LifeSupportGenerator: 0x6effa8,
	Radar: 0xff8fd0,
	WeaponGun: 0xff5a4f,
	Turret: 0xff9a3f,
	TurretBase: 0xff9a3f,
	MissileLauncher: 0xff7a7a,
	Missile: 0xff7a7a,
	Armor: 0x9aa4b0,
	Relay: 0xd0d0a0
};
const HIT_COLOUR = 0xff2020;
const SELECTED_COLOUR = 0xffffff;

function kindColour(kind: string): number {
	return KIND_COLOURS[kind.split('/')[0]] ?? 0xa0a0a0;
}

/** A distinct colour per damage part, the view's own (golden-angle hues). */
function partColour(i: number): THREE.Color {
	return new THREE.Color().setHSL((i * 0.618034) % 1, 0.55, 0.55);
}

export class Hangar {
	readonly view: ShipView;
	ship: HangarShip | null = null;
	private readonly shell = new THREE.Group();
	private readonly boxes = new THREE.Group();
	private readonly marks = new THREE.Group();
	private readonly boxByIndex = new Map<number, THREE.LineSegments>();
	private highlighted = new Set<number>();
	private selected: number | null = null;
	private generation = 0;

	constructor(canvas: HTMLCanvasElement, source: RenderSource) {
		this.view = new ShipView(canvas, source);
		this.shell.visible = false;
		this.view.scene.add(this.shell, this.boxes, this.marks);
	}

	/** Damage parts in display order; index = colour. */
	get parts(): string[] {
		return this.view.parts;
	}

	resize(width: number, height: number) {
		this.view.resize(width, height);
	}

	/** Draw `ship`; returns the render log. */
	async show(ship: HangarShip): Promise<string[]> {
		const generation = ++this.generation;
		const log = await this.view.show(ship.scene);
		if (generation !== this.generation) return log;
		this.ship = ship;
		this.buildShell(ship);
		this.buildBoxes(ship.components);
		this.selected = null;
		this.clearMarks();
		log.push(`${ship.parts.length} damage parts, ${ship.components.length} components, ${ship.scene.interior.length} interior brushes`);
		return log;
	}

	setMode(mode: ShipMode): Promise<void> {
		return this.view.setMode(mode);
	}

	setHoloOpacity(opacity: number) {
		this.view.setHoloOpacity(opacity);
	}

	setInterior(visible: boolean) {
		this.view.setInterior(visible);
	}

	setSlice(axis: SliceAxis, at: number, flip = false) {
		this.view.setSlice(axis, at, flip);
	}

	rayAt(x: number, y: number): THREE.Ray {
		return this.view.rayAt(x, y);
	}

	private buildShell(ship: HangarShip) {
		this.shell.clear();
		ship.parts.forEach((p, i) => {
			const g = new THREE.BufferGeometry();
			g.setAttribute('position', new THREE.Float32BufferAttribute(p.positions.flat(), 3));
			g.setIndex(p.indices);
			const mesh = new THREE.Mesh(
				g,
				new THREE.MeshBasicMaterial({ color: partColour(this.parts.indexOf(p.name)), wireframe: true, transparent: true, opacity: 0.35 })
			);
			mesh.name = p.name;
			mesh.userData.part = i;
			this.shell.add(mesh);
		});
	}

	private buildBoxes(components: ShipComponent[]) {
		this.boxes.clear();
		this.boxByIndex.clear();
		for (const c of components) {
			if (!c.min || !c.max) continue;
			const min = new THREE.Vector3(...c.min);
			const max = new THREE.Vector3(...c.max);
			const size = max.clone().sub(min);
			const geo = new THREE.EdgesGeometry(new THREE.BoxGeometry(Math.max(size.x, 1e-3), Math.max(size.y, 1e-3), Math.max(size.z, 1e-3)));
			geo.translate(...min.add(max).multiplyScalar(0.5).toArray());
			const line = new THREE.LineSegments(
				geo,
				new THREE.LineBasicMaterial({ color: kindColour(c.kind), depthTest: false, transparent: true, opacity: 0.9 })
			);
			line.renderOrder = 10;
			const t = c.transform;
			line.matrix.set(t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], 0, 0, 0, 1);
			line.matrixAutoUpdate = false;
			line.userData.component = c;
			// Only what penetration can damage, unless asked for everything.
			line.visible = c.health != null;
			this.boxes.add(line);
			this.boxByIndex.set(c.index, line);
		}
	}

	/** Every component box, or only those with health. */
	setAllBoxes(all: boolean) {
		for (const line of this.boxByIndex.values()) {
			const c = line.userData.component as ShipComponent;
			line.visible = all || c.health != null;
		}
		this.view.render();
	}

	setBoxes(visible: boolean) {
		this.boxes.visible = visible;
		this.view.render();
	}

	setShell(visible: boolean) {
		this.shell.visible = visible;
		this.view.render();
	}

	/** Highlight `index` (and the components a shot reached). */
	select(index: number | null) {
		this.selected = index;
		this.recolour();
	}

	/** A damageable component in its type colour, white selected, red when reached. */
	private colour(c: ShipComponent): number {
		return c.index === this.selected ? SELECTED_COLOUR : this.highlighted.has(c.index) ? HIT_COLOUR : kindColour(c.kind);
	}

	private recolour() {
		const colours = new Map<string, THREE.ColorRepresentation>();
		for (const c of this.ship?.components ?? []) if (c.health != null) colours.set(c.path, this.colour(c));
		this.view.setPortColours(colours);
		for (const [i, line] of this.boxByIndex) {
			const c = line.userData.component as ShipComponent;
			(line.material as THREE.LineBasicMaterial).color.set(this.colour(c));
			if (i === this.selected || this.highlighted.has(i)) line.visible = true;
		}
		this.view.render();
	}

	/**
	 * The component under canvas pixel (`x`, `y`): the nearest damageable
	 * component model the view ray meets, else the nearest visible box.
	 */
	componentAt(x: number, y: number): number | null {
		const path = this.view.at(x, y);
		if (path) {
			const hit = this.ship?.components.find((c) => c.path.toLowerCase() === path);
			if (hit) return hit.index;
		}
		const ray = this.view.rayAt(x, y);
		let best: { d: number; i: number } | null = null;
		for (const [i, line] of this.boxByIndex) {
			if (!line.visible || !this.boxes.visible) continue;
			const c = line.userData.component as ShipComponent;
			const box = new THREE.Box3(new THREE.Vector3(...c.min!), new THREE.Vector3(...c.max!));
			const local = ray.clone().applyMatrix4(line.matrix.clone().invert());
			const point = local.intersectBox(box, new THREE.Vector3());
			if (!point) continue;
			const d = point.applyMatrix4(line.matrix).distanceTo(ray.origin);
			if (!best || d < best.d) best = { d, i };
		}
		return best?.i ?? null;
	}

	clearMarks() {
		this.marks.clear();
		this.highlighted.clear();
		this.recolour();
	}

	/** A shot: the impact, the penetration along it, the components reached in red. */
	showShot(origin: THREE.Vector3, direction: THREE.Vector3, result: ShotResult) {
		this.marks.clear();
		this.highlighted = new Set(result.components.map((c) => c.index));
		const radius = this.view.radius;
		const dir = direction.clone().normalize();
		if (result.point) {
			const p = new THREE.Vector3(...result.point);
			const dot = new THREE.Mesh(
				new THREE.SphereGeometry(radius * 0.012, 16, 12),
				new THREE.MeshBasicMaterial({ color: 0xffe14f, depthTest: false })
			);
			dot.position.copy(p);
			dot.renderOrder = 11;
			this.marks.add(dot);
			const before = p.clone().addScaledVector(dir, -radius * 0.25);
			this.marks.add(this.line([before, p], 0xffe14f, 0.6));
			if (result.depth > 0) this.marks.add(this.line([p, p.clone().addScaledVector(dir, result.depth)], 0xff3b30, 1));
		} else {
			this.marks.add(this.line([origin, origin.clone().addScaledVector(dir, radius * 4)], 0x8a8f98, 0.5));
		}
		this.recolour();
	}

	/**
	 * Attack directions on component `c`: a line from its centre out to the
	 * hull surface per direction, green when `reach` gets through to it,
	 * shading to red the deeper it sits beyond that.
	 */
	showExposure(c: ShipComponent, dirs: ExposureDirection[], reach: number) {
		this.marks.clear();
		if (!c.min || !c.max) return;
		const t = c.transform;
		const m = new THREE.Matrix4().set(t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], 0, 0, 0, 1);
		const centre = new THREE.Vector3(...c.min).add(new THREE.Vector3(...c.max)).multiplyScalar(0.5).applyMatrix4(m);
		const positions: number[] = [];
		const colours: number[] = [];
		const colour = new THREE.Color();
		const far = Math.max(reach * 3, 1);
		for (const d of dirs) {
			// The shot travels along `direction`; it comes from the opposite side.
			const out = new THREE.Vector3(...d.direction).multiplyScalar(-1);
			const end = centre.clone().addScaledVector(out, Math.max(d.depth, 0.05));
			const k = d.depth <= reach ? 0 : Math.min(1, (d.depth - reach) / (far - reach));
			colour.setHSL(0.33 * (1 - k), 0.9, d.depth <= reach ? 0.55 : 0.45);
			positions.push(...centre.toArray(), ...end.toArray());
			colours.push(colour.r, colour.g, colour.b, colour.r, colour.g, colour.b);
		}
		const g = new THREE.BufferGeometry();
		g.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
		g.setAttribute('color', new THREE.Float32BufferAttribute(colours, 3));
		const lines = new THREE.LineSegments(
			g,
			new THREE.LineBasicMaterial({ vertexColors: true, depthTest: false, transparent: true, opacity: 0.8 })
		);
		lines.renderOrder = 12;
		this.marks.add(lines);
		this.view.render();
	}

	private line(points: THREE.Vector3[], colour: number, opacity: number): THREE.Line {
		const l = new THREE.Line(
			new THREE.BufferGeometry().setFromPoints(points),
			new THREE.LineBasicMaterial({ color: colour, depthTest: false, transparent: true, opacity })
		);
		l.renderOrder = 11;
		return l;
	}

	/** Free the WebGL context (a page leaving must call this; contexts are few). */
	dispose() {
		this.view.dispose();
	}
}
