import type { ParamValues } from "@/params";

export interface CaptionPreset {
	id: string;
	name: string;
	description: string;
	params: ParamValues;
	highlightColor: string;
	maxLines: number;
	maxCharactersPerLine: number;
	wordHighlight: boolean;
}
export const CAPTION_PRESETS: CaptionPreset[] = [
	{
		id: "studio-clean",
		name: "Studio Clean",
		description: "Neutral white captions with a restrained shadow.",
		params: { fontFamily: "Inter", fontSize: 6, fontWeight: "bold", color: "#ffffff", textAlign: "center", "background.enabled": false },
		highlightColor: "#67e8f9",
		maxLines: 2,
		maxCharactersPerLine: 28,
		wordHighlight: true,
	},
	{
		id: "ink-card",
		name: "Ink Card",
		description: "Warm paper text on a dark rounded card.",
		params: { fontFamily: "Georgia", fontSize: 5.5, fontWeight: "bold", color: "#fff7df", textAlign: "center", "background.enabled": true, "background.color": "#171512e8", "background.cornerRadius": 22, "background.paddingX": 18, "background.paddingY": 10 },
		highlightColor: "#fbbf24",
		maxLines: 2,
		maxCharactersPerLine: 30,
		wordHighlight: true,
	},
	{
		id: "signal-yellow",
		name: "Signal Yellow",
		description: "Bold black type with a high-visibility yellow highlight.",
		params: { fontFamily: "Arial", fontSize: 7, fontWeight: "bold", color: "#ffffff", textAlign: "center", "background.enabled": false },
		highlightColor: "#fde047",
		maxLines: 2,
		maxCharactersPerLine: 22,
		wordHighlight: true,
	},
	{
		id: "editorial-serif",
		name: "Editorial Serif",
		description: "Elegant mixed-case serif captions.",
		params: { fontFamily: "Georgia", fontSize: 5.8, fontWeight: "normal", fontStyle: "italic", color: "#fffaf0", textAlign: "center", letterSpacing: 0.2, "background.enabled": false },
		highlightColor: "#fb7185",
		maxLines: 2,
		maxCharactersPerLine: 32,
		wordHighlight: true,
	},
	{
		id: "electric-blue",
		name: "Electric Blue",
		description: "Compact tech captions with cyan word tracking.",
		params: { fontFamily: "Inter", fontSize: 5.7, fontWeight: "bold", color: "#dbeafe", textAlign: "center", letterSpacing: 0.6, "background.enabled": true, "background.color": "#071a36d9", "background.cornerRadius": 14 },
		highlightColor: "#22d3ee",
		maxLines: 2,
		maxCharactersPerLine: 28,
		wordHighlight: true,
	},
	{
		id: "mono-terminal",
		name: "Mono Terminal",
		description: "Left-aligned terminal-inspired captions.",
		params: { fontFamily: "Courier New", fontSize: 5.2, fontWeight: "bold", color: "#d1fae5", textAlign: "left", letterSpacing: 0, "background.enabled": true, "background.color": "#04130ee6", "background.cornerRadius": 8 },
		highlightColor: "#34d399",
		maxLines: 3,
		maxCharactersPerLine: 34,
		wordHighlight: true,
	},
	{
		id: "paper-label",
		name: "Paper Label",
		description: "Black editorial type on a pale label.",
		params: { fontFamily: "Arial", fontSize: 5.6, fontWeight: "bold", color: "#18181b", textAlign: "center", "background.enabled": true, "background.color": "#f5f1e8f2", "background.cornerRadius": 4, "background.paddingX": 20 },
		highlightColor: "#dc2626",
		maxLines: 2,
		maxCharactersPerLine: 29,
		wordHighlight: true,
	},
	{
		id: "neon-magenta",
		name: "Neon Magenta",
		description: "Nightlife captions with vivid magenta tracking.",
		params: { fontFamily: "Arial", fontSize: 6.2, fontWeight: "bold", color: "#ffffff", textAlign: "center", "background.enabled": true, "background.color": "#19051bcc", "background.cornerRadius": 28 },
		highlightColor: "#f472b6",
		maxLines: 2,
		maxCharactersPerLine: 25,
		wordHighlight: true,
	},
	{
		id: "documentary",
		name: "Documentary",
		description: "Quiet lower-third captions for interviews.",
		params: { fontFamily: "Arial", fontSize: 4.8, fontWeight: "normal", color: "#ffffff", textAlign: "left", "background.enabled": true, "background.color": "#000000b8", "background.cornerRadius": 2 },
		highlightColor: "#ffffff",
		maxLines: 2,
		maxCharactersPerLine: 38,
		wordHighlight: false,
	},
	{
		id: "sports-score",
		name: "Sports Score",
		description: "Condensed all-action styling for fast edits.",
		params: { fontFamily: "Arial", fontSize: 7.2, fontWeight: "bold", fontStyle: "italic", color: "#ffffff", textAlign: "center", letterSpacing: -0.4, "background.enabled": false },
		highlightColor: "#a3e635",
		maxLines: 2,
		maxCharactersPerLine: 20,
		wordHighlight: true,
	},
	{
		id: "soft-lavender",
		name: "Soft Lavender",
		description: "Rounded, calm styling for lifestyle videos.",
		params: { fontFamily: "Inter", fontSize: 5.5, fontWeight: "bold", color: "#faf5ff", textAlign: "center", "background.enabled": true, "background.color": "#3b1e4dcc", "background.cornerRadius": 36 },
		highlightColor: "#d8b4fe",
		maxLines: 2,
		maxCharactersPerLine: 28,
		wordHighlight: true,
	},
	{
		id: "cjk-focus",
		name: "CJK Focus",
		description: "Balanced Unicode line length for Chinese, Japanese and Korean.",
		params: { fontFamily: "Noto Sans CJK SC", fontSize: 6.4, fontWeight: "bold", color: "#ffffff", textAlign: "center", letterSpacing: 0.4, lineHeight: 1.3, "background.enabled": true, "background.color": "#111827d9", "background.cornerRadius": 18 },
		highlightColor: "#fca5a5",
		maxLines: 2,
		maxCharactersPerLine: 16,
		wordHighlight: true,
	},
];

export const DEFAULT_CAPTION_PRESET = CAPTION_PRESETS[0];

export function getCaptionPreset({ id }: { id: string }): CaptionPreset {
	return CAPTION_PRESETS.find((preset) => preset.id === id) ?? DEFAULT_CAPTION_PRESET;
}
