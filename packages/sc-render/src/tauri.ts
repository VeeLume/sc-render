// The Tauri plugin (`tauri-plugin-sc-render`) as a `RenderSource`:
// descriptions over its commands, heavy bytes over its `scr` URI scheme.
import { invoke } from '@tauri-apps/api/core';
import type { DecodeKind, RenderSource, TextureData, VolumeData } from './library';
import type { BoneInfo, MaterialInfo, ModelInfo } from './types';

const base = navigator.userAgent.includes('Windows') ? 'http://scr.localhost' : 'scr://localhost';

function url(route: string, params: Record<string, string | number> = {}): string {
	const q = Object.entries(params)
		.map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
		.join('&');
	return q ? `${base}/${route}?${q}` : `${base}/${route}`;
}

async function fetchBytes(u: string): Promise<ArrayBuffer> {
	const r = await fetch(u);
	if (!r.ok) throw new Error(`${u}: ${await r.text()}`);
	return r.arrayBuffer();
}

let source: RenderSource | undefined;

/** The plugin's source; one per app, so every cell shares its caches. */
export function tauriSource(): RenderSource {
	source ??= {
		async model(path) {
			const info = await invoke<ModelInfo>('plugin:sc-render|load_model', { path });
			return { info, buffer: await fetchBytes(url('model', { path })) };
		},
		material: (path) => invoke<MaterialInfo>('plugin:sc-render|load_material', { path }),
		skeleton: (path) => invoke<BoneInfo[]>('plugin:sc-render|load_skeleton', { path }),
		async texture(path: string, kind: DecodeKind, max: number): Promise<TextureData> {
			const buf = await fetchBytes(url('texture', { path, kind, max }));
			const view = new DataView(buf);
			const [width, height, srgb, count] = [0, 4, 8, 12].map((o) => view.getUint32(o, true));
			const levels: Uint8Array[] = [];
			let offset = 16;
			for (let i = 0; i < count; i++) {
				const len = Math.max(1, width >> i) * Math.max(1, height >> i) * 4;
				levels.push(new Uint8Array(buf, offset, len));
				offset += len;
			}
			return { width, height, srgb: srgb !== 0, levels };
		},
		async hueLut(): Promise<VolumeData> {
			const buf = await fetchBytes(url('hue_lut'));
			const view = new DataView(buf);
			const [width, height, depth] = [0, 4, 8].map((o) => view.getUint32(o, true));
			return { width, height, depth, data: new Uint8Array(buf, 12) };
		}
	};
	return source;
}
