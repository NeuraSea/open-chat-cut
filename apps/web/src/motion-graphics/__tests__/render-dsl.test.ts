import { describe, expect, test } from "bun:test";
import { renderMotionGraphicDsl } from "../render-dsl";

describe("motion graphic DSL renderer", () => {
	test("clips bounded groups before rendering their children", async () => {
		const calls: Array<[string, ...number[]]> = [];
		const context = {
			globalAlpha: 1,
			globalCompositeOperation: "source-over",
			clearRect: (...values: number[]) => calls.push(["clearRect", ...values]),
			save: () => calls.push(["save"]),
			restore: () => calls.push(["restore"]),
			translate: (...values: number[]) => calls.push(["translate", ...values]),
			rotate: (...values: number[]) => calls.push(["rotate", ...values]),
			scale: (...values: number[]) => calls.push(["scale", ...values]),
			beginPath: () => calls.push(["beginPath"]),
			rect: (...values: number[]) => calls.push(["rect", ...values]),
			clip: () => calls.push(["clip"]),
		} as unknown as OffscreenCanvasRenderingContext2D;

		await renderMotionGraphicDsl({
			context,
			definition: {
				version: 1,
				width: 1920,
				height: 1080,
				durationSeconds: 5,
				nodes: [{
					id: "slot-mask",
					type: "group",
					x: 100,
					y: 200,
					width: 640,
					height: 180,
					anchorX: 0,
					anchorY: 0,
					clip: true,
				}],
			},
			localTime: 0.5,
			media: { resolve: async () => null },
		});

		const rect = calls.find(([name]) => name === "rect");
		expect(rect?.slice(1).map((value) => Math.abs(value))).toEqual([0, 0, 640, 180]);
		expect(calls).toContainEqual(["clip"]);
	});
});
