// Player IPC wrappers. The UI never touches mpv directly — every call
// goes through the Tauri commands in src-tauri/src/player_ipc.rs.
import { invoke } from '@tauri-apps/api/core';

export interface PlayerStatus {
	position_secs: number;
	duration_secs: number | null;
	paused: boolean;
	speed: number;
	volume: number;
	muted: boolean;
	audio_only: boolean;
	quality_height: number | null;
	/** Id of the video actually loaded; changes when autoplay advances. */
	current_video_id: string | null;
	/** Playback failure message (e.g. the next video could not be resolved); null otherwise. */
	error: string | null;
}

export const playVideo = (id: string, height?: number) =>
	invoke<PlayerStatus>('play_video', { id, height: height ?? null });
export const playerPlay = () => invoke<PlayerStatus>('player_play');
export const playerPause = () => invoke<PlayerStatus>('player_pause');
export const playerToggle = () => invoke<PlayerStatus>('player_toggle');
export const playerSeek = (secs: number) => invoke<PlayerStatus>('player_seek', { secs });
export const playerSetSpeed = (speed: number) => invoke<PlayerStatus>('player_set_speed', { speed });
export const playerSetVolume = (volume: number) =>
	invoke<PlayerStatus>('player_set_volume', { volume });
export const playerSetMuted = (muted: boolean) => invoke<PlayerStatus>('player_set_muted', { muted });
export const playerSetAudioOnly = (audioOnly: boolean) =>
	invoke<PlayerStatus>('player_set_audio_only', { audioOnly });
export const playerStatus = () => invoke<PlayerStatus>('player_status');
export const playerNext = () => invoke<string | null>('player_next');
