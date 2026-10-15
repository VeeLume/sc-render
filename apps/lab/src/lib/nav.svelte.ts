// Navigation model. Components read `nav.groups` and never build routes
// themselves, so adding a destination is one edit here. The types are the
// kit's.
import type { NavGroup, NavItem } from '@veelume/ui';
import { Boxes, PersonStanding } from 'lucide-svelte';

/** Destinations that exist regardless of configuration. */
const coreItems: NavItem[] = [
	{ label: 'Items', icon: Boxes, path: '/items' },
	{ label: 'Character', icon: PersonStanding, path: '/character' }
];

class Nav {
	get groups(): NavGroup[] {
		return [{ items: coreItems }];
	}
}

export const nav = new Nav();
