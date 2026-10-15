// SC submaterials as three.js materials. Each shader family is a
// MeshStandardMaterial whose texture sampling is patched in onBeforeCompile,
// so lighting and shadows stay three's own.
//
// Reconstructed behaviour, tuned against in-game screenshots (StarBreaker's
// Blender addon is the reference for slot roles):
//   Illum          TexSlot1 diffuse, TexSlot2 normal (+ gloss in `_ddna`),
//                  Specular = F0, Shininess / 255 scales gloss.
//   GlassPBR       Diffuse is the glass tint (1 = clear, 0.05 = a dark visor;
//                  Opacity stays 1 and is not transparency), Specular = F0,
//                  Shininess / 255 = gloss, TexSlot2 normal. Drawn as a
//                  transparent layer whose opacity grows as the tint darkens;
//                  it casts no shadow. Thin film, dirt and glare not modelled.
//   Organic        Rocks and crystals: TexSlot10 albedo (else TexSlot3 diffuse),
//                  TexSlot2 ddna; TransmittanceColor glows through when
//                  TRANSMITTANCE_LIGHTING is on (a stand-in for the game's
//                  light transmission). Its detail layers are not modelled.
//   LayerBlend_V2  Up to four MatLayers BaseLayer1-4 (each a `Layer` .mtl with
//                  TexSlot1 diffuse, TexSlot2 ddna). The rules below follow
//                  the game's own G-buffer shader (read from the local Vulkan
//                  shader cache; only the rules are kept here):
//                  - TexSlot12 blend map (sampled as sRGB, as the texture's
//                    engine flag asks), in priority order: layer 1 by R,
//                    layer 2 by G of the rest, layer 3 by B of what is left,
//                    the last layer fills the remainder.
//                  - Per layer (the game's layer packer): colour = layer
//                    Diffuse x submaterial Diffuse x texture; F0 = layer
//                    Specular; gloss = Shininess / 255 x GlossMult x texture
//                    gloss (the submaterial's Specular and Shininess play no
//                    part); tiling = UVTiling x the layer's LayerTiling.
//                  - The layer's TintMode says what is tinted: 1 the colour,
//                    2 the F0, 0 both. TintColor multiplies the colour; on
//                    F0 it applies as is to cloth, as hue only to metals and
//                    not at all otherwise. PaletteTint 1-3 takes the tint
//                    palette's entry A-C, multiplying the colour and
//                    replacing the F0 by the same split; the entry's
//                    glossiness always multiplies the layer gloss.
//                  - TexSlot13 HueAoLum map: .z scales colour and F0 by
//                    0.8-1.2, .x hue-shifts them through the engine's hue LUT,
//                    .y is ambient occlusion on both.
//                  - Layer ShadingModel 4 (fabrics) is drawn as sheen instead
//                    of specular, 1 and 3 (`_clearcoat` layers) with a clear
//                    coat: stand-ins for the game's deferred lighting models.
//                  TexSlot3 macro normal; layer textures tiled by the layer's
//                  UVTiling. With the DECALS feature, TexSlot9 is a decal
//                  sheet placed by the vertex colour (24 bits -> two 12-bit
//                  coordinates, StarBreaker addon's layer_blend_decals);
//                  DiffuseDecalGloss / DiffuseDecalSpec are the decal's own
//                  surface. Wear layers are not drawn (a new item has no
//                  wear, and the game then draws the base layers only);
//                  HEIGHT_MAP, vertex AO and the detail maps are not modelled.
import * as THREE from 'three';
import { commands, type LayerInfo, type MaterialInfo, type SubMaterialInfo } from '$lib/bindings';
import { fetchHueLut, fetchTexture } from './protocol';
import type { Palette } from './scene';

const NO_DRAW = new Set(['NoDraw']);

const materialCache = new Map<string, Promise<MaterialInfo | null>>();

export function loadMaterial(path: string): Promise<MaterialInfo | null> {
	const key = path.toLowerCase();
	let hit = materialCache.get(key);
	if (!hit) {
		hit = commands.loadMaterial(path).then((r) => {
			if (r.status === 'ok') return r.data;
			console.warn('material', path, r.error);
			return null;
		});
		materialCache.set(key, hit);
	}
	return hit;
}

function slot(s: SubMaterialInfo, name: string) {
	return s.textures.find((t) => t.slot === name);
}

const isNormal = (file: string) => /_ddna?\./i.test(file) || /_ddna?$/i.test(file);
const hasGloss = (file: string) => /_ddna\./i.test(file);

/**
 * CryEngine's specular/gloss workflow: diffuse at full strength, `Specular`
 * is F0. Patch three's physical shader so F0 is that colour (times the
 * optional specular map) instead of the IOR-derived 0.04; metalness stays 0.
 */
const F0_LINE =
	'material.specularColor = min( pow2( ( material.ior - 1.0 ) / ( material.ior + 1.0 ) ) * specularColorFactor, vec3( 1.0 ) ) * specularIntensityFactor;';

/**
 * Replace `line` inside three's shader chunk `name`. onBeforeCompile sees
 * chunks as unexpanded `#include`s, so a plain replace on the shader never
 * reaches their lines: the chunk is inlined with the change; `after` follows
 * it.
 */
function patchChunk(shader: string, name: keyof typeof THREE.ShaderChunk, line: string, by: string, after = ''): string {
	const chunk: string = THREE.ShaderChunk[name];
	if (!chunk.includes(line)) console.warn(`three changed ${name}: patch not applied`);
	return shader.replace(`#include <${name}>`, `${chunk.replace(line, by)}\n${after}`);
}

/** Set F0 to the GLSL expression `f0`; `after` runs once three's material setup is done. */
function withF0(fragmentShader: string, f0: string, after = ''): string {
	return patchChunk(fragmentShader, 'lights_physical_fragment', F0_LINE, `material.specularColor = ${f0};`, after);
}

const MAP_N_LINE = 'vec3 mapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;';

/**
 * The environment only reflects, and only on metals. The game's inventory
 * has no reflection source of its own (its light group's environment probe
 * is off), so paint, leather and fabric get the rig lights' highlights
 * alone; the studio stands in for whatever lets the game's metals read as
 * metal. Metalness by F0: none below 0.08 (dielectrics are 0.02-0.06), full
 * from 0.25 (the game's layer packer treats specular above 0.25 as metal).
 */
const IBL_LINE = '#include <lights_fragment_maps>';
const SPECULAR_ONLY_IBL = `${IBL_LINE}
iblIrradiance = vec3( 0.0 );
float scMetal = smoothstep( 0.08, 0.25, max( max( material.specularColor.r, material.specularColor.g ), material.specularColor.b ) );
radiance *= scMetal;
#ifdef USE_CLEARCOAT
clearcoatRadiance *= scMetal;
#endif`;
/** Glass keeps its reflections: they are what makes a visor read as glass. */
const GLASS_IBL = `${IBL_LINE}
iblIrradiance = vec3( 0.0 );`;

function color(c: [number, number, number]): THREE.Color {
	return new THREE.Color().setRGB(c[0], c[1], c[2]);
}

function srgb(c: [number, number, number]): THREE.Color {
	return new THREE.Color().setRGB(c[0] / 255, c[1] / 255, c[2] / 255, THREE.SRGBColorSpace);
}

/** One submaterial; `null` for parts that are not drawn (proxies). */
export async function buildMaterial(s: SubMaterialInfo, palette?: Palette): Promise<THREE.Material | null> {
	if (NO_DRAW.has(s.shader) || s.name.startsWith('$')) return null;
	if (s.shader.startsWith('LayerBlend')) return layerBlend(s, palette);
	if (s.shader === 'GlassPBR') return glass(s);
	if (s.shader === 'Organic') return organic(s);
	return illum(s);
}

async function illum(s: SubMaterialInfo): Promise<THREE.Material> {
	const m = new THREE.MeshPhysicalMaterial({
		color: color(s.diffuse),
		metalness: 0,
		roughness: 1,
		side: THREE.FrontSide
	});
	const diff = slot(s, 'TexSlot1');
	if (diff && !isNormal(diff.file)) m.map = await fetchTexture(diff.file, 'color');
	const nrm = [slot(s, 'TexSlot2'), slot(s, 'TexSlot3')].find((t) => t && isNormal(t.file));
	if (nrm) {
		m.normalMap = await fetchTexture(nrm.file, 'normal');
		m.normalScale.set(1, -1);
		if (hasGloss(nrm.file)) m.roughnessMap = await fetchTexture(nrm.file, 'gloss');
	}
	const specSlot = s.features.includes('SPECULAR_MAP') ? slot(s, 'TexSlot4') : undefined;
	const specMap = specSlot && m.map ? await fetchTexture(specSlot.file, 'color') : null;
	const hasGlossMap = !!m.roughnessMap;
	m.onBeforeCompile = (shader) => {
		Object.assign(shader.uniforms, {
			uGlossMul: { value: s.shininess / 255 },
			uF0: { value: color(s.specular) },
			uSpecMap: { value: specMap }
		});
		shader.fragmentShader = withF0(
			shader.fragmentShader
				.replace('#include <common>', '#include <common>\nuniform float uGlossMul;\nuniform vec3 uF0;\nuniform sampler2D uSpecMap;')
				.replace(
					'#include <roughnessmap_fragment>',
					hasGlossMap
						? 'float roughnessFactor = 1.0 - texture2D( roughnessMap, vRoughnessMapUv ).g * uGlossMul;'
						: 'float roughnessFactor = 1.0 - uGlossMul;'
				)
				.replace(IBL_LINE, SPECULAR_ONLY_IBL),
			specMap ? 'uF0 * texture2D( uSpecMap, vMapUv ).rgb' : 'uF0'
		);
	};
	if (s.features.includes('DECAL')) {
		m.transparent = true;
		m.depthWrite = false;
		m.polygonOffset = true;
		m.polygonOffsetFactor = -1;
		m.polygonOffsetUnits = -4;
	}
	if (s.alpha_test > 0) m.alphaTest = s.alpha_test;
	if (s.opacity < 1) {
		m.transparent = true;
		m.opacity = s.opacity;
	}
	return m;
}

async function glass(s: SubMaterialInfo): Promise<THREE.Material> {
	const tint = color(s.diffuse);
	const luminance = 0.2126 * tint.r + 0.7152 * tint.g + 0.0722 * tint.b;
	const m = new THREE.MeshPhysicalMaterial({
		color: tint,
		metalness: 0,
		roughness: 1 - s.shininess / 255,
		transparent: true,
		opacity: 0.15 + 0.75 * (1 - Math.min(luminance, 1)),
		depthWrite: false
	});
	const nrm = slot(s, 'TexSlot2');
	if (nrm && isNormal(nrm.file)) {
		m.normalMap = await fetchTexture(nrm.file, 'normal');
		m.normalScale.set(1, -1);
	}
	const f0 = color(s.specular);
	m.onBeforeCompile = (shader) => {
		shader.uniforms.uF0 = { value: f0 };
		shader.fragmentShader = withF0(
			shader.fragmentShader
				.replace('#include <common>', '#include <common>\nuniform vec3 uF0;')
				.replace(IBL_LINE, GLASS_IBL),
			'uF0'
		);
	};
	m.userData.noShadow = true;
	return m;
}

async function organic(s: SubMaterialInfo): Promise<THREE.Material> {
	const albedo = slot(s, 'TexSlot10') ?? slot(s, 'TexSlot3');
	const m = new THREE.MeshPhysicalMaterial({ color: color(s.diffuse), metalness: 0, roughness: 1 });
	if (albedo) m.map = await fetchTexture(albedo.file, 'color');
	const nrm = slot(s, 'TexSlot2');
	if (nrm && isNormal(nrm.file)) {
		m.normalMap = await fetchTexture(nrm.file, 'normal');
		m.normalScale.set(1, -1);
		if (hasGloss(nrm.file)) m.roughnessMap = await fetchTexture(nrm.file, 'gloss');
	}
	const param = (k: string) => s.params.find(([key]) => key === k)?.[1];
	const transmittance = (param('TransmittanceColor') ?? '').split(',').map(Number);
	if (s.features.includes('TRANSMITTANCE_LIGHTING') && transmittance.length === 3 && transmittance.every(Number.isFinite)) {
		m.emissive = color(transmittance as [number, number, number]).multiplyScalar(0.35);
	}
	const f0 = color(s.specular);
	const glossMul = s.shininess / 255;
	m.onBeforeCompile = (shader) => {
		Object.assign(shader.uniforms, { uF0: { value: f0 }, uGlossMul: { value: glossMul } });
		shader.fragmentShader = withF0(
			shader.fragmentShader
				.replace('#include <common>', '#include <common>\nuniform vec3 uF0;\nuniform float uGlossMul;')
				.replace(
					'#include <roughnessmap_fragment>',
					m.roughnessMap
						? 'float roughnessFactor = 1.0 - texture2D( roughnessMap, vRoughnessMapUv ).g * uGlossMul;'
						: 'float roughnessFactor = 1.0 - uGlossMul;'
				)
				.replace(IBL_LINE, SPECULAR_ONLY_IBL),
			'uF0'
		);
	};
	return m;
}

interface ResolvedLayer {
	diff: THREE.Texture | null;
	/** Normal X, Y in R, G; gloss in B (`normal_gloss`). */
	normalGloss: THREE.Texture | null;
	color: THREE.Color;
	f0: THREE.Color;
	/** 0..1, times the texture gloss. */
	gloss: number;
	tiling: number;
	/** Lighting-model weights: x cloth (ShadingModel 4), y clear coat (1, 3). */
	model: THREE.Vector2;
}

/**
 * The game's G-buffer flags ShadingModel 4 layers for its cloth lighting and
 * 1 / 3 for a second lighting model that the `_clearcoat` layers use; drawn
 * here as three's sheen and clear coat (approximations: the game's deferred
 * lighting for them has not been read).
 */
function shadingModel(sub: SubMaterialInfo): THREE.Vector2 {
	const sm = Number(sub.params.find(([k]) => k === 'ShadingModel')?.[1] ?? 0);
	return new THREE.Vector2(sm === 4 ? 1 : 0, sm === 1 || sm === 3 ? 1 : 0);
}

/**
 * What the MatLayer TintColor and a palette entry tint, by the layer's
 * `TintMode`: 1 the colour, 2 the specular, 0 both (the game's layer
 * packer; the same split sets the shader's palette flags 4 and 8).
 */
function tintTargets(sub: SubMaterialInfo): { color: boolean; spec: boolean } {
	const mode = Number(sub.params.find(([k]) => k === 'TintMode')?.[1] ?? 0);
	return { color: mode !== 2, spec: mode !== 1 };
}

/**
 * The TintColor's effect on a specular-tinted layer: cloth (ShadingModel 4)
 * takes it as is; a metal (specular above 0.25) takes only its hue, scaled
 * to a brightest channel of 1; anything else is left untinted.
 */
function specTint(sub: SubMaterialInfo, tint: THREE.Color): THREE.Color {
	if (shadingModel(sub).x > 0) return tint.clone();
	const peak = Math.max(tint.r, tint.g, tint.b);
	if (Math.max(...sub.specular) > 0.25 && peak > 0) return tint.clone().multiplyScalar(1 / peak);
	return new THREE.Color(1, 1, 1);
}

/** `parent`: the LayerBlend submaterial, whose Diffuse multiplies every layer's. */
async function resolveLayer(l: LayerInfo | undefined, palette?: Palette, parent?: SubMaterialInfo): Promise<ResolvedLayer> {
	// An empty slot, as the game's packer fills it: white, no specular, no gloss.
	const fallback: ResolvedLayer = {
		diff: null,
		normalGloss: null,
		color: new THREE.Color(1, 1, 1),
		f0: new THREE.Color(0, 0, 0),
		gloss: 0,
		tiling: 1,
		model: new THREE.Vector2()
	};
	if (!l) return fallback;
	const mtl = await loadMaterial(l.path);
	const sub = mtl?.submaterials.find((s) => s.name === l.submaterial) ?? mtl?.submaterials[0];
	if (!sub) return { ...fallback, color: color(l.tint) };
	const diffSlot = slot(sub, 'TexSlot1');
	const nrmSlot = slot(sub, 'TexSlot2');
	const entry = l.palette_tint >= 1 && l.palette_tint <= 3 ? palette?.[l.palette_tint - 1] : undefined;
	const tints = tintTargets(sub);
	const tint = color(l.tint);
	const layerColor = color(sub.diffuse);
	if (parent) layerColor.multiply(color(parent.diffuse));
	if (tints.color) layerColor.multiply(tint);
	if (entry && tints.color) layerColor.multiply(srgb(entry.tint));
	const f0 = color(sub.specular);
	if (tints.spec) f0.multiply(specTint(sub, tint));
	const layerTiling = Number(sub.params.find(([k]) => k === 'LayerTiling')?.[1] ?? 1) || 1;
	return {
		diff: diffSlot ? await fetchTexture(diffSlot.file, 'color') : null,
		normalGloss: nrmSlot ? await fetchTexture(nrmSlot.file, 'normal_gloss') : null,
		color: layerColor,
		f0: entry && tints.spec ? srgb(entry.spec) : f0,
		gloss: Math.min(1, (sub.shininess / 255) * l.gloss_mult) * (entry ? entry.gloss / 255 : 1),
		tiling: (l.uv_tiling || 1) * layerTiling,
		model: shadingModel(sub)
	};
}

const WHITE = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
WHITE.needsUpdate = true;
/** A flat `normal_gloss` texel: normal (0, 0), gloss 1. */
const FLAT_GLOSS = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
FLAT_GLOSS.needsUpdate = true;
const BLACK = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
BLACK.needsUpdate = true;

async function layerBlend(s: SubMaterialInfo, palette?: Palette): Promise<THREE.Material> {
	// Layers sit in the slot their name gives (BaseLayer1-4); a slot no layer
	// names holds the packer's empty layer, matte white (the Horizon helmet's
	// parts material names two; its blend-map black, slot 4, is white in game).
	const named = [1, 2, 3, 4].map((n) => s.layers.find((l) => l.name === `BaseLayer${n}`));
	if (!named.some(Boolean)) named[0] = s.layers[0];
	const neutral = await resolveLayer(undefined, palette);
	const layers = await Promise.all(named.map((l) => (l ? resolveLayer(l, palette, s) : undefined)));
	const blendSlot = s.features.includes('BLEND_MAP') ? slot(s, 'TexSlot12') : undefined;
	const blend = blendSlot ? await fetchTexture(blendSlot.file, 'mask') : null;
	const decalSlot =
		s.features.includes('DECALS') && !s.features.includes('WEAR_DIRT_AO_VERTEX') ? slot(s, 'TexSlot9') : undefined;
	const decal = decalSlot ? await fetchTexture(decalSlot.file, 'color') : null;
	if (decal) decal.wrapS = decal.wrapT = THREE.ClampToEdgeWrapping;
	const param = (k: string) => s.params.find(([key]) => key === k)?.[1];
	const decalGloss = Number(param('DiffuseDecalGloss') ?? 10) / 255;
	const decalSpec = (param('DiffuseDecalSpec') ?? '0.04,0.04,0.04').split(',').map(Number) as [number, number, number];
	const macroSlot = [slot(s, 'TexSlot3'), slot(s, 'TexSlot2')].find((t) => t && isNormal(t.file));
	const macro = macroSlot ? await fetchTexture(macroSlot.file, 'normal') : null;
	const halSlot = slot(s, 'TexSlot13');
	const hal = halSlot ? await fetchTexture(halSlot.file, 'mask') : null;
	const hueLut = hal ? await fetchHueLut() : null;
	// Shader slots: the base drawn where the blend map is black (layer 4),
	// then the layers its R, G, B channels mix in (layers 1, 2, 3). Without a
	// blend map, layer 1 covers everything.
	const base = (blend ? layers[3] : (layers[0] ?? layers.find(Boolean))) ?? neutral;
	const byChannel = blend ? [0, 1, 2].map((i) => layers[i] ?? neutral) : [neutral, neutral, neutral];
	const slot4 = [base, ...byChannel];
	const pad = (i: number) => slot4[i];

	const m = new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 1, metalness: 0 });
	// Layer 1 rides on three's own samplers (map, normalMap); they also turn
	// on the UV varying and the tangent-space normal path. WebView2 allows 16
	// samplers per shader; three takes three (environment, its DFG table, one
	// shadow map), which leaves 13: 8 for the layers, the blend map, and the
	// optional decal sheet, macro normal, HueAoLum map and hue LUT.
	m.map = base.diff ?? WHITE;
	m.normalMap = base.normalGloss ?? FLAT_GLOSS;
	m.normalScale.set(1, -1);
	if (slot4.some((l) => l.model.x > 0)) {
		m.sheen = 1;
		m.sheenRoughness = 0.6;
	}
	if (slot4.some((l) => l.model.y > 0)) {
		m.clearcoat = 1;
		m.clearcoatRoughness = 0.08;
	}
	// Optional samplers are declared only where used; each variant needs its
	// own program cache key (three keys programs by this function's text).
	const variant = [hal && hueLut ? 'SC_HAL' : '', decal ? 'SC_DECAL' : '', macro ? 'SC_MACRO' : ''].filter(Boolean);
	const defines = variant.map((d) => `#define ${d}\n`).join('');
	m.customProgramCacheKey = () => `layerBlend-${variant.join('-')}`;
	m.onBeforeCompile = (shader) => {
		Object.assign(shader.uniforms, {
			uHal: { value: hal },
			uHueLut: { value: hueLut },
			uL1Diff: { value: pad(1).diff ?? WHITE },
			uL2Diff: { value: pad(2).diff ?? WHITE },
			uL3Diff: { value: pad(3).diff ?? WHITE },
			uL1NG: { value: pad(1).normalGloss ?? FLAT_GLOSS },
			uL2NG: { value: pad(2).normalGloss ?? FLAT_GLOSS },
			uL3NG: { value: pad(3).normalGloss ?? FLAT_GLOSS },
			uBlend: { value: blend ?? BLACK },
			// Channel weights (R, G, B): all off without a blend map.
			uBlendWeights: { value: new THREE.Vector3(...[1, 1, 1].map((w) => (blend ? w : 0))) },
			uBlendTile: { value: Number(param('BlendMapTiling') ?? 1) || 1 },
			// Per-layer values as separate uniforms (no uniform arrays).
			...Object.fromEntries(
				[0, 1, 2, 3].flatMap((i) => [
					[`uColor${i}`, { value: pad(i).color }],
					[`uF0${i}`, { value: pad(i).f0 }],
					[`uGloss${i}`, { value: pad(i).gloss }],
					[`uTile${i}`, { value: pad(i).tiling }],
					[`uModel${i}`, { value: pad(i).model }]
				])
			),
			uMacroNrm: { value: macro },
			uDecal: { value: decal },
			uDecalGloss: { value: decalGloss },
			uDecalSpec: { value: color(decalSpec) }
		});
		// Decal coordinates ride in the vertex colour: decode per vertex so the
		// interpolated pair maps the sheet across the triangle.
		shader.vertexShader = shader.vertexShader
			.replace(
				'#include <common>',
				`#include <common>
attribute vec4 scColor;
varying vec2 vDecalUv;`
			)
			.replace(
				'#include <uv_vertex>',
				`#include <uv_vertex>
vec3 dc = floor( scColor.rgb * 255.0 + 0.5 );
float dPacked = dc.r * 65536.0 + dc.g * 256.0 + dc.b;
float dU = floor( dPacked / 4096.0 );
vDecalUv = vec2( dU, dPacked - dU * 4096.0 ) / 4095.0 * 4.0 - 0.5;`
			);
		shader.fragmentShader = shader.fragmentShader
			.replace(
				'#include <common>',
				`#include <common>
${defines}uniform sampler2D uL1Diff, uL2Diff, uL3Diff, uL1NG, uL2NG, uL3NG, uBlend;
#ifdef SC_DECAL
uniform sampler2D uDecal;
#endif
#ifdef SC_MACRO
uniform sampler2D uMacroNrm;
#endif
uniform vec3 uBlendWeights, uDecalSpec;
uniform vec3 uColor0, uColor1, uColor2, uColor3, uF00, uF01, uF02, uF03;
uniform float uGloss0, uGloss1, uGloss2, uGloss3;
uniform float uTile0, uTile1, uTile2, uTile3;
uniform vec2 uModel0, uModel1, uModel2, uModel3;
uniform float uBlendTile, uDecalGloss;
#ifdef SC_HAL
uniform sampler2D uHal;
uniform highp sampler3D uHueLut;
// The LUT holds two 8^3 colour cubes stacked in depth: positive shifts in
// the first half, negative in the second; |shift| blends towards it.
vec3 hueShift( vec3 c, float shift ) {
	vec3 uvw = c * 0.875 + 0.0625;
	uvw.z = 0.5 * ( uvw.z + step( shift, -1e-6 ) );
	return mix( c, texture( uHueLut, uvw ).rgb, abs( shift ) );
}
#endif
varying vec2 vDecalUv;
vec3 layerF0;
float layerGloss;
vec2 layerNormal;
vec2 layerModel;
float decalA;
vec3 blendLayer( vec3 c, float w, vec4 diff, vec4 ng, vec3 color, vec3 f0, float gloss, vec2 model ) {
	layerGloss = mix( layerGloss, ng.b * gloss, w );
	layerModel = mix( layerModel, model, w );
	layerF0 = mix( layerF0, f0, w );
	layerNormal = mix( layerNormal, ng.rg, w );
	return mix( c, diff.rgb * color, w );
}`
			)
			.replace(
				'#include <map_fragment>',
				`vec2 uv0 = vMapUv * uTile0;
vec4 ng0 = texture2D( normalMap, uv0 );
vec3 layerColor = texture2D( map, uv0 ).rgb * uColor0;
layerGloss = ng0.b * uGloss0;
layerF0 = uF00;
layerNormal = ng0.rg;
layerModel = uModel0;
vec3 bw = texture2D( uBlend, vMapUv * uBlendTile ).rgb * uBlendWeights;
vec2 uv1 = vMapUv * uTile1;
vec2 uv2 = vMapUv * uTile2;
vec2 uv3 = vMapUv * uTile3;
// Nested from the base up, so R outranks G outranks B: weights R,
// G(1-R), B(1-R)(1-G) and (1-R)(1-G)(1-B) for the base.
layerColor = blendLayer( layerColor, bw.b, texture2D( uL3Diff, uv3 ), texture2D( uL3NG, uv3 ), uColor3, uF03, uGloss3, uModel3 );
layerColor = blendLayer( layerColor, bw.g, texture2D( uL2Diff, uv2 ), texture2D( uL2NG, uv2 ), uColor2, uF02, uGloss2, uModel2 );
layerColor = blendLayer( layerColor, bw.r, texture2D( uL1Diff, uv1 ), texture2D( uL1NG, uv1 ), uColor1, uF01, uGloss1, uModel1 );
#ifdef SC_HAL
vec3 hal = texture2D( uHal, vMapUv ).rgb;
float halLum = hal.z * 0.4 + 0.8;
float halShift = clamp( ( hal.x - 0.5 ) * 2.0, -1.0, 1.0 );
layerColor = hueShift( clamp( layerColor * halLum, 0.0, 1.0 ), halShift ) * hal.y;
layerF0 = hueShift( clamp( layerF0 * halLum, 0.0, 1.0 ), halShift ) * hal.y;
#endif
diffuseColor.rgb *= layerColor;
decalA = 0.0;
#ifdef SC_DECAL
float inDiffuse = step( 2.0, vDecalUv.x ) * ( 1.0 - step( 3.0, vDecalUv.x ) );
float inStencil = step( 2.0, vDecalUv.y ) * ( 1.0 - step( 3.0, vDecalUv.y ) );
vec4 decalTex = texture2D( uDecal, vec2( vDecalUv.x - 2.0, vDecalUv.y ) );
decalA = decalTex.a * inDiffuse * ( 1.0 - inStencil );
diffuseColor.rgb = mix( diffuseColor.rgb, decalTex.rgb, decalA );
#endif`
			)
			.replace(
				'#include <roughnessmap_fragment>',
				`float roughnessFactor = mix( 1.0 - layerGloss, 1.0 - uDecalGloss, decalA );`
			)
			.replace(IBL_LINE, SPECULAR_ONLY_IBL);
		shader.fragmentShader = patchChunk(
			shader.fragmentShader,
			'normal_fragment_maps',
			MAP_N_LINE,
			`vec2 nxy = layerNormal * 2.0 - 1.0;
vec3 nLayer = vec3( nxy, sqrt( max( 1.0 - dot( nxy, nxy ), 0.0 ) ) );
#ifdef SC_MACRO
vec3 nMacro = texture2D( uMacroNrm, vMapUv ).xyz * 2.0 - 1.0;
vec3 mapN = normalize( vec3( nMacro.xy + nLayer.xy, nMacro.z * nLayer.z ) );
#else
vec3 mapN = nLayer;
#endif`
		);
		shader.fragmentShader = withF0(
			shader.fragmentShader,
			'mix( layerF0, uDecalSpec, decalA ) * ( 1.0 - layerModel.x * ( 1.0 - decalA ) )',
			// Cloth trades the specular lobe for a soft sheen of its own colour.
			`#ifdef USE_SHEEN
material.sheenColor = layerModel.x * ( 1.0 - decalA ) * sqrt( layerColor ) * 0.5;
#endif
#ifdef USE_CLEARCOAT
material.clearcoat = layerModel.y * ( 1.0 - decalA );
#endif`
		);
	};
	return m;
}
