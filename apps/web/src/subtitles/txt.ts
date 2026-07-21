import type { ParseSubtitleResult } from "./types";

const DEFAULT_CUE_SECONDS = 3;

export function parseTxt({ input }: { input: string }): ParseSubtitleResult {
	const paragraphs = input
		.replace(/^\uFEFF/, "")
		.replace(/\r\n?/g, "\n")
		.split(/\n{2,}/)
		.map((paragraph) => paragraph.trim())
		.filter(Boolean);
	return {
		captions: paragraphs.map((text, index) => ({
			text,
			startTime: index * DEFAULT_CUE_SECONDS,
			duration: DEFAULT_CUE_SECONDS,
		})),
		skippedCueCount: 0,
		warnings:
			paragraphs.length > 0
				? ["TXT has no timing data; generated consecutive 3-second cues."]
				: [],
	};
}
