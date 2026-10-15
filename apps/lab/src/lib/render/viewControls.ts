// Pointer and wheel handling for a rendered cell: left drag turns the item,
// right / middle drag (or Shift + left drag) pans, the wheel zooms towards the
// cursor, a double click restores the whole view.
import type { Cell } from './scene';

export type AxisLock = 'free' | 'turn' | 'tilt';

export const AXIS_LOCKS: { value: AxisLock; label: string; hint: string }[] = [
	{ value: 'free', label: 'Free', hint: 'Drag in any direction' },
	{ value: 'turn', label: 'Turn', hint: 'Only spin about the vertical (horizontal drag)' },
	{ value: 'tilt', label: 'Tilt', hint: 'Only tip towards or away (vertical drag)' }
];

export const VIEW_HINT = 'Drag to turn, right-drag or Shift-drag to pan, wheel to zoom, double-click to reset';

/**
 * Handlers to spread onto the canvas. `cell` and `lock` are read on every
 * event, so they may change after the handlers are made.
 */
export function viewControls(canvas: () => HTMLCanvasElement, cell: () => Cell | null, lock: () => AxisLock) {
	let drag: { id: number; x: number; y: number; pan: boolean } | null = null;
	return {
		onpointerdown(e: PointerEvent) {
			if (e.button > 2) return;
			drag = { id: e.pointerId, x: e.clientX, y: e.clientY, pan: e.button !== 0 || e.shiftKey };
			canvas().setPointerCapture(e.pointerId);
		},
		onpointermove(e: PointerEvent) {
			if (!drag || e.pointerId !== drag.id) return;
			const dx = e.clientX - drag.x;
			const dy = e.clientY - drag.y;
			drag.x = e.clientX;
			drag.y = e.clientY;
			if (drag.pan) {
				cell()?.panBy(dx, dy);
				return;
			}
			const l = lock();
			cell()?.rotate(l === 'tilt' ? 0 : dx, l === 'turn' ? 0 : dy);
		},
		onpointerup(e: PointerEvent) {
			if (drag?.id === e.pointerId) drag = null;
		},
		onpointercancel(e: PointerEvent) {
			if (drag?.id === e.pointerId) drag = null;
		},
		oncontextmenu(e: MouseEvent) {
			e.preventDefault();
		},
		ondblclick() {
			cell()?.resetView();
		}
	};
}

/**
 * Wheel zoom towards the cursor. Attached by hand: Svelte registers `onwheel`
 * as passive, which cannot stop the page from scrolling. Returns the detach.
 */
export function bindWheelZoom(canvas: HTMLCanvasElement, cell: () => Cell | null): () => void {
	const onWheel = (e: WheelEvent) => {
		e.preventDefault();
		cell()?.zoomAt(Math.exp(-e.deltaY * 0.0015), e.offsetX, e.offsetY);
	};
	canvas.addEventListener('wheel', onWheel, { passive: false });
	return () => canvas.removeEventListener('wheel', onWheel);
}
