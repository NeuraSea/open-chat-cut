import { describe, expect, test } from "bun:test";

import { SCENE_CLEAR_COLOR } from "../clear-color";

describe("buildFrameDescriptor", () => {
	test("leaves project pixels transparent for alpha exports", () => {
		expect(SCENE_CLEAR_COLOR).toEqual([0, 0, 0, 0]);
	});
});
