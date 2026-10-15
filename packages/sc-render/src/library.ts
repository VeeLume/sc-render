// What the renderer reads, and the caches in front of it. The host supplies a
// `RenderSource` (the Tauri plugin's is `tauriSource` in `./tauri`); a
// `Library` turns its answers into three.js objects once and shares them
// between every cell drawing from the same source.
import * as THREE from 'three';
import type { BoneInfo, MaterialInfo, ModelInfo } from './types';

/** Decoded pixels: RGBA8 levels, largest first, each half the one before. */
export interface TextureData {
	width: number;
	height: number;
	/** The engine reads the texture as sRGB. */
	srgb: boolean;
	levels: Uint8Array[];
}

/** A 3D texture, RGBA8, `width` x `height` x `depth`. */
export interface VolumeData {
	width: number;
	height: number;
	depth: number;
	data: Uint8Array;
}

/**
 * How a texture is decoded: `color` as stored; `normal` with Z rebuilt;
 * `gloss` a `ddna` map's gloss in R, G and B; `normal_gloss` normal X, Y in
 * R, G and gloss in B.
 */
export type DecodeKind = 'color' | 'normal' | 'gloss' | 'normal_gloss';

/**
 * Where game assets come from, by p4k path. Failures reject; the renderer
 * logs them and draws what it has.
 */
export interface RenderSource {
	/** A geometry file packed for upload (layout: `ModelInfo`). */
	model(path: string): Promise<{ info: ModelInfo; buffer: ArrayBuffer }>;
	/** A `.mtl` file (extension optional). */
	material(path: string): Promise<MaterialInfo>;
	/** A `.chr` or `.skin`'s bones, model space. */
	skeleton(path: string): Promise<BoneInfo[]>;
	/** A texture, its largest level no wider or taller than `max`. */
	texture(path: string, kind: DecodeKind, max: number): Promise<TextureData>;
	/** The LayerBlend hue-shift lookup table (8 x 8 x 16). */
	hueLut(): Promise<VolumeData>;
}

/**
 * `color` and `mask` are delivered as stored. `color` (diffuse, decal
 * sheets) is always sampled as sRGB: not every diffuse map carries the
 * engine's sRGB flag, and reading unflagged ones as linear washes blacks
 * out to grey. `mask` follows the flag: blend maps are sRGB, HueAoLum maps
 * linear.
 */
export type TextureKind = DecodeKind | 'mask';

const libraries = new WeakMap<RenderSource, Library>();

/** A source's assets, loaded once. */
export class Library {
	private readonly materials = new Map<string, Promise<MaterialInfo | null>>();
	private readonly skeletons = new Map<string, Promise<BoneInfo[]>>();
	private readonly textures = new Map<string, Promise<THREE.DataTexture | null>>();
	private lut: Promise<THREE.Data3DTexture | null> | undefined;

	private constructor(readonly source: RenderSource) {}

	/** The library of `source`, shared by every cell that draws from it. */
	static of(source: RenderSource): Library {
		let lib = libraries.get(source);
		if (!lib) {
			lib = new Library(source);
			libraries.set(source, lib);
		}
		return lib;
	}

	/** Not cached: buffers are large, and a cell keeps what it built from them. */
	model(path: string): Promise<{ info: ModelInfo; buffer: ArrayBuffer }> {
		return this.source.model(path);
	}

	/** `null` when the file is missing or unreadable. */
	material(path: string): Promise<MaterialInfo | null> {
		return cached(this.materials, path.toLowerCase(), () =>
			this.source.material(path).catch((e) => {
				console.warn('material', path, e);
				return null;
			})
		);
	}

	/** Empty when the file is missing or unreadable. */
	skeleton(path: string): Promise<BoneInfo[]> {
		return cached(this.skeletons, path.toLowerCase(), () =>
			this.source.skeleton(path).catch((e) => {
				console.warn('skeleton', path, e);
				return [];
			})
		);
	}

	/** A decoded texture with its stored mip chain, repeating; `null` if missing. */
	texture(path: string, kind: TextureKind, max = 1024): Promise<THREE.DataTexture | null> {
		return cached(this.textures, `${kind}|${max}|${path.toLowerCase()}`, () =>
			this.source
				.texture(path, kind === 'mask' ? 'color' : kind, max)
				.then((t) => {
					// The file's own mips (they carry the engine's gloss filtering).
					const mipmaps = t.levels.map((data, i) => ({
						data,
						width: Math.max(1, t.width >> i),
						height: Math.max(1, t.height >> i)
					}));
					const tex = new THREE.DataTexture(mipmaps[0].data, t.width, t.height, THREE.RGBAFormat);
					tex.mipmaps = mipmaps as unknown as THREE.DataTexture['mipmaps'];
					tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
					tex.magFilter = THREE.LinearFilter;
					tex.minFilter = THREE.LinearMipmapLinearFilter;
					tex.generateMipmaps = false;
					tex.anisotropy = 8;
					tex.colorSpace = kind === 'color' || t.srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
					tex.needsUpdate = true;
					return tex;
				})
				.catch((e) => {
					console.warn('texture', path, e);
					return null;
				})
		);
	}

	/** The LayerBlend hue-shift LUT as a linear, clamped 3D texture; `null` if missing. */
	hueLut(): Promise<THREE.Data3DTexture | null> {
		this.lut ??= this.source
			.hueLut()
			.then((v) => {
				const tex = new THREE.Data3DTexture(v.data, v.width, v.height, v.depth);
				tex.format = THREE.RGBAFormat;
				tex.minFilter = tex.magFilter = THREE.LinearFilter;
				tex.wrapS = tex.wrapT = tex.wrapR = THREE.ClampToEdgeWrapping;
				tex.unpackAlignment = 1;
				tex.needsUpdate = true;
				return tex;
			})
			.catch((e) => {
				console.warn('hue lut', e);
				return null;
			});
		return this.lut;
	}
}

function cached<T>(map: Map<string, Promise<T>>, key: string, load: () => Promise<T>): Promise<T> {
	let hit = map.get(key);
	if (!hit) {
		hit = load();
		map.set(key, hit);
	}
	return hit;
}
