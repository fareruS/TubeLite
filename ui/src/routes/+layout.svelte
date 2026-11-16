<script lang="ts">
	import '../app.css';
	import CommandPalette from '$lib/components/CommandPalette.svelte';
	let { children } = $props();
</script>

<nav aria-label="Main">
	<a href="/">Home</a> · <a href="/search">Search</a> · <a href="/subscriptions">Subscriptions</a> ·
	<a href="/library">Library</a> · <a href="/settings">Settings</a>
	<span class="hint">Ctrl/⌘ K for commands</span>
</nav>
<main>{@render children()}</main>
<CommandPalette />

<style>
	.hint {
		float: right;
		opacity: 0.6;
		font-size: 0.85rem;
	}
</style>
