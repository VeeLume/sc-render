<script lang="ts">
	import { Settings, Segmented } from '@veelume/ui';
	import { appearance, type Theme, type Density } from '$lib/stores/appearance.svelte';

	const themes: { value: Theme; label: string }[] = [
		{ value: 'light', label: 'Light' },
		{ value: 'dark', label: 'Dark' },
		{ value: 'system', label: 'System' }
	];

	const densities: { value: Density; label: string }[] = [
		{ value: 'comfortable', label: 'Comfortable' },
		{ value: 'compact', label: 'Compact' }
	];
</script>

<Settings.Page title="Appearance">
	<Settings.Section>
		<Settings.Row label="Theme" hint="'System' follows the device's setting.">
			<Segmented options={themes} value={appearance.theme} onchange={(v) => appearance.setTheme(v as Theme)} />
		</Settings.Row>
		<Settings.Row label="Density" hint="'Comfortable' is larger and finger-friendly; 'Compact' shows more at once.">
			<Segmented
				options={densities}
				value={appearance.density}
				onchange={(v) => appearance.setDensity(v as Density)}
			/>
		</Settings.Row>
	</Settings.Section>
</Settings.Page>
