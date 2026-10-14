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

/** A tint palette entry (A, B, C), sRGB bytes; gloss 0..255. */
export interface PaletteEntry {
	tint: [number, number, number];
	spec: [number, number, number];
	gloss: number;
}

export type Palette = [PaletteEntry, PaletteEntry, PaletteEntry];

/** A further geometry of the same item, placed at the item origin. */
export interface FixturePart {
	geometry: string;
	material?: string;
}

/**
 * A part on a skeleton bone: a .cdf CA_BONE attachment, or a default-loadout
 * child on its item port's helper bone (SItemPortDefAttachmentImplementationBone).
 * Drawn with its own material, not the item's skin palette.
 */
export interface FixtureAttachment extends FixturePart {
	bone: string;
	/** The attached item's own default palette (its SubGeometry `Palette`). */
	palette?: Palette;
}

export interface Fixture {
	id: string;
	label: string;
	geometry: string;
	/** `.mtl` from the DCB; when absent the geometry's MtlName chunk names it. */
	material?: string;
	/** Further geometries (e.g. a .cdf's CA_BONE attachment on `root`). */
	parts?: FixturePart[];
	/** `.chr` skeleton the attachments hang on (the .cdf's Model). */
	skeleton?: string;
	attachments?: FixtureAttachment[];
	/** The skin variant's tint palette (SubGeometry `Palette`). */
	palette?: Palette;
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

const WEAPONS_PRIMARY_RIG: RigLight[] = [
	{ name: 'rim', pos: [0.47229, -0.06081, 0.85692], rot: [-63.7861, 61.85, 171.2527], color: [1, 1, 1], intensity: 5, radius: 2, fov: 70 },
	{ name: 'key', pos: [-0.9053, -0.02471, 0.76301], rot: [-90.842, 39.8225, 10.5107], color: [1, 1, 1], intensity: 0.5, radius: 3, fov: 80 },
	{ name: 'fill', pos: [0.94634, -0.20077, -0.20853], rot: [-1.7429, -10.9212, 168.8429], color: [1, 1, 1], intensity: 0.1, radius: 2, fov: 60 }
];

const WEAPONS_SIDEARMS_RIG: RigLight[] = [
	{ name: 'rim', pos: [0.31945, 0.29371, 0.79426], rot: [-105.3224, 50.0775, -132.8656], color: [1, 1, 1], intensity: 5, radius: 2, fov: 70 },
	{ name: 'key', pos: [-0.834, -0.5154, 0.63952], rot: [-93.3101, 33.8256, 37.2065], color: [1, 1, 1], intensity: 0.2, radius: 3, fov: 80 },
	{ name: 'fill', pos: [0.96229, -0.94286, -0.18237], rot: [-5.7567, -7.811, 135.7253], color: [1, 1, 1], intensity: 0.1, radius: 2, fov: 60 }
];

/** tintpalettes/brand/behr/weapons/behr_weapon_default */
const BEHR_DEFAULT: Palette = [
	{ tint: [0, 0, 0], spec: [143, 143, 143], gloss: 207 },
	{ tint: [51, 51, 51], spec: [53, 54, 57], gloss: 201 },
	{ tint: [39, 39, 39], spec: [56, 56, 56], gloss: 171 }
];

/** tintpalettes/brand/gmni/weapons/gmni_weapon_default */
const GMNI_DEFAULT: Palette = [
	{ tint: [32, 32, 32], spec: [46, 50, 57], gloss: 200 },
	{ tint: [43, 43, 43], spec: [59, 59, 59], gloss: 173 },
	{ tint: [43, 43, 43], spec: [59, 59, 59], gloss: 174 }
];

/** tintpalettes/brand/ksar/weapons/ksar_weapon_pink_blue */
const KSAR_PINK_BLUE: Palette = [
	{ tint: [177, 67, 92], spec: [177, 67, 92], gloss: 220 },
	{ tint: [177, 67, 92], spec: [177, 67, 92], gloss: 219 },
	{ tint: [25, 173, 227], spec: [53, 53, 53], gloss: 214 }
];

/** tintpalettes/brand/gmni/weapons/gmni_weapon_pink_pink_cian */
const GMNI_PINK_PINK_CIAN: Palette = [
	{ tint: [255, 175, 208], spec: [255, 175, 208], gloss: 182 },
	{ tint: [222, 87, 141], spec: [222, 87, 141], gloss: 164 },
	{ tint: [89, 212, 231], spec: [89, 212, 231], gloss: 181 }
];

export const FIXTURES: Fixture[] = [
	{
		id: 'p4ar_starkitten',
		label: 'P4-AR "Star Kitten"',
		// behr_rifle_ballistic_01_sf01: geometryTags Sf01 -> its SubGeometry's material + palette.
		geometry: 'objects/fps_weapons/weapons_v7/behr/rifle/p4ar/brfl_fps_behr_p4ar_parts.skin',
		material: 'objects/fps_weapons/weapons_v7/behr/rifle/p4ar/cosmetics/brfl_fps_behr_p4ar_sf_01_mat.mtl',
		skeleton: 'objects/fps_weapons/weapons_v7/behr/rifle/p4ar/brfl_fps_behr_p4ar.chr',
		attachments: [
			// default loadout: magazine_attach -> behr_rifle_ballistic_01_mag
			{ geometry: 'objects/fps_weapons/attachments/magazines/behr/mgzn_s03_behr_5rb_01.cgf', bone: 'magAttach', palette: BEHR_DEFAULT }
		],
		palette: KSAR_PINK_BLUE,
		offset: [0, 125, 5],
		rig: WEAPONS_PRIMARY_RIG,
		aspect: 2
	},
	{
		id: 'lh86_takahashi',
		label: 'LH86 "Takahashi Racing"',
		// gmni_pistol_ballistic_01_pink_cian01: geometryTags Pink_cian01 -> base material, own palette.
		geometry: 'objects/fps_weapons/weapons_v7/gmni/pistol/lh86/bpst_fps_gmni_lh86_parts.skin',
		material: 'objects/fps_weapons/weapons_v7/gmni/pistol/lh86/bpst_fps_gmni_lh86_mat.mtl',
		parts: [{ geometry: 'objects/fps_weapons/weapons_v7/gmni/pistol/lh86/bpst_fps_gmni_lh86_body.cgf' }],
		skeleton: 'objects/fps_weapons/weapons_v7/gmni/pistol/lh86/bpst_fps_gmni_lh86.chr',
		attachments: [
			// .cdf CA_BONE ironsight_attachment
			{ geometry: 'objects/fps_weapons/attachments/ironsights/gmni/optc_s01_gmni_r21.cgf', bone: 'sight_attachment' },
			// default loadout: magazine_attach -> gmni_pistol_ballistic_01_mag
			{ geometry: 'objects/fps_weapons/attachments/magazines/gmni/mgzn_s01_gmni_10pb_01.cgf', bone: 'magAttach', palette: GMNI_DEFAULT }
		],
		palette: GMNI_PINK_PINK_CIAN,
		offset: [0, 125, 5],
		rig: WEAPONS_SIDEARMS_RIG,
		aspect: 2
	},
	{
		id: 'js400',
		label: 'A&R JS-400 (power plant)',
		geometry: 'objects/spaceships/power_plants/medium_component/amrs/powr_amrs_s02_pl01.cga',
		offset: [20, 45, 17],
		rig: MASTER_RIG,
		aspect: 1
	},
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
