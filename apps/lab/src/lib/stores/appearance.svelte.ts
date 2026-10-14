// Appearance preferences (colour scheme + density), persisted in localStorage
// and applied to <html>: `data-density` drives the density CSS variables and
// the `dark` class drives the dark-theme tokens (see app.css / theme.css).
// init() runs once from the root layout, before content first paints.
//
// Deliberately NOT part of the Rust settings snapshot: this is per-device
// display preference, it must apply before any IPC round-trip completes, and
// the web build (if one ever happens) has no backend to ask.

export type Density = 'comfortable' | 'compact';
export type Theme = 'light' | 'dark' | 'system';

const DENSITY_KEY = 'sc-item-render:density';
const THEME_KEY = 'sc-item-render:theme';

class Appearance {
	density = $state<Density>('comfortable');
	theme = $state<Theme>('system');
	private mql: MediaQueryList | null = null;
	private started = false;

	init() {
		if (this.started || typeof window === 'undefined') return;
		this.started = true;

		const d = localStorage.getItem(DENSITY_KEY);
		if (d === 'comfortable' || d === 'compact') this.density = d;
		const t = localStorage.getItem(THEME_KEY);
		if (t === 'light' || t === 'dark' || t === 'system') this.theme = t;

		this.mql = window.matchMedia('(prefers-color-scheme: dark)');
		this.mql.addEventListener('change', () => this.applyTheme());
		this.applyDensity();
		this.applyTheme();
	}

	setDensity(d: Density) {
		this.density = d;
		localStorage.setItem(DENSITY_KEY, d);
		this.applyDensity();
	}

	setTheme(t: Theme) {
		this.theme = t;
		localStorage.setItem(THEME_KEY, t);
		this.applyTheme();
	}

	/** Whether the *effective* scheme is dark (resolves 'system'). */
	get isDark() {
		return this.theme === 'dark' || (this.theme === 'system' && !!this.mql?.matches);
	}

	private applyDensity() {
		const root = document.documentElement;
		if (this.density === 'compact') root.dataset.density = 'compact';
		else delete root.dataset.density;
	}

	private applyTheme() {
		const dark = this.isDark;
		document.documentElement.classList.toggle('dark', dark);
	}
}

export const appearance = new Appearance();
