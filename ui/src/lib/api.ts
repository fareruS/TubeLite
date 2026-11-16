// UI -> Rust only. The UI never talks to YouTube directly.
import { invoke } from '@tauri-apps/api/core';
import type { AccountStatus, Capabilities, FeedPage, VideoCard } from './types';
export const searchVideos = (query: string) => invoke<{ items: VideoCard[]; next_cursor?: string | null }>('search_videos', { query });
export const getCapabilities = () => invoke<Capabilities>('get_capabilities');
export const reportVideoRect = (x: number, y: number, width: number, height: number) => invoke('report_video_rect', { x, y, width, height });
export const signIn = () => invoke<AccountStatus>('sign_in');
export const signOut = () => invoke<AccountStatus>('sign_out');
export const accountStatus = () => invoke<AccountStatus>('account_status');
export const importCookies = (path: string) => invoke<AccountStatus>('import_cookies', { path });
export const secretBackend = () => invoke<string>('secret_backend');
export const getHomeFeed = () => invoke<FeedPage>('get_home_feed');
