// Heavy bytes come over the `scr` URI scheme (see src-tauri/src/render.rs);
// descriptions come over typed commands.
import * as THREE from 'three';

const base = navigator.userAgent.includes('Windows') ? 'http://scr.localhost' : 'scr://localhost';

/**
 * `color` and `mask` are delivered as stored. `color` (diffuse, decal
 * sheets) is always sampled as sRGB: not every diffuse map carries the
 * engine's sRGB flag, and reading unflagged ones as linear washes blacks
 * out to grey. `mask` follows the flag: blend maps are sRGB, HueAoLum maps
 * linear.
 */
export type TextureKind = 'color' | 'mask' | 'normal' | 'gloss' | 'normal_gloss';

function url(route: string, params: Record<string, string | number>): string {
	const q = Object.entries(params)
		.map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
		.join('&');
	return `${base}/${route}?${q}`;
}

async function fetchBytes(u: string): Promise<ArrayBuffer> {
	const r = await fetch(u);
	if (!r.ok) throw new Error(`${u}: ${await r.text()}`);
	return r.arrayBuffer();
}

export function fetchModelBuffer(path: string): Promise<ArrayBuffer> {
	return fetchBytes(url('model', { path }));
}

const textureCache = new Map<string, Promise<THREE.DataTexture | null>>();

let hueLut: Promise<THREE.Data3DTexture | null> | undefined;

/** The LayerBlend hue-shift LUT as a linear, clamped 3D texture; `null` if missing. */
export function fetchHueLut(): Promise<THREE.Data3DTexture | null> {
	hueLut ??= fetchBytes(`${base}/hue_lut`)
		.then((buf) => {
			const view = new DataView(buf);
			const [w, h, d] = [0, 4, 8].map((o) => view.getUint32(o, true));
			const tex = new THREE.Data3DTexture(new Uint8Array(buf, 12), w, h, d);
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
	return hueLut;
}

/** A decoded texture with its stored mip chain, repeating; `null` if missing. */
export function fetchTexture(path: string, kind: TextureKind, max = 1024): Promise<THREE.DataTexture | null> {
	const key = `${kind}|${max}|${path.toLowerCase()}`;
	let hit = textureCache.get(key);
	if (!hit) {
		hit = fetchBytes(url('texture', { path, kind, max }))
			.then((buf) => {
				const view = new DataView(buf);
				const w = view.getUint32(0, true);
				const h = view.getUint32(4, true);
				const srgb = view.getUint32(8, true) !== 0;
				const levels = view.getUint32(12, true);
				// The file's own mips (they carry the engine's gloss filtering).
				const mipmaps: { data: Uint8Array; width: number; height: number }[] = [];
				let offset = 16;
				for (let i = 0, mw = w, mh = h; i < levels; i++, mw = Math.max(1, mw >> 1), mh = Math.max(1, mh >> 1)) {
					mipmaps.push({ data: new Uint8Array(buf, offset, mw * mh * 4), width: mw, height: mh });
					offset += mw * mh * 4;
				}
				const tex = new THREE.DataTexture(mipmaps[0].data, w, h, THREE.RGBAFormat);
				tex.mipmaps = mipmaps as unknown as THREE.DataTexture['mipmaps'];
				tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
				tex.magFilter = THREE.LinearFilter;
				tex.minFilter = THREE.LinearMipmapLinearFilter;
				tex.generateMipmaps = false;
				tex.anisotropy = 8;
				tex.colorSpace = kind === 'color' || srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
				tex.needsUpdate = true;
				return tex;
			})
			.catch((e) => {
				console.warn('texture', path, e);
				return null;
			});
		textureCache.set(key, hit);
	}
	return hit;
}
