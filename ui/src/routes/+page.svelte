<script lang="ts">
	import { searchVideos } from '$lib/api';
	import { pageCacheGet, pageCacheSet } from '$lib/pagecache';
	import { windowedSlice } from '$lib/rows';
	import type { VideoCard } from '$lib/types';

	// Fixed row height: each card row is exactly ROW_PX tall (CSS below),
	// so the windowing math in rows.ts is honest — no assumed heights.
	const ROW_PX = 96;

	let query = $state('');
	let videos = $state<VideoCard[]>([]);
	let error = $state('');
	let loading = $state(false);
	let scrollTop = $state(0);
	let viewportHeight = $state(600);
	let listEl: HTMLDivElement | undefined = $state();

	let win = $derived(windowedSlice(videos, scrollTop, viewportHeight, ROW_PX));
	let visible = $derived(videos.slice(win.start, win.end));

	async function runSearch(q: string) {
		const cached = pageCacheGet<VideoCard[]>(`search:${q}`);
		if (cached) {
			videos = cached;
			return;
		}
		loading = true;
		error = '';
		try {
			const page = await searchVideos(q);
			videos = page.items;
			pageCacheSet(`search:${q}`, page.items);
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	}

	function onSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (query.trim()) runSearch(query.trim());
	}

	$effect(() => {
		runSearch('fixture');
	});
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
{#if error}<p role="alert">{error}</p>{/if}
<div
	class="list"
	bind:this={listEl}
	onscroll={(e) => {
		scrollTop = (e.currentTarget as HTMLElement).scrollTop;
		viewportHeight = (e.currentTarget as HTMLElement).clientHeight;
	}}
>
	<div style="height: {win.padTop}px"></div>
	{#each visible as v (v.id)}
		<article class="video-card">
			<a href={`/watch/${v.id}`}>
				{#if v.thumbnails[0]}
					<img
						src={v.thumbnails[0].url}
						alt=""
						loading="lazy"
						decoding="async"
						width="120"
						height="90"
					/>
				{/if}
				<div>
					<h2>{v.title}</h2>
					<p>{v.channel_title}{v.duration_secs ? ` · ${v.duration_secs}s` : ''}</p>
				</div>
			</a>
		</article>
	{/each}
	<div style="height: {win.padBottom}px"></div>
</div>
<p class="note">
	{videos.length} results · windowed rendering ({win.start}–{win.end} of {videos.length} rows
	in the DOM) · search results cached 5 min, max 80 pages.
</p>

<style>
	.list {
		height: 600px;
		overflow-y: auto;
	}
	.video-card {
		height: 96px;
	}
	.video-card a {
		display: flex;
		gap: 0.75rem;
		align-items: center;
		height: 100%;
		color: inherit;
		text-decoration: none;
	}
	.video-card h2 {
		font-size: 1rem;
		margin: 0;
	}
	.video-card p {
		margin: 0.25rem 0 0;
		opacity: 0.75;
	}
	.note {
		opacity: 0.7;
		font-size: 0.85rem;
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
