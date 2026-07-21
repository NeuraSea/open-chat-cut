import type { ParseSubtitleResult, SubtitleCue } from "./types";

const CUE_TIMING = /^(?:(\d+):)?(\d{2}):(\d{2})[.](\d{3})\s+-->\s+(?:(\d+):)?(\d{2}):(\d{2})[.](\d{3})(?:\s+.*)?$/;

function timestamp({
	hours,
	minutes,
	seconds,
	milliseconds,
}: {
	hours?: string;
	minutes: string;
	seconds: string;
	milliseconds: string;
}): number {
	return (
		Number(hours ?? 0) * 3600 +
		Number(minutes) * 60 +
		Number(seconds) +
		Number(milliseconds) / 1000
	);
}
function stripVttMarkup({ input }: { input: string }): string {
	return input
		.replace(/<\d{2}:\d{2}(?::\d{2})?[.]\d{3}>/g, "")
		.replace(/<[^>]*>/g, "")
		.replaceAll("&lt;", "<")
		.replaceAll("&gt;", ">")
		.replaceAll("&amp;", "&")
		.trim();
}

export function parseVtt({ input }: { input: string }): ParseSubtitleResult {
	const normalized = input.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n").trim();
	if (!normalized) return { captions: [], skippedCueCount: 0, warnings: [] };
	const blocks = normalized.replace(/^WEBVTT[^\n]*\n+/i, "").split(/\n{2,}/);
	const captions: SubtitleCue[] = [];
	let skippedCueCount = 0;

	for (const block of blocks) {
		const lines = block.split("\n").map((line) => line.trim());
		if (!lines[0] || /^(NOTE|STYLE|REGION)(?:\s|$)/.test(lines[0])) continue;
		const timingIndex = lines.findIndex((line) => line.includes("-->"));
		if (timingIndex < 0) {
			skippedCueCount += 1;
			continue;
		}
		const match = lines[timingIndex].match(CUE_TIMING);
		if (!match) {
			skippedCueCount += 1;
			continue;
		}
		const startTime = timestamp({ hours: match[1], minutes: match[2], seconds: match[3], milliseconds: match[4] });
		const endTime = timestamp({ hours: match[5], minutes: match[6], seconds: match[7], milliseconds: match[8] });
		const text = stripVttMarkup({ input: lines.slice(timingIndex + 1).join("\n") });
		if (!text || endTime <= startTime) {
			skippedCueCount += 1;
			continue;
		}
		captions.push({ text, startTime, duration: endTime - startTime });
	}

	return { captions, skippedCueCount, warnings: [] };
}
