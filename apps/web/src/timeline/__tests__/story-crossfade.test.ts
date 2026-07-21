import { describe, expect, test } from "bun:test";

import type { AudioElement } from "../types";
import { TICKS_PER_SECOND, type MediaTime } from "@/wasm";

const { resolveEffectiveAudioGain } = await import("../audio-state");
const mt = (ticks: number) => ticks as MediaTime;

const element: AudioElement = {
	id: "dialogue-part",
	name: "Dialogue",
	type: "audio",
	sourceType: "upload",
	mediaId: "asset:dialogue",
	startTime: mt(0),
	duration: mt(60_000),
	trimStart: mt(0),
	trimEnd: mt(0),
	params: { volume: 0, muted: false },
	storyCrossfade: {
		version: 1,
		fadeInTicks: mt(2_000),
		fadeOutTicks: mt(2_000),
		curve: "equalPower",
		preservesLinkedAvTiming: true,
	},
};

describe("story cut audio envelope", () => {
	test("adds equal-power boundary fades without changing clip timing", () => {
		expect(resolveEffectiveAudioGain({ element, localTime: 0 })).toBe(0);
		expect(
			resolveEffectiveAudioGain({
				element,
				localTime: 1_000 / TICKS_PER_SECOND,
			}),
		).toBeCloseTo(Math.SQRT1_2, 5);
		expect(
			resolveEffectiveAudioGain({
				element,
				localTime: 2_000 / TICKS_PER_SECOND,
			}),
		).toBeCloseTo(1, 5);
		expect(
			resolveEffectiveAudioGain({
				element,
				localTime: 59_000 / TICKS_PER_SECOND,
			}),
		).toBeCloseTo(Math.SQRT1_2, 5);
		expect(Number(element.duration)).toBe(60_000);
		expect(Number(element.startTime)).toBe(0);
	});
});
