import { describe, expect, test } from "bun:test";
import { parseSubtitleFile } from "../parse";

describe("subtitle formats", () => {
	test("parses WebVTT cues and strips inert markup", () => {
		const result = parseSubtitleFile({
			fileName: "captions.vtt",
			input: "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.500 align:center\n<c.speaker>Hello</c> world",
		});
		expect(result.captions).toEqual([
			{ text: "Hello world", startTime: 1, duration: 2.5 },
		]);
	});

	test("imports plain text with an explicit timing warning", () => {
		const result = parseSubtitleFile({
			fileName: "script.txt",
			input: "First paragraph\n\nSecond paragraph",
		});
		expect(result.captions).toHaveLength(2);
		expect(result.captions[1].startTime).toBe(3);
		expect(result.warnings[0]).toContain("no timing data");
	});
});
