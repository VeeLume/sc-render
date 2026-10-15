<script lang="ts">
	/** A labelled range input with its value shown beside the label. */
	let {
		label,
		value = $bindable(),
		min,
		max,
		step,
		digits = 2,
		oninput,
		onchange
	}: {
		label: string;
		value: number;
		min: number;
		max: number;
		step: number;
		/** Decimals of the shown value. */
		digits?: number;
		/** Every move. */
		oninput?: () => void;
		/** Once the value settles (release, key press). */
		onchange?: () => void;
	} = $props();
</script>

<label class="block text-sm">
	<span class="flex justify-between">
		<span>{label}</span>
		<span class="text-muted-foreground tabular-nums">{value.toFixed(digits)}</span>
	</span>
	<input type="range" class="w-full accent-primary" {min} {max} {step} bind:value {oninput} {onchange} />
</label>
