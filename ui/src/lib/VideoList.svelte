<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { windowedSlice } from '$lib/rows';
	import type { VideoCard } from '$lib/types';

	// Shared windowed video list (Home has its own inline copy for now —
	// this component serves search / channel / playlist / library).
	// Fixed row height: each card row is exactly ROW_PX tall (CSS below),
	// so the windowing math in rows.ts is honest.
	const ROW_PX = 96;

	let { videos }: { videos: VideoCard[] } = $props();

	let scrollTop = $state(0);
	let viewportHeight = $state(600);
	let listEl: HTMLDivElement | undefined = $state();
	let saved = $state<Set<string>>(new Set());

	let win = $derived(windowedSlice(videos, scrollTop, viewportHeight, ROW_PX));
	let visible = $derived(videos.slice(win.start, win.end));

	$effect(() => {
		invoke<{ video_id: string }[]>('list_watch_later')
			.then((list) => {
				saved = new Set(list.map((e) => e.video_id));
			})
			.catch(() => {});
	});

	async function toggleWatchLater(v: VideoCard) {
		if (saved.has(v.id)) {
			await invoke('remove_watch_later', { videoId: v.id });
			saved.delete(v.id);
		} else {
			await invoke('add_watch_later', {
				videoId: v.id,
				title: v.title,
				channel: v.channel_title
			});
			saved.add(v.id);
		}
		saved = new Set(saved);
	}

	function fmtDuration(secs: number | null | undefined): string {
		if (secs == null) return '';
		const m = Math.floor(secs / 60);
		const s = Math.floor(secs % 60);
		return `${m}:${String(s).padStart(2, '0')}`;
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
	{#each visible as v (v.id)}
		<article class="video-card">
			<a class="thumb-link" href={`/watch/${v.id}`}>
				{#if v.thumbnails[0]}
					<img src={v.thumbnails[0].url} alt="" loading="lazy" decoding="async" />
				{/if}
			</a>
			<div>
				<h2><a href={`/watch/${v.id}`}>{v.title}</a></h2>
				<p>
					<a href={`/channel/${v.channel_id}`}>{v.channel_title}</a>
					{#if v.duration_secs != null} · {fmtDuration(v.duration_secs)}{/if}
					{#if v.view_count != null} · {v.view_count.toLocaleString()} views{/if}
					{#if v.is_live} · LIVE{/if}
				</p>
				<button onclick={() => toggleWatchLater(v)}>
					{saved.has(v.id) ? '✓ Saved' : '+ Watch later'}
				</button>
			</div>
		</article>
	{/each}
	<div style="height: {win.padBottom}px"></div>
</div>
<p class="note">Showing rows {win.start + 1}–{win.end} of {videos.length} in the DOM.</p>

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
	.video-card img {
		width: 128px;
		height: 72px;
		object-fit: cover;
		flex: none;
	}
	.video-card h2 {
		font-size: 1rem;
		margin: 0.25rem 0;
	}
	.video-card p {
		color: var(--muted);
		margin: 0;
	}
	.note {
		color: var(--muted);
	}
</style>
