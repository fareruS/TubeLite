// Typed settings over the Tauri get_setting / set_setting IPC commands.
// Keys are plain strings in the SQLite `settings` table (see store crate).
// All values are strings; booleans are "true"/"false".

import { invoke } from '@tauri-apps/api/core';
import { applyTheme, type Theme } from './theme';

export type QualitySetting = 'auto' | '144' | '240' | '360' | '480' | '720' | '1080' | 'audio';

export interface AppSettings {
	quality: QualitySetting;
	audioOnly: boolean;
	theme: Theme;
	ytdlpPath: string;
}

export const DEFAULT_SETTINGS: AppSettings = {
	quality: 'auto',
	audioOnly: false,
	theme: 'system',
	ytdlpPath: ''
};

const get = (key: string) => invoke<string | null>('get_setting', { key });
const set = (key: string, value: string) => invoke('set_setting', { key, value });

export async function loadSettings(): Promise<AppSettings> {
	const [quality, audioOnly, theme, ytdlpPath] = await Promise.all([
		get('quality'),
		get('audio_only'),
		get('theme'),
		get('ytdlp_path')
	]);
	const q = quality ?? DEFAULT_SETTINGS.quality;
	return {
		quality: (
			['auto', '144', '240', '360', '480', '720', '1080', 'audio'] as QualitySetting[]
		).includes(q as QualitySetting)
			? (q as QualitySetting)
			: DEFAULT_SETTINGS.quality,
		audioOnly: audioOnly === 'true',
		theme: (['light', 'dark', 'system'] as Theme[]).includes(theme as Theme)
			? (theme as Theme)
			: DEFAULT_SETTINGS.theme,
		ytdlpPath: ytdlpPath ?? ''
	};
}

export async function saveSettings(s: AppSettings): Promise<void> {
	await Promise.all([
		set('quality', s.quality),
		set('audio_only', s.audioOnly ? 'true' : 'false'),
		set('theme', s.theme),
		set('ytdlp_path', s.ytdlpPath)
	]);
	applyTheme(s.theme);
}
