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
//   LayerBlend_V2  Up to four MatLayers BaseLayer1-4 (each a `Layer` .mtl with
//                  TexSlot1 diffuse, TexSlot2 ddna), with BLEND_MAP chosen per
//                  pixel by TexSlot12: near-exclusive masks over a base of
//                  layer 3, R mixing in layer 1, G layer 2, B layer 4 (with
//                  fewer than three layers the base is layer 1). Fitted to
//                  in-game shots of the Big Bite helmet: shell B -> olive 4,
//                  camo and face mask black -> dark 3, stripe and mask
//                  markers R -> red 1, teeth G -> white 2.
//                  TexSlot3 macro normal; layer textures tiled by the layer's
//                  UVTiling. A layer with PaletteTint 1-3 takes the tint
//                  palette's entry A-C: colour multiplied in, its spec as F0
//                  (dielectric layers only), its glossiness pulling gloss
//                  towards 1; the submaterial's own Shininess / 255 scales the
//                  result (255 on hard surfaces, 10 on cloth: matte). With the
//                  DECALS feature, TexSlot9 is a decal
//                  sheet placed by the vertex colour (24 bits -> two 12-bit
//                  coordinates, StarBreaker addon's layer_blend_decals);
//                  DiffuseDecalGloss / DiffuseDecalSpec are the decal's own
//                  surface. Wear layers are not drawn (items show pristine;
//                  the TexSlot11 mask is white where paint is intact) and
//                  HEIGHT_MAP / HAL (TexSlot13) details are not modelled yet.
import * as THREE from 'three';
import { commands, type LayerInfo, type MaterialInfo, type SubMaterialInfo } from '$lib/bindings';
import { fetchTexture } from './protocol';
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
 * The environment only reflects: keep its specular, drop its diffuse
 * irradiance (matte items stay as calibrated; see CellOptions.env).
 */
const IBL_LINE = '#include <lights_fragment_maps>';
const SPECULAR_ONLY_IBL = `${IBL_LINE}
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
			)
			.replace(IBL_LINE, SPECULAR_ONLY_IBL);
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
		shader.fragmentShader = shader.fragmentShader
			.replace('#include <common>', '#include <common>\nuniform vec3 uF0;')
			.replace(F0_LINE, 'material.specularColor = uF0;')
			.replace(IBL_LINE, SPECULAR_ONLY_IBL);
	};
	m.userData.noShadow = true;
	return m;
}

interface ResolvedLayer {
	diff: THREE.Texture | null;
	/** Normal X, Y in R, G; gloss in B (`normal_gloss`). */
	normalGloss: THREE.Texture | null;
	color: THREE.Color;
	f0: THREE.Color;
	glossMul: number;
	/** Palette glossiness 0..1; pulls the gloss towards 1 (0 = no palette). */
	paletteGloss: number;
	tiling: number;
}

async function resolveLayer(l: LayerInfo | undefined, palette?: Palette): Promise<ResolvedLayer> {
	const fallback: ResolvedLayer = {
		diff: null,
		normalGloss: null,
		color: new THREE.Color(0.5, 0.5, 0.5),
		f0: new THREE.Color(0.04, 0.04, 0.04),
		glossMul: 0.5,
		paletteGloss: 0,
		tiling: 1
	};
	if (!l) return fallback;
	const mtl = await loadMaterial(l.path);
	const sub = mtl?.submaterials.find((s) => s.name === l.submaterial) ?? mtl?.submaterials[0];
	if (!sub) return { ...fallback, color: color(l.tint) };
	const diffSlot = slot(sub, 'TexSlot1');
	const nrmSlot = slot(sub, 'TexSlot2');
	const entry = l.palette_tint >= 1 && l.palette_tint <= 3 ? palette?.[l.palette_tint - 1] : undefined;
	// A conductor (dark diffuse, bright specular; StarBreaker's layer_metallic)
	// keeps its own F0; only a dielectric takes the palette's spec as F0.
	const metal = Math.max(...sub.diffuse) < 0.1 && Math.max(...sub.specular) > 0.25;
	return {
		diff: diffSlot ? await fetchTexture(diffSlot.file, 'color') : null,
		normalGloss: nrmSlot ? await fetchTexture(nrmSlot.file, 'normal_gloss') : null,
		color: color(sub.diffuse)
			.multiply(color(l.tint))
			.multiply(entry ? srgb(entry.tint) : new THREE.Color(1, 1, 1)),
		f0: entry && !metal ? srgb(entry.spec) : color(sub.specular),
		glossMul: (sub.shininess / 255) * l.gloss_mult,
		// StarBreaker's layer surface: smoothness = mix(texture gloss, 1, palette glossiness).
		paletteGloss: entry ? entry.gloss / 255 : 0,
		tiling: l.uv_tiling || 1
	};
}

const WHITE = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
WHITE.needsUpdate = true;
const FLAT = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
/** A flat `normal_gloss` texel: normal (0, 0), gloss 1. */
const FLAT_GLOSS = new THREE.DataTexture(new Uint8Array([128, 128, 255, 255]), 1, 1);
FLAT_GLOSS.needsUpdate = true;
FLAT.needsUpdate = true;
const BLACK = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
BLACK.needsUpdate = true;

async function layerBlend(s: SubMaterialInfo, palette?: Palette): Promise<THREE.Material> {
	const bases = s.layers.filter((l) => l.name.startsWith('BaseLayer')).slice(0, 4);
	const layers = await Promise.all((bases.length ? bases : s.layers.slice(0, 1)).map((l) => resolveLayer(l, palette)));
	if (!layers.length) layers.push(await resolveLayer(undefined, palette));
	const blendSlot = s.features.includes('BLEND_MAP') ? slot(s, 'TexSlot12') : undefined;
	const blend = blendSlot && layers.length > 1 ? await fetchTexture(blendSlot.file, 'mask') : null;
	const decalSlot =
		s.features.includes('DECALS') && !s.features.includes('WEAR_DIRT_AO_VERTEX') ? slot(s, 'TexSlot9') : undefined;
	const decal = decalSlot ? await fetchTexture(decalSlot.file, 'color') : null;
	if (decal) decal.wrapS = decal.wrapT = THREE.ClampToEdgeWrapping;
	const param = (k: string) => s.params.find(([key]) => key === k)?.[1];
	const decalGloss = Number(param('DiffuseDecalGloss') ?? 10) / 255;
	const decalSpec = (param('DiffuseDecalSpec') ?? '0.04,0.04,0.04').split(',').map(Number) as [number, number, number];
	const macroSlot = [slot(s, 'TexSlot3'), slot(s, 'TexSlot2')].find((t) => t && isNormal(t.file));
	const macro = macroSlot ? await fetchTexture(macroSlot.file, 'normal') : null;
	// Slots: the base drawn where the blend map is black, then the layers the
	// map's R, G, B channels mix in. With three or more layers the base is
	// layer 3 and R, G, B pick layers 1, 2, 4; with fewer it is layer 1.
	const base = layers.length >= 3 ? layers[2] : layers[0];
	const byChannel = layers.length >= 3 ? [layers[0], layers[1], layers[3]] : [undefined, layers[1], undefined];
	const slot4 = [base, ...byChannel.map((l) => l ?? base)];
	const pad = (i: number) => slot4[i];

	const m = new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 1, metalness: 0 });
	// Layer 1 rides on three's own samplers (map, normalMap); they also turn
	// on the UV varying and the tangent-space normal path. WebView2 allows 16
	// samplers per shader, and shadows and the environment take three.
	m.map = base.diff ?? WHITE;
	m.normalMap = base.normalGloss ?? FLAT_GLOSS;
	m.normalScale.set(1, -1);
	m.onBeforeCompile = (shader) => {
		Object.assign(shader.uniforms, {
			uL1Diff: { value: pad(1).diff ?? WHITE },
			uL2Diff: { value: pad(2).diff ?? WHITE },
			uL3Diff: { value: pad(3).diff ?? WHITE },
			uL1NG: { value: pad(1).normalGloss ?? FLAT_GLOSS },
			uL2NG: { value: pad(2).normalGloss ?? FLAT_GLOSS },
			uL3NG: { value: pad(3).normalGloss ?? FLAT_GLOSS },
			uBlend: { value: blend ?? BLACK },
			// Channel weights (R, G, B); zero where that channel's layer is missing.
			uBlendWeights: { value: new THREE.Vector3(...byChannel.map((l) => (blend && l ? 1 : 0))) },
			uBlendTile: { value: Number(param('BlendMapTiling') ?? 1) || 1 },
			// Per-layer values as separate uniforms (no uniform arrays).
			...Object.fromEntries(
				[0, 1, 2, 3].flatMap((i) => [
					[`uColor${i}`, { value: pad(i).color }],
					[`uF0${i}`, { value: pad(i).f0 }],
					[`uGlossMul${i}`, { value: pad(i).glossMul }],
					[`uPalGloss${i}`, { value: pad(i).paletteGloss }],
					[`uTile${i}`, { value: pad(i).tiling }]
				])
			),
			uParentGloss: { value: s.shininess / 255 },
			uMacroNrm: { value: macro ?? FLAT },
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
uniform sampler2D uL1Diff, uL2Diff, uL3Diff, uL1NG, uL2NG, uL3NG, uBlend, uMacroNrm, uDecal;
uniform vec3 uBlendWeights, uDecalSpec;
uniform vec3 uColor0, uColor1, uColor2, uColor3, uF00, uF01, uF02, uF03;
uniform float uGlossMul0, uGlossMul1, uGlossMul2, uGlossMul3;
uniform float uPalGloss0, uPalGloss1, uPalGloss2, uPalGloss3;
uniform float uTile0, uTile1, uTile2, uTile3;
uniform float uBlendTile, uHasDecal, uDecalGloss, uParentGloss;
varying vec2 vDecalUv;
vec3 layerF0;
float layerGloss;
vec2 layerNormal;
float decalA;
vec3 blendLayer( vec3 c, float w, vec4 diff, vec4 ng, vec3 color, vec3 f0, float glossMul, float palGloss ) {
	layerGloss = mix( layerGloss, mix( ng.b * glossMul, 1.0, palGloss ), w );
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
layerGloss = mix( ng0.b * uGlossMul0, 1.0, uPalGloss0 );
layerF0 = uF00;
layerNormal = ng0.rg;
vec3 bw = texture2D( uBlend, vMapUv * uBlendTile ).rgb * uBlendWeights;
vec2 uv1 = vMapUv * uTile1;
vec2 uv2 = vMapUv * uTile2;
vec2 uv3 = vMapUv * uTile3;
layerColor = blendLayer( layerColor, bw.r, texture2D( uL1Diff, uv1 ), texture2D( uL1NG, uv1 ), uColor1, uF01, uGlossMul1, uPalGloss1 );
layerColor = blendLayer( layerColor, bw.g, texture2D( uL2Diff, uv2 ), texture2D( uL2NG, uv2 ), uColor2, uF02, uGlossMul2, uPalGloss2 );
layerColor = blendLayer( layerColor, bw.b, texture2D( uL3Diff, uv3 ), texture2D( uL3NG, uv3 ), uColor3, uF03, uGlossMul3, uPalGloss3 );
diffuseColor.rgb *= layerColor;
float inDiffuse = step( 2.0, vDecalUv.x ) * ( 1.0 - step( 3.0, vDecalUv.x ) );
float inStencil = step( 2.0, vDecalUv.y ) * ( 1.0 - step( 3.0, vDecalUv.y ) );
vec4 decalTex = texture2D( uDecal, vec2( vDecalUv.x - 2.0, vDecalUv.y ) );
decalA = uHasDecal * decalTex.a * inDiffuse * ( 1.0 - inStencil );
diffuseColor.rgb = mix( diffuseColor.rgb, decalTex.rgb, decalA );`
			)
			.replace(
				'#include <roughnessmap_fragment>',
				`float roughnessFactor = mix( 1.0 - layerGloss * uParentGloss, 1.0 - uDecalGloss, decalA );`
			)
			.replace(F0_LINE, 'material.specularColor = mix( layerF0, uDecalSpec, decalA );')
			.replace(IBL_LINE, SPECULAR_ONLY_IBL)
			.replace(
				'vec3 mapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;',
				`vec2 nxy = layerNormal * 2.0 - 1.0;
vec3 nLayer = vec3( nxy, sqrt( max( 1.0 - dot( nxy, nxy ), 0.0 ) ) );
vec3 nMacro = texture2D( uMacroNrm, vMapUv ).xyz * 2.0 - 1.0;
vec3 mapN = normalize( vec3( nMacro.xy + nLayer.xy, nMacro.z * nLayer.z ) );`
			);
	};
	return m;
}
