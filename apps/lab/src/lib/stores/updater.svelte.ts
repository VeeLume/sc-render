// Desktop auto-update. `check()` is fire-and-forget from the root layout; the
// banner renders whenever `available` is set.
//
// Mobile has no Tauri updater (Android ships its own APK flow) — `check()`
// throws there and is swallowed, which is the intended no-op.
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

class Updater {
	available = $state<Update | null>(null);
	installing = $state(false);
	progress = $state(0);
	error = $state<string | null>(null);

	async check() {
		try {
			const update = await check();
			if (update) this.available = update;
		} catch (e) {
			this.error = String(e);
		}
	}

	async installAndRelaunch() {
		const update = this.available;
		if (!update || this.installing) return;
		this.installing = true;
		this.error = null;
		try {
			let downloaded = 0;
			let total = 0;
			await update.downloadAndInstall((event) => {
				if (event.event === 'Started') total = event.data.contentLength ?? 0;
				else if (event.event === 'Progress') {
					downloaded += event.data.chunkLength;
					this.progress = total ? downloaded / total : 0;
				}
			});
			await relaunch();
		} catch (e) {
			this.error = String(e);
			this.installing = false;
		}
	}

	dismiss() {
		this.available = null;
	}
}

export const updater = new Updater();
