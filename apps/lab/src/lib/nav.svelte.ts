// Navigation model. Components read `nav.groups` / `nav.bottomItems` and never
// build routes themselves, so adding a destination is one edit here.
import type { ComponentType } from 'svelte';
import { House, Settings } from 'lucide-svelte';

// lucide-svelte still ships Svelte-4-style class components, so this is the
// legacy `ComponentType` rather than the runes-era `Component`.
export type IconComponent = ComponentType;

export interface NavItem {
	label: string;
	icon: IconComponent;
	path: string;
}

export interface NavGroup {
	/** Rendered as a section heading in the rail when labels are shown. */
	label?: string;
	items: NavItem[];
}

/** Destinations that exist regardless of configuration. */
const coreItems: NavItem[] = [{ label: 'Home', icon: House, path: '/home' }];

const settingsItem: NavItem = {
	label: 'Settings',
	icon: Settings,
	path: '/settings'
};

class Nav {
	get groups(): NavGroup[] {
		const groups: NavGroup[] = [{ items: coreItems }];
		return groups;
	}

	get items(): NavItem[] {
		return this.groups.flatMap((g) => g.items);
	}

	/** Bottom bar holds at most five slots; settings always takes the last one. */
	get bottomItems(): NavItem[] {
		return [...this.items.slice(0, 4), settingsItem];
	}
}

export const nav = new Nav();

/**
 * Longest-prefix match, so `/items/42` still lights up `/items` and a nested
 * route wins over its parent.
 */
export function activeNavPath(pathname: string, paths: string[]): string | null {
	let best: string | null = null;
	for (const p of paths) {
		if (pathname === p || pathname.startsWith(p + '/')) {
			if (!best || p.length > best.length) best = p;
		}
	}
	return best;
}
