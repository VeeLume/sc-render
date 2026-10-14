// Test items. STOPGAP: geometry, material, pose offset and light rig
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
 * One item's geometry: a model plus further parts drawn with its palette,
 * and the items attached to it (which may carry attachments of their own).
 */
export interface ItemPart extends FixturePart {
	/** `.mtl` from the DCB; when absent the geometry's MtlName chunk names it. */
	material?: string;
	/** Further geometries (e.g. a .cdf's CA_BONE attachment on `root`). */
	parts?: FixturePart[];
	/** `.chr` skeleton the attachments hang on (the .cdf's Model). */
	skeleton?: string;
	attachments?: FixtureAttachment[];
	/** The item's tint palette (its geometry node's `Palette`). */
	palette?: Palette;
}

/**
 * An item on a parent's attach point: a .cdf CA_BONE attachment, or a
 * loadout child on its item port's helper (SItemPortDefAttachmentImplementationBone).
 * `bone` names a skeleton bone or, without one, a node of the parent's
 * geometry (a .cga helper such as a rack's `$slot_rifle_1`).
 */
export interface FixtureAttachment extends ItemPart {
	bone: string;
}

export interface Fixture extends ItemPart {
	id: string;
	label: string;
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

/** tintpalettes/brand/behr/weapons/behr_optic_default */
const BEHR_OPTIC_DEFAULT: Palette = [
	{ tint: [51, 51, 51], spec: [143, 139, 136], gloss: 139 },
	{ tint: [29, 29, 29], spec: [65, 65, 65], gloss: 235 },
	{ tint: [14, 14, 14], spec: [68, 68, 68], gloss: 155 }
];

/** tintpalettes/brand/hdgw/weapons/hdgw_grey_black */
const HDGW_GREY_BLACK: Palette = [
	{ tint: [0, 0, 0], spec: [179, 179, 179], gloss: 255 },
	{ tint: [55, 55, 55], spec: [54, 54, 54], gloss: 238 },
	{ tint: [39, 39, 39], spec: [44, 44, 44], gloss: 204 }
];

/** tintpalettes/brand/hdgw/weapons/hdgw_grey_brown_black */
const HDGW_GREY_BROWN_BLACK: Palette = [
	{ tint: [0, 0, 0], spec: [179, 179, 179], gloss: 230 },
	{ tint: [245, 117, 43], spec: [68, 68, 68], gloss: 230 },
	{ tint: [0, 0, 0], spec: [188, 188, 188], gloss: 213 }
];

// Valerie's hangar rack (inventory.json, Hangar_XLTop_NewBabbage): port_crc is
// crc32c(port name), e.g. 1163995043 = `$slot_rifle_1`.

/** behr_sniper_ballistic_01 with its loadout from the inventory dump. */
const P6LR: ItemPart = {
	// .cdf: CA_SKIN parts + CA_BONE body on root, CA_BONE bullet; base record -> behr_weapon_default.
	geometry: 'objects/fps_weapons/weapons_v7/behr/sniper/p6lr/bsnp_fps_behr_p6lr_parts.skin',
	material: 'objects/fps_weapons/weapons_v7/behr/sniper/p6lr/bsnp_fps_behr_p6lr_mat.mtl',
	parts: [{ geometry: 'objects/fps_weapons/weapons_v7/behr/sniper/p6lr/bsnp_fps_behr_p6lr_body.cgf' }],
	skeleton: 'objects/fps_weapons/weapons_v7/behr/sniper/p6lr/bsnp_fps_behr_p6lr.chr',
	attachments: [
		{ geometry: 'objects/fps_weapons/ammo/ccti/ammo_fps_ccti_50cal.cgf', bone: 'bullet' },
		// barrel_attach -> arma_barrel_comp_s2_03 (no palette)
		{ geometry: 'objects/fps_weapons/attachments/compensators/arma/cmpn_s02_arma_03.cgf', bone: 'barrel_attachment' },
		// optics_attach -> hdgw_optics_tsco_x4_s2
		{
			geometry: 'objects/fps_weapons/attachments/sights_scopes/hdgw/optc_s02_hdgw_01.cgf',
			bone: 'sight_attachment',
			palette: HDGW_GREY_BLACK
		},
		// underbarrel_attach -> nvtc_ubarrel_lasr_s1 (no palette)
		{ geometry: 'objects/fps_weapons/attachments/laserpointers/nvtac/lsrp_s01_nvtac_01.cgf', bone: 'underbarrel_attachment' },
		// magazine_attach -> behr_sniper_ballistic_01_mag
		{
			geometry: 'objects/fps_weapons/attachments/magazines/behr/mgzn_s03_behr_p6lr.cgf',
			material: 'objects/fps_weapons/attachments/magazines/behr/mgzn_s03_behr_p6lr_mat.mtl',
			bone: 'magAttach',
			palette: BEHR_DEFAULT
		}
	],
	palette: BEHR_DEFAULT
};

/** hdgw_rifle_ballistic_01 with its loadout; a .cga, so ports are geometry nodes. */
const HDGW_RIFLE: ItemPart = {
	geometry: 'objects/fps_weapons/weapons_v7/hdgw/rifle/ballistic_01/hdgw_rifle_ballistic_01.cga',
	attachments: [
		{ geometry: 'objects/fps_weapons/attachments/compensators/arma/cmpn_s02_arma_01.cgf', bone: 'barrel_attachment' },
		{
			geometry: 'objects/fps_weapons/attachments/sights_scopes/behr/optc_s01_behr_04.cga',
			bone: 'sight_attachment',
			palette: BEHR_OPTIC_DEFAULT
		},
		{ geometry: 'objects/fps_weapons/attachments/laserpointers/klwe/lsrp_s01_klwe_01.cgf', bone: 'underbarrel_attachment' },
		{
			geometry: 'objects/fps_weapons/attachments/magazines/hdgw/mgzn_hdgw_rifle_ballistic_01_a.cgf',
			bone: 'magAttach',
			palette: HDGW_GREY_BROWN_BLACK
		}
	],
	palette: HDGW_GREY_BROWN_BLACK
};

/** PlayerDeco_Gunrack_4P_3R_lowtech_1_a: Misc / Weapon -> the Misc UNDEFINED category, default rig. */
const RACK = {
	geometry: 'objects/buildingsets/human/lowtech/prop/gunrack/gunrack_1_a.cga',
	offset: [0, 155, 2.5] as [number, number, number],
	rig: MASTER_RIG,
	aspect: 1
};

export const FIXTURES: Fixture[] = [
	{ id: 'p6lr', label: 'P6-LR (rack loadout)', ...P6LR, offset: [0, 125, 5], rig: WEAPONS_PRIMARY_RIG, aspect: 2 },
	{ id: 'rack', label: 'Besco weapon rack', ...RACK },
	{
		id: 'rack_loaded',
		label: 'Besco weapon rack (loaded)',
		...RACK,
		attachments: [
			{ ...P6LR, bone: '$slot_rifle_1' },
			{ ...HDGW_RIFLE, bone: '$slot_rifle_2' }
		]
	},
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
