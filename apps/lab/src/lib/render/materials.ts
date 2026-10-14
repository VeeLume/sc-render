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
//                  layer's UVTiling.
import * as THREE from 'three';
import { commands, type LayerInfo, type MaterialInfo, type SubMaterialInfo } from '$lib/bindings';
import { fetchTexture } from './protocol';

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

/** Dielectric below ~0.2 F0, metal above ~0.7; linear between. */
function metalness(spec: [number, number, number]): number {
	const f0 = Math.max(...spec);
	return THREE.MathUtils.clamp((f0 - 0.2) / 0.5, 0, 1);
}

function color(c: [number, number, number]): THREE.Color {
	return new THREE.Color().setRGB(c[0], c[1], c[2]);
}

/** One submaterial; `null` for parts that are not drawn (proxies). */
export async function buildMaterial(s: SubMaterialInfo): Promise<THREE.Material | null> {
	if (NO_DRAW.has(s.shader) || s.name.startsWith('$')) return null;
	if (s.shader.startsWith('LayerBlend')) return layerBlend(s);
	return illum(s);
}

function glossPatch(m: THREE.MeshStandardMaterial, glossMul: number, hasMap: boolean) {
	m.onBeforeCompile = (shader) => {
		shader.uniforms.uGlossMul = { value: glossMul };
		shader.fragmentShader = shader.fragmentShader
			.replace('#include <common>', '#include <common>\nuniform float uGlossMul;')
			.replace(
				'#include <roughnessmap_fragment>',
				hasMap
					? 'float roughnessFactor = 1.0 - texture2D( roughnessMap, vRoughnessMapUv ).g * uGlossMul;'
					: 'float roughnessFactor = 1.0 - uGlossMul;'
			);
	};
}

async function illum(s: SubMaterialInfo): Promise<THREE.Material> {
	const metal = metalness(s.specular);
	const m = new THREE.MeshStandardMaterial({
		color: color(s.diffuse).lerp(color(s.specular), metal),
		metalness: metal,
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
	glossPatch(m, s.shininess / 255, !!m.roughnessMap);
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
	metal: number;
	glossMul: number;
	tiling: number;
}

async function resolveLayer(l: LayerInfo | undefined): Promise<ResolvedLayer> {
	const fallback: ResolvedLayer = {
		diff: null,
		nrm: null,
		gloss: null,
		color: new THREE.Color(0.5, 0.5, 0.5),
		metal: 0,
		glossMul: 0.5,
		tiling: 1
	};
	if (!l) return fallback;
	const mtl = await loadMaterial(l.path);
	const sub = mtl?.submaterials.find((s) => s.name === l.submaterial) ?? mtl?.submaterials[0];
	if (!sub) return { ...fallback, color: color(l.tint) };
	const metal = metalness(sub.specular);
	const diffSlot = slot(sub, 'TexSlot1');
	const nrmSlot = slot(sub, 'TexSlot2');
	return {
		diff: diffSlot ? await fetchTexture(diffSlot.file, 'color') : null,
		nrm: nrmSlot ? await fetchTexture(nrmSlot.file, 'normal') : null,
		gloss: nrmSlot && hasGloss(nrmSlot.file) ? await fetchTexture(nrmSlot.file, 'gloss') : null,
		color: (metal > 0.5 ? color(sub.specular) : color(sub.diffuse)).multiply(color(l.tint)),
		metal,
		glossMul: (sub.shininess / 255) * l.gloss_mult,
		tiling: l.uv_tiling || 1
	};
}

const WHITE = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
WHITE.needsUpdate = true;
const FLAT = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
FLAT.needsUpdate = true;
const BLACK = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
BLACK.needsUpdate = true;

async function layerBlend(s: SubMaterialInfo): Promise<THREE.Material> {
	const base = await resolveLayer(s.layers.find((l) => l.name.startsWith('BaseLayer')) ?? s.layers[0]);
	const wear = await resolveLayer(s.layers.find((l) => l.name.startsWith('WearLayer')));
	const wearSlot = slot(s, 'TexSlot11');
	const wearMask = wearSlot ? await fetchTexture(wearSlot.file, 'color') : null;
	const macroSlot = [slot(s, 'TexSlot3'), slot(s, 'TexSlot2')].find((t) => t && isNormal(t.file));
	const macro = macroSlot ? await fetchTexture(macroSlot.file, 'normal') : null;

	const m = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 1, metalness: 0 });
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
			uMacroNrm: { value: macro ?? FLAT },
			uBaseColor: { value: base.color },
			uWearColor: { value: wear.color },
			uBaseTile: { value: base.tiling },
			uWearTile: { value: wear.tiling },
			uBaseGlossMul: { value: base.glossMul },
			uWearGlossMul: { value: wear.glossMul },
			uBaseMetal: { value: base.metal },
			uWearMetal: { value: wear.metal }
		});
		shader.fragmentShader = shader.fragmentShader
			.replace(
				'#include <common>',
				`#include <common>
uniform sampler2D uBaseDiff, uWearDiff, uBaseNrm, uWearNrm, uBaseGloss, uWearGloss, uWearMask, uMacroNrm;
uniform vec3 uBaseColor, uWearColor;
uniform float uBaseTile, uWearTile, uBaseGlossMul, uWearGlossMul, uBaseMetal, uWearMetal;
float wearF;`
			)
			.replace(
				'#include <map_fragment>',
				`wearF = clamp( texture2D( uWearMask, vMapUv ).r, 0.0, 1.0 );
vec3 cBase = texture2D( uBaseDiff, vMapUv * uBaseTile ).rgb * uBaseColor;
vec3 cWear = texture2D( uWearDiff, vMapUv * uWearTile ).rgb * uWearColor;
diffuseColor.rgb *= mix( cBase, cWear, wearF );`
			)
			.replace(
				'#include <roughnessmap_fragment>',
				`float roughnessFactor = 1.0 - mix(
	texture2D( uBaseGloss, vMapUv * uBaseTile ).g * uBaseGlossMul,
	texture2D( uWearGloss, vMapUv * uWearTile ).g * uWearGlossMul,
	wearF );`
			)
			.replace('#include <metalnessmap_fragment>', 'float metalnessFactor = mix( uBaseMetal, uWearMetal, wearF );')
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
