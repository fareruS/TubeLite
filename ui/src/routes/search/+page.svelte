<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { searchVideos } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';

	let q = $derived($page.url.searchParams.get('q') ?? '');
	let query = $state('');
	let items = $state<FeedItem[]>([]);
	let error = $state('');
	let loading = $state(false);

	let reqId = 0;

	function runSearch(term: string) {
		const id = ++reqId;
		loading = true;
		error = '';
		searchVideos(term)
			.then((page) => {
				if (id === reqId) items = page.items;
			})
			.catch((e) => {
				if (id === reqId) error = String(e);
			})
			.finally(() => {
				if (id === reqId) loading = false;
			});
	}

	$effect(() => {
		query = q;
		if (!q) {
			items = [];
			error = '';
			loading = false;
			return;
		}
		runSearch(q);
	});

	function onSubmit(e: SubmitEvent) {
		e.preventDefault();
		const term = query.trim();
		if (!term) return;
		// Re-searching the same query must re-run: goto() to the
		// identical URL is a no-op, so the effect above would never
		// fire and the button looked dead.
		if (term === q) {
			runSearch(term);
		} else {
			goto(`/search?q=${encodeURIComponent(term)}`);
		}
	}
</script>

<h1>Search{#if q}: {q}{/if}</h1>
<form onsubmit={onSubmit} role="search">
	<input
		type="search"
		bind:value={query}
		placeholder="Search videos…"
		aria-label="Search videos"
	/>
	<button type="submit">Search</button>
</form>
{#if loading}<p>Loading…</p>{/if}
{#if error}<p role="alert">couldn't load feed: {error}</p>{/if}
{#if !loading && !error && items.length === 0 && q}<p>No results for “{q}”.</p>{/if}
{#if !q}<p>Type a search above.</p>{/if}
<VideoList {items} />

<style>
	form {
		display: flex;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}
	form input {
		flex: 1;
		padding: 0.5rem;
	}
</style>
