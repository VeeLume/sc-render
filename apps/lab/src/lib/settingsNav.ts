// The settings categories, in display order: adding a setting is one entry
// here plus one small page under routes/settings/<id>/.
import type { SettingsCategory } from '@veelume/ui';
import { Palette } from 'lucide-svelte';

export const settingsCategories: SettingsCategory[] = [
	{
		id: 'appearance',
		label: 'Appearance',
		description: 'Theme and density',
		icon: Palette,
		path: '/settings/appearance'
	}
];
