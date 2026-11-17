<script lang="ts">
	import { goto } from '$app/navigation';
	import { getHomeContinuation, getHomeFeed } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedChip, FeedItem } from '$lib/types';

	let query = $state('');
	let items = $state<FeedItem[]>([]);
	let chips = $state<FeedChip[]>([]);
	let continuation = $state<string | null>(null);
	let error = $state('');
	let loading = $state(false);
	let loadingMore = $state(false);
	let moreError = $state('');

	$effect(() => {
		loading = true;
		error = '';
		getHomeFeed()
			.then((page) => {
				items = page.items;
				chips = page.chips ?? [];
				continuation = page.continuation ?? null;
			})
			.catch((e) => {
				error = String(e);
			})
			.finally(() => {
				loading = false;
			});
	});

	async function loadMore() {
		if (!continuation || loadingMore) return;
		loadingMore = true;
		moreError = '';
		try {
			const page = await getHomeContinuation(continuation);
			items = [...items, ...page.items];
			continuation = page.continuation ?? null;
		} catch (e) {
			moreError = String(e);
		} finally {
			loadingMore = false;
		}
	}

	function onSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (query.trim()) goto(`/search?q=${encodeURIComponent(query.trim())}`);
	}
</script>

<h1>Home</h1>
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
{#if !loading && !error && items.length === 0}<p>No videos on your Home feed yet.</p>{/if}
{#if chips.length > 0}
	<p class="chips">
		{#each chips as c, i (i)}
			<span class:selected={c.selected}>{c.text}</span>{' '}
		{/each}
	</p>
{/if}
<VideoList {items} />
{#if continuation}
	<button type="button" disabled={loadingMore} onclick={loadMore}>
		{loadingMore ? 'Loading…' : 'Load more'}
	</button>
{/if}
{#if moreError}<p role="alert">couldn't load more: {moreError}</p>{/if}

<style>
	.chips span.selected {
		color: var(--accent);
		font-weight: bold;
	}
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
