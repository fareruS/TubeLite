<script lang="ts">
	import { page } from '$app/stores';
	import { invoke } from '@tauri-apps/api/core';
	import { getWatchNext, hideVideoWindow, reportVideoRect } from '$lib/api';
	import { loadSettings } from '$lib/settings';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';
	import {
		playVideo,
		playerToggle,
		playerSeek,
		playerSetSpeed,
		playerSetVolume,
		playerSetMuted,
		playerSetAudioOnly,
		playerStatus,
		type PlayerStatus
	} from '$lib/player';

	interface Chapter {
		title: string;
		start_secs: number;
	}

	interface CaptionTrack {
		language_code: string;
		language_name: string;
		is_auto_generated: boolean;
	}

	let id = $derived($page.params.id ?? '');
	let title = $state('');
	let channelTitle = $state('');
	let chapters = $state<Chapter[]>([]);
	let captions = $state<CaptionTrack[]>([]);
	let activeCaption = $state('');
	let captionError = $state('');
	let status = $state<PlayerStatus | null>(null);
	let error = $state('');
	let seeking = $state(false);
	let theatre = $state(false);
	let quality = $state('auto');
	let upNext = $state<FeedItem[]>([]);
	let upNextError = $state('');
	let upNextLoading = $state(false);

	// The chapter containing the current position, for highlighting.
	let currentChapter = $derived.by(() => {
		if (chapters.length === 0 || !status) return -1;
		let idx = 0;
		for (let i = 0; i < chapters.length; i++) {
			if (chapters[i].start_secs <= status.position_secs) idx = i;
			else break;
		}
		return idx;
	});

	$effect(() => {
		invoke<{
			title: string;
			channel_title: string;
			chapters: Chapter[];
			captions: CaptionTrack[];
		}>('get_video', { id })
			.then((v) => {
				title = v.title;
				channelTitle = v.channel_title;
				chapters = v.chapters ?? [];
				captions = v.captions ?? [];
				// Record the watch in history (position 0 — the resume
				// position is saved on leave, see below).
				invoke('record_history_entry', {
					videoId: id,
					title: v.title,
					channel: v.channel_title,
					positionSecs: 0,
					durationSecs: null
				}).catch(() => {});
			})
			.catch(() => {
				title = id;
			});
	});

	// Up Next feed for this video.
	$effect(() => {
		if (!id) return;
		upNextLoading = true;
		upNextError = '';
		getWatchNext(id)
			.then((p) => {
				upNext = p.items;
			})
			.catch((e) => {
				upNextError = String(e);
			})
			.finally(() => {
				upNextLoading = false;
			});
	});

	// Save the resume position when leaving the page, and hide the
	// native video surface so it does not linger over the next page.
	$effect(() => {
		return () => {
			if (status && status.position_secs > 5) {
				invoke('save_resume_position', { videoId: id, positionSecs: status.position_secs }).catch(
					() => {}
				);
			}
			hideVideoWindow().catch(() => {});
		};
	});

	// Start playback on mount, at the user's saved quality. On a build
	// without the mpv feature the command fails and the error is shown —
	// never a silent black rectangle.
	$effect(() => {
		let cancelled = false;
		// Report the rect first: the native surface must exist
		// before the first loadfile, or mpv opens its own window.
		onResize()
			.catch(() => {})
			.then(() => loadSettings())
			.then((s) => {
				quality = s.quality;
				const height = s.quality === 'auto' ? 0 : s.quality === 'audio' ? undefined : Number(s.quality);
				return playVideo(id, height);
			})
			.then(async (st) => {
				if (cancelled) return;
				status = st;
				// Resume where the user left off: only when meaningfully
				// mid-video (>5 s in, >10 s from the end).
				try {
					const saved = await invoke<number | null>('get_resume_position', { videoId: id });
					if (saved != null && saved > 5) {
						const dur = st.duration_secs;
						if (dur == null || saved < dur - 10) {
							const resumed = await playerSeek(saved);
							status = resumed;
						}
					}
				} catch {
					/* no saved position — start from 0 */
				}
			})
			.catch((e) => {
				if (!cancelled) error = String(e);
			});
		return () => {
			cancelled = true;
		};
	});

	// 4 Hz position tick (the plan's tick rate).
	$effect(() => {
		const t = setInterval(() => {
			if (seeking) return;
			playerStatus()
				.then((st) => {
					status = st;
				})
				.catch(() => {
					/* player not running — the error line already says so */
				});
		}, 250);
		return () => clearInterval(t);
	});

	// Keyboard shortcuts arrive as `tubelite:<action>` events (see
	// lib/shortcuts.ts + CommandPalette). Wire the playback ones here.
	$effect(() => {
		function onAction(e: Event) {
			const detail = (e as CustomEvent<{ value?: number }>).detail ?? {};
			const action = (e as CustomEvent).type.replace('tubelite:', '');
			if (action === 'play-pause') playerToggle().then((st) => (status = st));
			else if (action === 'seek') seekBy(detail.value ?? 0);
			else if (action === 'seek-percent' && status?.duration_secs)
				playerSeek((status.duration_secs * (detail.value ?? 0)) / 100).then((st) => (status = st));
			else if (action === 'fullscreen') toggleFullscreen();
			else if (action === 'theatre') theatre = !theatre;
			else if (action === 'mute' && status) toggleMute();
			else if (action === 'volume' && status)
				playerSetVolume(Math.min(100, Math.max(0, status.volume + (detail.value ?? 0)))).then(
					(st) => (status = st)
				);
		}
		window.addEventListener('tubelite:play-pause', onAction);
		window.addEventListener('tubelite:seek', onAction);
		window.addEventListener('tubelite:seek-percent', onAction);
		window.addEventListener('tubelite:fullscreen', onAction);
		window.addEventListener('tubelite:theatre', onAction);
		window.addEventListener('tubelite:mute', onAction);
		window.addEventListener('tubelite:volume', onAction);
		return () => {
			window.removeEventListener('tubelite:play-pause', onAction);
			window.removeEventListener('tubelite:seek', onAction);
			window.removeEventListener('tubelite:seek-percent', onAction);
			window.removeEventListener('tubelite:fullscreen', onAction);
			window.removeEventListener('tubelite:theatre', onAction);
			window.removeEventListener('tubelite:mute', onAction);
			window.removeEventListener('tubelite:volume', onAction);
		};
	});

	function seekBy(delta: number) {
		if (!status) return;
		playerSeek(Math.max(0, status.position_secs + delta)).then((st) => (status = st));
	}

	function toggleMute() {
		if (!status) return;
		playerSetMuted(!status.muted).then((st) => (status = st));
	}

	// Quality switch mid-playback: re-resolve at the new quality, then
	// seek back to where the viewer was (the <500 ms switch gap is a
	// measured target, not something the UI can assert).
	async function changeQuality(q: string) {
		if (!status) return;
		const pos = status.position_secs;
		quality = q;
		const height = q === 'auto' ? 0 : q === 'audio' ? undefined : Number(q);
		try {
			const st = await playVideo(id, height);
			status = st;
			if (pos > 1) {
				const resumed = await playerSeek(pos);
				status = resumed;
			}
		} catch (e) {
			error = String(e);
		}
	}

	// Report the video rectangle (physical px) over IPC so the
	// native video surface can be placed over the hole. Called at
	// mount, on resize (ResizeObserver below), and before playback
	// starts — the surface must exist before the first loadfile, or
	// mpv opens its own window instead.
	function onResize() {
		const el = document.getElementById('video-hole');
		if (!el) return Promise.resolve();
		const r = el.getBoundingClientRect();
		const scale = window.devicePixelRatio || 1;
		return reportVideoRect(r.x * scale, r.y * scale, r.width * scale, r.height * scale);
	}

	// Keep the native surface aligned with the hole: a ResizeObserver
	// fires on theatre/fullscreen/layout changes, and window resize
	// is the fallback. (The `onresize` attribute the div used to
	// carry never fires on a plain element.)
	$effect(() => {
		const el = document.getElementById('video-hole');
		if (!el) return;
		onResize().catch(() => {});
		const ro = new ResizeObserver(() => {
			onResize().catch(() => {});
		});
		ro.observe(el);
		const onWinResize = () => {
			onResize().catch(() => {});
		};
		window.addEventListener('resize', onWinResize);
		return () => {
			ro.disconnect();
			window.removeEventListener('resize', onWinResize);
		};
	});

	// Fullscreen on the video hole. The native surface follows the
	// hole through the ResizeObserver above.
	function toggleFullscreen() {
		const el = document.getElementById('video-hole');
		if (!el) return;
		if (document.fullscreenElement) void document.exitFullscreen();
		else void el.requestFullscreen().catch(() => {});
	}

	function fmt(secs: number | null | undefined): string {
		if (secs == null || !isFinite(secs)) return '--:--';
		const m = Math.floor(secs / 60);
		const s = Math.floor(secs % 60);
		return `${m}:${String(s).padStart(2, '0')}`;
	}

</script>

<h1>{title || id}</h1>
{#if error}
	<p class="error-banner" role="alert">Playback unavailable: {error}</p>
{/if}
<div class:theatre>
	<div id="video-hole" style="aspect-ratio: 16/9; background: #000;"></div>
</div>

{#if status}
	<div class="controls">
		<button onclick={() => playerToggle().then((st) => (status = st))}>
			{status.paused ? '▶ Play' : '⏸ Pause'}
		</button>
		<input
			type="range"
			class="seek"
			min="0"
			max={status.duration_secs ?? 0}
			step="0.1"
			value={status.position_secs}
			onpointerdown={() => (seeking = true)}
			onchange={(e) => {
				seeking = false;
				playerSeek(Number((e.currentTarget as HTMLInputElement).value)).then((st) => (status = st));
			}}
			oninput={(e) => {
				status = { ...status!, position_secs: Number((e.currentTarget as HTMLInputElement).value) };
			}}
			aria-label="Seek"
		/>
		<span>{fmt(status.position_secs)} / {fmt(status.duration_secs)}</span>
		{#if currentChapter >= 0 && chapters[currentChapter]}
			<span class="chapter-now">{chapters[currentChapter].title}</span>
		{/if}
		<select
			value={status.speed}
			onchange={(e) => playerSetSpeed(Number((e.currentTarget as HTMLSelectElement).value)).then((st) => (status = st))}
			aria-label="Speed"
		>
			<option value="0.5">0.5×</option>
			<option value="1">1×</option>
			<option value="1.5">1.5×</option>
			<option value="2">2×</option>
		</select>
		<select
			value={quality}
			onchange={(e) => changeQuality((e.currentTarget as HTMLSelectElement).value)}
			aria-label="Quality"
		>
			<option value="auto">Auto (720p)</option>
			<option value="144">144p</option>
			<option value="240">240p</option>
			<option value="360">360p</option>
			<option value="480">480p</option>
			<option value="720">720p</option>
			<option value="1080">1080p</option>
			<option value="audio">Audio only</option>
		</select>
		<label>
			<input
				type="checkbox"
				checked={status.audio_only}
				onchange={(e) => playerSetAudioOnly((e.currentTarget as HTMLInputElement).checked).then((st) => (status = st))}
			/>
			Audio only
		</label>
		<button onclick={toggleMute}>{status.muted ? '🔇 Unmute' : '🔊 Mute'}</button>
		<input
			type="range"
			min="0"
			max="100"
			step="1"
			value={status.volume}
			onchange={(e) => playerSetVolume(Number((e.currentTarget as HTMLInputElement).value)).then((st) => (status = st))}
			aria-label="Volume"
		/>
		<button onclick={() => (theatre = !theatre)}>{theatre ? 'Exit theatre (T)' : 'Theatre (T)'}</button>
		<button onclick={toggleFullscreen}>Fullscreen (F)</button>
	</div>
{/if}

{#if captions.length > 0}
	<section aria-label="Captions">
		<h2>Captions</h2>
		<div class="captions">
			<button
				class:active={activeCaption === ''}
				onclick={() => {
					invoke('player_clear_captions')
						.then(() => {
							activeCaption = '';
							captionError = '';
						})
						.catch((e) => (captionError = String(e)));
				}}
			>
				Off
			</button>
			{#each captions as track (track.language_code)}
				<button
					class:active={activeCaption === track.language_code}
					onclick={() => {
						invoke<string>('player_load_captions', { id, lang: track.language_code })
							.then(() => {
								activeCaption = track.language_code;
								captionError = '';
							})
							.catch((e) => (captionError = String(e)));
					}}
				>
					{track.language_name}{track.is_auto_generated ? ' (auto)' : ''}
				</button>
			{/each}
		</div>
		{#if captionError}<p class="error">{captionError}</p>{/if}
	</section>
{/if}

{#if chapters.length > 0}
	<section aria-label="Chapters">
		<h2>Chapters</h2>
		<ol class="chapters">
			{#each chapters as ch, i (ch.start_secs)}
				<li class:active={i === currentChapter}>
					<button onclick={() => playerSeek(ch.start_secs).then((st) => (status = st))}>
						{fmt(ch.start_secs)} — {ch.title}
					</button>
				</li>
			{/each}
		</ol>
	</section>
{/if}

<section aria-label="Up Next">
	<h2>Up Next</h2>
	{#if upNextLoading}<p>Loading…</p>{/if}
	{#if upNextError}<p role="alert">couldn't load feed: {upNextError}</p>{/if}
	{#if !upNextLoading && !upNextError && upNext.length === 0}<p>No up-next videos.</p>{/if}
	<VideoList items={upNext} />
</section>

<p>Video is drawn by mpv, not by a &lt;video&gt; element, so audio and picture share one clock. Player surface is Phase 0/2.</p>
<p>Keyboard: Space play/pause, J/K/L seek, F fullscreen, M mute, C captions. (Phase 4)</p>

<style>
	.controls {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin: 0.5rem 0;
	}
	.controls input.seek {
		flex: 1;
	}
	.theatre #video-hole {
		aspect-ratio: 21 / 9 !important;
	}
	.theatre ~ section {
		display: none;
	}
	.error {
		color: var(--accent);
	}
	.error-banner {
		background: var(--accent-soft);
		border: 1px solid var(--accent);
		padding: 0.75rem 1rem;
		font-weight: 600;
		margin: 0.75rem 0;
	}
	.chapter-now {
		color: var(--muted);
	}
	.captions {
		display: flex;
		gap: 0.5rem;
		flex-wrap: wrap;
	}
	.captions button.active {
		color: var(--accent);
		font-weight: bold;
	}
	.chapters {
		list-style: none;
		padding: 0;
	}
	.chapters li.active button {
		color: var(--accent);
		font-weight: bold;
	}
	.chapters button {
		background: none;
		border: none;
		padding: 0.25rem 0;
		cursor: pointer;
		color: inherit;
		text-align: left;
	}
</style>
