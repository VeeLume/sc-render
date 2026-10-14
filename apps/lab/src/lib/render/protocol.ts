// Heavy bytes come over the `scr` URI scheme (see src-tauri/src/render.rs);
// descriptions come over typed commands.
import * as THREE from 'three';

const base = navigator.userAgent.includes('Windows') ? 'http://scr.localhost' : 'scr://localhost';

export type TextureKind = 'color' | 'normal' | 'gloss';

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

/** A decoded texture as a repeating, mipmapped DataTexture; `null` if missing. */
export function fetchTexture(path: string, kind: TextureKind, max = 1024): Promise<THREE.DataTexture | null> {
	const key = `${kind}|${max}|${path.toLowerCase()}`;
	let hit = textureCache.get(key);
	if (!hit) {
		hit = fetchBytes(url('texture', { path, kind, max }))
			.then((buf) => {
				const view = new DataView(buf);
				const w = view.getUint32(0, true);
				const h = view.getUint32(4, true);
				const tex = new THREE.DataTexture(new Uint8Array(buf, 8), w, h, THREE.RGBAFormat);
				tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
				tex.magFilter = THREE.LinearFilter;
				tex.minFilter = THREE.LinearMipmapLinearFilter;
				tex.generateMipmaps = true;
				tex.anisotropy = 8;
				tex.colorSpace = kind === 'color' ? THREE.SRGBColorSpace : THREE.NoColorSpace;
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
