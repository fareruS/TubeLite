<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { windowedSlice } from '$lib/rows';
	import type { FeedItem, FeedThumb } from '$lib/types';

	// Shared windowed feed list (Home, search, channel, playlist, library,
	// watch Up Next). Fixed row height: each card row is exactly ROW_PX tall
	// (CSS below), so the windowing math in rows.ts is honest.
	const ROW_PX = 96;

	let { items }: { items: FeedItem[] } = $props();

	let scrollTop = $state(0);
	let viewportHeight = $state(600);
	let listEl: HTMLDivElement | undefined = $state();
	let saved = $state<Set<string>>(new Set());

	let win = $derived(windowedSlice(items, scrollTop, viewportHeight, ROW_PX));
	let visible = $derived(items.slice(win.start, win.end));

	// Thumbnails arrive largest-first; pick a mid-size one so rows stay light.
	function pickThumb(thumbs: FeedThumb[]): string | undefined {
		for (const t of thumbs) {
			if (t.width != null && t.width <= 480) return t.url;
		}
		return thumbs[0]?.url;
	}

	$effect(() => {
		invoke<{ video_id: string }[]>('list_watch_later')
			.then((list) => {
				saved = new Set(list.map((e) => e.video_id));
			})
			.catch(() => {});
	});

	async function toggleWatchLater(v: FeedItem) {
		if (saved.has(v.video_id)) {
			await invoke('remove_watch_later', { videoId: v.video_id });
			saved.delete(v.video_id);
		} else {
			await invoke('add_watch_later', {
				videoId: v.video_id,
				title: v.title,
				channel: v.channel_name
			});
			saved.add(v.video_id);
		}
		saved = new Set(saved);
	}
</script>

<div
	class="list"
	bind:this={listEl}
	onscroll={(e) => {
		scrollTop = (e.currentTarget as HTMLElement).scrollTop;
		viewportHeight = (e.currentTarget as HTMLElement).clientHeight;
	}}
>
	<div style="height: {win.padTop}px"></div>
	{#each visible as v (v.video_id)}
		{@const thumb = pickThumb(v.thumbnails)}
		<article class="video-card">
			<a class="thumb-link" href={`/watch/${v.video_id}`}>
				{#if thumb}
					<img src={thumb} alt="" loading="lazy" decoding="async" />
				{/if}
				{#if v.duration_text}<span class="duration">{v.duration_text}</span>{/if}
			</a>
			<div>
				<h2><a href={`/watch/${v.video_id}`}>{v.title}</a></h2>
				<p>
					{#if v.channel_id}
						<a href={`/channel/${v.channel_id}`}>{v.channel_name}</a>
					{:else}
						{v.channel_name}
					{/if}
					{#if v.view_count_text} · {v.view_count_text}{/if}
					{#if v.published_text} · {v.published_text}{/if}
					{#each v.badges as b (b.label)} · {b.label}{/each}
				</p>
				<button onclick={() => toggleWatchLater(v)}>
					{saved.has(v.video_id) ? '✓ Saved' : '+ Watch later'}
				</button>
			</div>
		</article>
	{/each}
	<div style="height: {win.padBottom}px"></div>
</div>
{#if items.length > 0}
	<p class="note">Showing rows {win.start + 1}–{win.end} of {items.length} in the DOM.</p>
{/if}

<style>
	.list {
		height: 600px;
		overflow-y: auto;
	}
	.video-card {
		height: 96px;
		display: flex;
		gap: 0.75rem;
		border-bottom: 1px solid var(--border);
	}
	.thumb-link {
		position: relative;
		display: block;
		flex: none;
	}
	.video-card img {
		width: 128px;
		height: 72px;
		object-fit: cover;
		display: block;
	}
	.duration {
		position: absolute;
		right: 4px;
		bottom: 4px;
		background: rgba(0, 0, 0, 0.8);
		color: #fff;
		font-size: 0.75rem;
		padding: 0 4px;
	}
	.video-card h2 {
		font-size: 1rem;
		margin: 0.25rem 0;
	}
	.video-card h2 a {
		color: inherit;
		text-decoration: none;
	}
	.video-card p {
		color: var(--muted);
		margin: 0;
	}
	.video-card p a {
		color: inherit;
	}
	.note {
		color: var(--muted);
	}
</style>
