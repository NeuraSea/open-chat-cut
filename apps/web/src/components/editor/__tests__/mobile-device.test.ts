import { describe, expect, test } from "bun:test";
import { isMobileDevice } from "../mobile-device";

describe("isMobileDevice", () => {
	test("does not classify a narrow desktop browser as mobile", () => {
		expect(
			isMobileDevice({
				userAgent:
					"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/138 Safari/537.36",
				platform: "MacIntel",
				maxTouchPoints: 0,
				userAgentDataMobile: false,
			}),
		).toBe(false);
	});

	test("recognizes a mobile browser", () => {
		expect(
			isMobileDevice({
				userAgent:
					"Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 Mobile Safari/537.36",
				platform: "Linux armv8l",
				maxTouchPoints: 5,
				userAgentDataMobile: true,
			}),
		).toBe(true);
	});

	test("recognizes iPadOS using a desktop user agent", () => {
		expect(
			isMobileDevice({
				userAgent:
					"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15",
				platform: "MacIntel",
				maxTouchPoints: 5,
			}),
		).toBe(true);
	});
});
