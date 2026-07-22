import { describe, expect, test } from "bun:test";

import { motionGraphicTimeSeconds } from "../time";

describe("motionGraphicTimeSeconds", () => {
	test("converts timeline ticks before evaluating second-based keyframes", () => {
		expect(motionGraphicTimeSeconds(0)).toBe(0);
		expect(motionGraphicTimeSeconds(60_000)).toBe(0.5);
		expect(motionGraphicTimeSeconds(120_000)).toBe(1);
	});

	test("clamps negative local time before rendering", () => {
		expect(motionGraphicTimeSeconds(-1)).toBe(0);
	});
});
