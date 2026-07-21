import { describe, expect, test } from "bun:test";
import { buildCaptionRenderState } from "../caption-element";
import type { CaptionCue } from "@/timeline";
import type { MediaTime } from "@/wasm";

function ticks(value: number): MediaTime {
	return value as MediaTime;
}

function cue(words: Array<[string, string, number, number]>): CaptionCue {
	return {
		id: "cue",
		startTime: ticks(0),
		endTime: ticks(100),
		words: words.map(([id, displayText, startTime, endTime]) => ({
			id,
			spokenText: displayText,
			displayText,
			startTime: ticks(startTime),
			endTime: ticks(endTime),
		})),
	};
}

describe("semantic caption layout", () => {
	test("tracks the active word without mutating cue text", () => {
		const state = buildCaptionRenderState({
			cue: cue([
				["one", "Hello", 0, 20],
				["two", "world", 20, 40],
			]),
			localTime: ticks(25),
			maxCharactersPerLine: 30,
			maxLines: 2,
		});
		expect(state.content).toBe("Hello world");
		expect(state.activeWordId).toBe("two");
		expect(state.activePrefix).toBe("Hello ");
	});

	test("does not insert spaces between CJK tokens", () => {
		const state = buildCaptionRenderState({
			cue: cue([
				["one", "你好", 0, 20],
				["two", "世界", 20, 40],
				["three", "！", 40, 60],
			]),
			localTime: ticks(30),
			maxCharactersPerLine: 10,
			maxLines: 2,
		});
		expect(state.content).toBe("你好世界！");
	});

	test("wraps on Unicode code points", () => {
		const state = buildCaptionRenderState({
			cue: cue([
				["one", "剪辑", 0, 20],
				["two", "真的", 20, 40],
				["three", "很简单", 40, 60],
			]),
			localTime: ticks(50),
			maxCharactersPerLine: 4,
			maxLines: 2,
		});
		expect(state.content).toBe("剪辑真的\n很简单");
		expect(state.activeLineIndex).toBe(1);
	});
});
