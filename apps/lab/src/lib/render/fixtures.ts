// Stage-1 test items. STOPGAP: geometry, material, pose offset and light rig
// are written out by hand from the DCB (see the sc-holotable notes on
// inventory item icons). The typed per-item recipe from sc-holotable
// replaces this file.

export interface RigLight {
	name: string;
	/** Position in the light group's frame. */
	pos: [number, number, number];
	/** Ang3 degrees; projectors shine along local +X. */
	rot: [number, number, number];
	color: [number, number, number];
	intensity: number;
	radius: number;
	fov: number;
}

export interface Fixture {
	id: string;
	label: string;
	geometry: string;
	/** `.mtl` from the DCB; when absent the geometry's MtlName chunk names it. */
	material?: string;
	/** PIT `orientationOffset` of the item's category, degrees. */
	offset: [number, number, number];
	rig: RigLight[];
	/** Cell width / height. */
	aspect: number;
}

/** `inventorylightgroup_master` (also used by power plants, drinks, armor, …). */
const MASTER_RIG: RigLight[] = [
	{ name: 'rim', pos: [0.83, 0.63, 0.97], rot: [0, 39.5, -132], color: [1, 1, 1], intensity: 5, radius: 2, fov: 70 },
	{ name: 'key', pos: [-0.86, -1.08, 1.12], rot: [-5.5, 40.7, 50.5], color: [1, 1, 1], intensity: 10, radius: 3, fov: 80 },
	{ name: 'fill', pos: [0.27, -1.21, -0.09], rot: [-2.4, -3.4, 112.4], color: [1, 1, 1], intensity: 0.1, radius: 2, fov: 60 }
];

export const FIXTURES: Fixture[] = [
	{
		id: 'regulus',
		label: 'Aegis Regulus (power plant)',
		geometry: 'objects/spaceships/power_plants/small_component/aegs/powr_aegs_s01_pl03.cga',
		offset: [20, 45, 17],
		rig: MASTER_RIG,
		aspect: 1
	},
	{
		id: 'pips',
		label: 'PIPS energy drink',
		geometry: 'objects/buildingsets/human/universal/prop/dressing/can/can_drink_3_energy_a.cga',
		material: 'objects/buildingsets/human/universal/prop/dressing/can/can_drink_3_a.mtl',
		offset: [0, 180, 0],
		rig: MASTER_RIG,
		aspect: 1
	}
];
