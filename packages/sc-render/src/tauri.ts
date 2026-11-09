// The Tauri plugin (`tauri-plugin-sc-render`) as a `RenderSource`:
// descriptions over its commands, heavy bytes over its `scr` URI scheme.
import { invoke } from '@tauri-apps/api/core';
import type { DecodeKind, RenderSource, TextureData, VolumeData } from './library';
import type { BoneInfo, HoloInfo, HoloRequest, MaterialInfo, ModelInfo, ScenePart } from './types';

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

interface SharedBufferEvent {
	additionalData?: { scRender?: string };
	getBuffer(): ArrayBuffer;
}
interface WebView2 {
	addEventListener(type: 'sharedbufferreceived', listener: (e: SharedBufferEvent) => void): void;
	releaseBuffer(buffer: ArrayBuffer): void;
}

/** WebView2, where big buffers skip the scheme's per-byte cost. */
const webview = (globalThis as { chrome?: { webview?: WebView2 } }).chrome?.webview;
const arrived = new Map<string, ArrayBuffer>();
const awaited = new Map<string, (buffer: ArrayBuffer) => void>();
webview?.addEventListener('sharedbufferreceived', (e) => {
	const id = e.additionalData?.scRender;
	if (!id) return;
	const buffer = e.getBuffer();
	const resolve = awaited.get(id);
	if (resolve) {
		awaited.delete(id);
		resolve(buffer);
	} else arrived.set(id, buffer);
});

/** The shared buffer posted as `id`, however the event and the reply race. */
function sharedBuffer(id: string): Promise<ArrayBuffer> {
	const buffer = arrived.get(id);
	if (buffer) {
		arrived.delete(id);
		return Promise.resolve(buffer);
	}
	return new Promise((resolve) => awaited.set(id, resolve));
}

let source: RenderSource | undefined;

/** The plugin's source; one per app, so every cell shares its caches. */
export function tauriSource(): RenderSource {
	source ??= {
		async model(path, options) {
			const level = options?.level ?? 0;
			const info = await invoke<ModelInfo>('plugin:sc-render|load_model', { path, level });
			return { info, buffer: await fetchBytes(url('model', { path: level ? `${path}@${level}` : path })) };
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
		async holo(root: ScenePart, request?: Partial<HoloRequest>) {
			const info = await invoke<HoloInfo>('plugin:sc-render|load_holo', { root, request: request ?? null, shared: !!webview });
			if (!info.shared) return { info, buffer: await fetchBytes(url('model', { path: info.path })) };
			const buffer = await sharedBuffer(info.path);
			return { info, buffer, release: () => webview!.releaseBuffer(buffer) };
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
