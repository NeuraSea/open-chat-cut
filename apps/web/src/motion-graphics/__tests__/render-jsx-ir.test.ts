import { describe, expect, test } from "bun:test";

import { evaluateMotionGraphicJsxIr } from "../render-jsx-ir";
import type { MotionGraphicJsxIr } from "../types";

const ir: MotionGraphicJsxIr = {
	version: 1,
	kind: "jsxSafeIr",
	width: 1_920,
	height: 1_080,
	durationSeconds: 2,
	fps: 30,
	program: {
		bindings: [
			{
				name: "frame",
				expression: { kind: "call", callee: "useCurrentFrame", arguments: [] },
			},
			{
				name: "opacity",
				expression: {
					kind: "call",
					callee: "interpolate",
					arguments: [
						{ kind: "identifier", name: "frame" },
						{
							kind: "array",
							items: [
								{ kind: "literal", value: 0 },
								{ kind: "literal", value: 30 },
							],
						},
						{
							kind: "array",
							items: [
								{ kind: "literal", value: 0 },
								{ kind: "literal", value: 1 },
							],
						},
					],
				},
			},
		],
		root: {
			kind: "element",
			tag: "AbsoluteFill",
			attributes: [
				[
					"style",
					{
						kind: "object",
						entries: [
							["backgroundColor", { kind: "literal", value: "#101828" }],
							["opacity", { kind: "identifier", name: "opacity" }],
						],
					},
				],
			],
			children: [{ kind: "text", value: "OpenChatCut" }],
		},
	},
};

describe("advanced motion graphic safe IR", () => {
	test("evaluates deterministic frame-bound bindings without executing source", () => {
		const first = evaluateMotionGraphicJsxIr(ir, 0);
		const middle = evaluateMotionGraphicJsxIr(ir, 0.5);
		expect(first[0]).toMatchObject({
			tag: "AbsoluteFill",
			props: { style: { backgroundColor: "#101828", opacity: 0 } },
			children: ["OpenChatCut"],
		});
		expect(middle[0]).toMatchObject({ props: { style: { opacity: 0.5 } } });
	});

	test("rejects prototype-chain access even in imported project IR", () => {
		const unsafe = structuredClone(ir);
		unsafe.program.bindings[0].expression = {
			kind: "member",
			object: { kind: "object", entries: [] },
			property: "constructor",
		};
		expect(() => evaluateMotionGraphicJsxIr(unsafe, 0)).toThrow(
			"forbidden property",
		);
	});
});
