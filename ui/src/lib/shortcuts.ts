// Keyboard shortcuts — pure matching logic, no DOM, no player.
// The Watch page listens for the dispatched CustomEvents and drives the
// player; shortcuts never talk to YouTube or to mpv directly.
//
// Conventions (YouTube-familiar, kept on purpose so muscle memory works):
//   k / Space  play-pause      j / l  seek -10s / +10s
//   ArrowLeft/Right seek ∓5s   ArrowUp/Down volume
//   f  fullscreen  m  mute     0-9  seek to 0-90%   t  theatre
//   Ctrl/Cmd+K  command palette   ?  shortcut help (palette shows them)
//
// Player actions are dispatched as `tubelite:<action>` CustomEvents on
// window with `detail` carrying the payload (e.g. { seconds: -10 }).

export type ShortcutAction =
	| 'play-pause'
	| 'seek'
	| 'volume'
	| 'fullscreen'
	| 'mute'
	| 'seek-percent'
	| 'theatre'
	| 'open-palette'
	| 'show-help';

export interface ShortcutEvent {
	key: string;
	ctrlKey: boolean;
	metaKey: boolean;
	shiftKey: boolean;
	/** True when the event target is an input/textarea/select/contenteditable. */
	inField: boolean;
}

export interface MatchedShortcut {
	action: ShortcutAction;
	/** Payload for seek (seconds), volume (delta), seek-percent (0-90). */
	value?: number;
}

/** Pure matcher: returns the shortcut for a key event, or null. */
export function matchShortcut(e: ShortcutEvent): MatchedShortcut | null {
	const mod = e.ctrlKey || e.metaKey;
	if (mod && (e.key === 'k' || e.key === 'K')) {
		return { action: 'open-palette' };
	}
	if (mod || e.inField) return null;
	switch (e.key) {
		case 'k':
		case ' ':
			return { action: 'play-pause' };
		case 'j':
			return { action: 'seek', value: -10 };
		case 'l':
			return { action: 'seek', value: 10 };
		case 'ArrowLeft':
			return { action: 'seek', value: -5 };
		case 'ArrowRight':
			return { action: 'seek', value: 5 };
		case 'ArrowUp':
			return { action: 'volume', value: 5 };
		case 'ArrowDown':
			return { action: 'volume', value: -5 };
		case 'f':
			return { action: 'fullscreen' };
		case 'm':
			return { action: 'mute' };
		case 't':
			return { action: 'theatre' };
		case '?':
			return { action: 'show-help' };
		default:
			if (/^[0-9]$/.test(e.key)) {
				return { action: 'seek-percent', value: Number(e.key) * 10 };
			}
			return null;
	}
}

/** Dispatch a matched shortcut as a window CustomEvent. No-op in SSR. */
export function dispatchShortcut(m: MatchedShortcut): void {
	if (typeof window === 'undefined') return;
	window.dispatchEvent(
		new CustomEvent(`tubelite:${m.action}`, { detail: { value: m.value } })
	);
}
