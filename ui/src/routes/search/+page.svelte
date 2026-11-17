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

	$effect(() => {
		query = q;
		if (!q) {
			items = [];
			error = '';
			loading = false;
			return;
		}
		const id = ++reqId;
		loading = true;
		error = '';
		searchVideos(q)
			.then((page) => {
				if (id === reqId) items = page.items;
			})
			.catch((e) => {
				if (id === reqId) error = String(e);
			})
			.finally(() => {
				if (id === reqId) loading = false;
			});
	});

	function onSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (query.trim()) goto(`/search?q=${encodeURIComponent(query.trim())}`);
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
