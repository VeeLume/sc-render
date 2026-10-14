<script lang="ts">
	import { appearance, type Theme, type Density } from '$lib/stores/appearance.svelte';
	import { settings } from '$lib/stores/settings.svelte';

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

{#snippet segmented(options: { value: string; label: string }[], current: string, pick: (v: string) => void)}
	<div class="inline-flex rounded-lg border p-0.5">
		{#each options as option (option.value)}
			<button
				class="rounded-md px-3 text-sm font-medium transition-colors"
				class:bg-primary={current === option.value}
				class:text-primary-foreground={current === option.value}
				class:text-muted-foreground={current !== option.value}
				style="height: calc(var(--density-target) - 0.5rem)"
				onclick={() => pick(option.value)}
			>
				{option.label}
			</button>
		{/each}
	</div>
{/snippet}

{#snippet toggle(label: string, hint: string, checked: boolean, set: (v: boolean) => void)}
	<label class="flex items-start justify-between gap-4 py-2">
		<span class="min-w-0">
			<span class="block text-sm font-medium">{label}</span>
			<span class="block text-xs text-muted-foreground">{hint}</span>
		</span>
		<input type="checkbox" class="mt-1 size-4 shrink-0" {checked} onchange={(e) => set(e.currentTarget.checked)} />
	</label>
{/snippet}

<div class="mx-auto max-w-2xl p-[var(--density-padding)]">
	<h1 class="text-2xl font-semibold">Settings</h1>

	<section class="mt-6">
		<h2 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
			Appearance
		</h2>
		<div class="space-y-3 rounded-lg border p-[var(--density-padding)]">
			<div class="flex items-center justify-between gap-4">
				<span class="text-sm font-medium">Theme</span>
				{@render segmented(themes, appearance.theme, (v) => appearance.setTheme(v as Theme))}
			</div>
			<div class="flex items-center justify-between gap-4">
				<span class="text-sm font-medium">Density</span>
				{@render segmented(densities, appearance.density, (v) =>
					appearance.setDensity(v as Density)
				)}
			</div>
		</div>
	</section>
</div>
