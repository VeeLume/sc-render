// Window-width breakpoints, Material's window-size classes. The layout asks
// this ("showRail?"), never the raw width — so a breakpoint change is one edit
// here, not a grep across components.
import { browser } from '$app/environment';

class BreakpointState {
	width = $state(browser ? window.innerWidth : 1024);

	constructor() {
		if (browser) {
			window.addEventListener('resize', () => {
				this.width = window.innerWidth;
			});
		}
	}

	get isCompact() {
		return this.width < 600;
	}
	get isMedium() {
		return this.width >= 600 && this.width < 840;
	}
	get isExpanded() {
		return this.width >= 840 && this.width < 1200;
	}
	get isLarge() {
		return this.width >= 1200 && this.width < 1600;
	}
	get isExtraLarge() {
		return this.width >= 1600;
	}
	get isDesktop() {
		return this.width >= 840;
	}
	/** Compact + medium get the bottom bar; everything wider gets the rail. */
	get showBottomNav() {
		return this.isCompact || this.isMedium;
	}
	get showRail() {
		return this.isDesktop;
	}
	/** The rail only earns its labels once there's width to spare. */
	get showLabels() {
		return this.isLarge || this.isExtraLarge;
	}
}

export const breakpoint = new BreakpointState();
