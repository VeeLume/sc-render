// The app settings snapshot, mirrored from Rust.
//
// One snapshot, whole-snapshot saves — the same contract as `AppSettings` on
// the Rust side. Per-key setters can come back if the surface ever grows past
// what one settings page edits; until then, `save({ key: value })` merges into
// the current snapshot and writes the whole thing.
import { commands, type AppSettings } from '$lib/bindings';

class SettingsStore {
	current = $state<AppSettings | null>(null);
	loading = $state(true);

	async init() {
		this.current = await commands.settingsGet();
		this.loading = false;
	}

	async save(patch: Partial<AppSettings>) {
		if (!this.current) return;
		const next = { ...this.current, ...patch };
		this.current = next;
		const res = await commands.settingsSave(next);
		if (res.status === 'error') throw new Error(res.error);
	}
}

export const settings = new SettingsStore();
