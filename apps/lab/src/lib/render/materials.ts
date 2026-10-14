// SC submaterials as three.js materials. Each shader family is a
// MeshStandardMaterial whose texture sampling is patched in onBeforeCompile,
// so lighting and shadows stay three's own.
//
// Reconstructed behaviour, tuned against in-game screenshots (StarBreaker's
// Blender addon is the reference for slot roles):
//   Illum          TexSlot1 diffuse, TexSlot2 normal (+ gloss in `_ddna`),
//                  Specular = F0, Shininess / 255 scales gloss.
//   LayerBlend_V2  MatLayers BaseLayer1 / WearLayer1 (each a `Layer` .mtl with
//                  TexSlot1 diffuse, TexSlot2 ddna), mixed by TexSlot11 (wear
//                  mask); TexSlot3 macro normal; layer textures tiled by the
//                  layer's UVTiling. A layer with PaletteTint 1-3 takes the
//                  tint palette's entry A-C: colour multiplied in, its spec as
//                  F0, its glossiness scaling gloss. With the DECALS feature,
//                  TexSlot9 is a decal sheet placed by the vertex colour (24
//                  bits -> two 12-bit coordinates, StarBreaker addon's
//                  layer_blend_decals); DiffuseDecalGloss / DiffuseDecalSpec
//                  are the decal's own surface.
import * as THREE from 'three';
import { commands, type LayerInfo, type MaterialInfo, type SubMaterialInfo } from '$lib/bindings';
import { fetchTexture } from './protocol';
import type { Palette } from './fixtures';

const NO_DRAW = new Set(['NoDraw']);

/**
 * Global wear strength. Wear factor = (1 - TexSlot11 mask) x WearBlendBase x
 * this. The mask is white where the paint is intact: read directly, the
 * Star Kitten's pink paint turns to its bare-metal wear layer, inverted only
 * edges and crevices wear. An item's real wear is runtime state; 0 renders
 * it pristine, which is how new items show in game.
 */
export const wearSettings = { strength: 0 };

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
		shader.fragmentShader = shader.fragmentShader
			.replace('#include <common>', '#include <common>\nuniform float uGlossMul;\nuniform vec3 uF0;\nuniform sampler2D uSpecMap;')
			.replace(
				'#include <roughnessmap_fragment>',
				hasGlossMap
					? 'float roughnessFactor = 1.0 - texture2D( roughnessMap, vRoughnessMapUv ).g * uGlossMul;'
					: 'float roughnessFactor = 1.0 - uGlossMul;'
			)
			.replace(
				F0_LINE,
				specMap
					? 'material.specularColor = uF0 * texture2D( uSpecMap, vMapUv ).rgb;'
					: 'material.specularColor = uF0;'
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

interface ResolvedLayer {
	diff: THREE.Texture | null;
	nrm: THREE.Texture | null;
	gloss: THREE.Texture | null;
	color: THREE.Color;
	f0: THREE.Color;
	glossMul: number;
	tiling: number;
}

async function resolveLayer(l: LayerInfo | undefined, palette?: Palette): Promise<ResolvedLayer> {
	const fallback: ResolvedLayer = {
		diff: null,
		nrm: null,
		gloss: null,
		color: new THREE.Color(0.5, 0.5, 0.5),
		f0: new THREE.Color(0.04, 0.04, 0.04),
		glossMul: 0.5,
		tiling: 1
	};
	if (!l) return fallback;
	const mtl = await loadMaterial(l.path);
	const sub = mtl?.submaterials.find((s) => s.name === l.submaterial) ?? mtl?.submaterials[0];
	if (!sub) return { ...fallback, color: color(l.tint) };
	const diffSlot = slot(sub, 'TexSlot1');
	const nrmSlot = slot(sub, 'TexSlot2');
	const entry = l.palette_tint >= 1 && l.palette_tint <= 3 ? palette?.[l.palette_tint - 1] : undefined;
	return {
		diff: diffSlot ? await fetchTexture(diffSlot.file, 'color') : null,
		nrm: nrmSlot ? await fetchTexture(nrmSlot.file, 'normal') : null,
		gloss: nrmSlot && hasGloss(nrmSlot.file) ? await fetchTexture(nrmSlot.file, 'gloss') : null,
		color: color(sub.diffuse)
			.multiply(color(l.tint))
			.multiply(entry ? srgb(entry.tint) : new THREE.Color(1, 1, 1)),
		f0: entry ? srgb(entry.spec) : color(sub.specular),
		glossMul: (sub.shininess / 255) * l.gloss_mult * (entry ? entry.gloss / 255 : 1),
		tiling: l.uv_tiling || 1
	};
}

const WHITE = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
WHITE.needsUpdate = true;
const FLAT = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
FLAT.needsUpdate = true;
const BLACK = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
BLACK.needsUpdate = true;

async function layerBlend(s: SubMaterialInfo, palette?: Palette): Promise<THREE.Material> {
	const base = await resolveLayer(s.layers.find((l) => l.name.startsWith('BaseLayer')) ?? s.layers[0], palette);
	const wear = await resolveLayer(s.layers.find((l) => l.name.startsWith('WearLayer')), palette);
	const decalSlot =
		s.features.includes('DECALS') && !s.features.includes('WEAR_DIRT_AO_VERTEX') ? slot(s, 'TexSlot9') : undefined;
	const decal = decalSlot ? await fetchTexture(decalSlot.file, 'color') : null;
	if (decal) decal.wrapS = decal.wrapT = THREE.ClampToEdgeWrapping;
	const param = (k: string) => s.params.find(([key]) => key === k)?.[1];
	const decalGloss = Number(param('DiffuseDecalGloss') ?? 10) / 255;
	const decalSpec = (param('DiffuseDecalSpec') ?? '0.04,0.04,0.04').split(',').map(Number) as [number, number, number];
	const wearSlot = slot(s, 'TexSlot11');
	const wearMask = wearSlot ? await fetchTexture(wearSlot.file, 'color') : null;
	const macroSlot = [slot(s, 'TexSlot3'), slot(s, 'TexSlot2')].find((t) => t && isNormal(t.file));
	const macro = macroSlot ? await fetchTexture(macroSlot.file, 'normal') : null;

	const m = new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 1, metalness: 0 });
	// Any texture enables the UV varying (vMapUv) and the tangent-space normal path.
	m.map = base.diff ?? WHITE;
	m.normalMap = base.nrm ?? FLAT;
	m.normalScale.set(1, -1);
	m.onBeforeCompile = (shader) => {
		Object.assign(shader.uniforms, {
			uBaseDiff: { value: base.diff ?? WHITE },
			uWearDiff: { value: wear.diff ?? WHITE },
			uBaseNrm: { value: base.nrm ?? FLAT },
			uWearNrm: { value: wear.nrm ?? FLAT },
			uBaseGloss: { value: base.gloss ?? WHITE },
			uWearGloss: { value: wear.gloss ?? WHITE },
			uWearMask: { value: wearMask ?? BLACK },
			uWearAmount: { value: Number(param('WearBlendBase') ?? 1) * wearSettings.strength },
			uMacroNrm: { value: macro ?? FLAT },
			uBaseColor: { value: base.color },
			uWearColor: { value: wear.color },
			uBaseTile: { value: base.tiling },
			uWearTile: { value: wear.tiling },
			uBaseGlossMul: { value: base.glossMul },
			uWearGlossMul: { value: wear.glossMul },
			uBaseF0: { value: base.f0 },
			uWearF0: { value: wear.f0 },
			uDecal: { value: decal ?? BLACK },
			uHasDecal: { value: decal ? 1 : 0 },
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
uniform sampler2D uBaseDiff, uWearDiff, uBaseNrm, uWearNrm, uBaseGloss, uWearGloss, uWearMask, uMacroNrm;
uniform vec3 uBaseColor, uWearColor, uBaseF0, uWearF0, uDecalSpec;
uniform float uBaseTile, uWearTile, uBaseGlossMul, uWearGlossMul, uHasDecal, uDecalGloss, uWearAmount;
uniform sampler2D uDecal;
varying vec2 vDecalUv;
float wearF;
float decalA;`
			)
			.replace(
				'#include <map_fragment>',
				`wearF = clamp( ( 1.0 - texture2D( uWearMask, vMapUv ).r ) * uWearAmount, 0.0, 1.0 );
vec3 cBase = texture2D( uBaseDiff, vMapUv * uBaseTile ).rgb * uBaseColor;
vec3 cWear = texture2D( uWearDiff, vMapUv * uWearTile ).rgb * uWearColor;
diffuseColor.rgb *= mix( cBase, cWear, wearF );
float inDiffuse = step( 2.0, vDecalUv.x ) * ( 1.0 - step( 3.0, vDecalUv.x ) );
float inStencil = step( 2.0, vDecalUv.y ) * ( 1.0 - step( 3.0, vDecalUv.y ) );
vec4 decalTex = texture2D( uDecal, vec2( vDecalUv.x - 2.0, vDecalUv.y ) );
decalA = uHasDecal * decalTex.a * inDiffuse * ( 1.0 - inStencil ) * ( 1.0 - wearF );
diffuseColor.rgb = mix( diffuseColor.rgb, decalTex.rgb, decalA );`
			)
			.replace(
				'#include <roughnessmap_fragment>',
				`float roughnessFactor = 1.0 - mix(
	texture2D( uBaseGloss, vMapUv * uBaseTile ).g * uBaseGlossMul,
	texture2D( uWearGloss, vMapUv * uWearTile ).g * uWearGlossMul,
	wearF );
roughnessFactor = mix( roughnessFactor, 1.0 - uDecalGloss, decalA );`
			)
			.replace(F0_LINE, 'material.specularColor = mix( mix( uBaseF0, uWearF0, wearF ), uDecalSpec, decalA );')
			.replace(
				'vec3 mapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;',
				`vec3 nLayer = normalize( mix(
	texture2D( uBaseNrm, vMapUv * uBaseTile ).xyz * 2.0 - 1.0,
	texture2D( uWearNrm, vMapUv * uWearTile ).xyz * 2.0 - 1.0,
	wearF ) );
vec3 nMacro = texture2D( uMacroNrm, vMapUv ).xyz * 2.0 - 1.0;
vec3 mapN = normalize( vec3( nMacro.xy + nLayer.xy, nMacro.z * nLayer.z ) );`
			);
	};
	return m;
}
