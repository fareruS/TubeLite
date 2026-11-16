export interface Thumbnail { url: string; width: number; height: number }
export interface VideoCard { id: string; title: string; channel_id: string; channel_title: string; thumbnails: Thumbnail[]; duration_secs?: number | null; view_count?: number | null; published_at?: string | null; is_live: boolean }
export interface Capabilities { search: boolean; subscriptions: boolean; playlists_write: boolean; like: boolean; comments_read: boolean; gapless: boolean; audio_only: boolean; captions: boolean; chapters: boolean; limitation_note?: string | null }
export interface AccountStatus { signed_in: boolean; account_id?: string | null }
export interface FeedThumb { url: string; width?: number | null; height?: number | null }
export interface FeedBadge { label: string; style?: string | null }
export interface FeedItem { video_id: string; title: string; channel_name: string; channel_id: string; thumbnails: FeedThumb[]; duration_text: string; published_text: string; view_count_text: string; badges: FeedBadge[] }
export interface FeedChip { text: string; token?: string | null; selected: boolean }
export interface FeedPage { items: FeedItem[]; continuation?: string | null; chips: FeedChip[]; stats: { attempted: number; skipped: number } }
