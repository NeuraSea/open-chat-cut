import { describe, expect, test } from "bun:test";
import { resolveLocalCoreBaseUrl } from "../client";

describe("local core API URL resolution", () => {
	test("same-origin uses the browser origin for hosted deployments", () => {
		expect(
			resolveLocalCoreBaseUrl({
				value: "same-origin",
				browserOrigin: "https://cut.example.com",
			}),
		).toBe("https://cut.example.com/api/v1");
	});

	test("same-origin keeps a deterministic loopback URL during server rendering", () => {
		expect(resolveLocalCoreBaseUrl({ value: "same-origin" })).toBe(
			"http://127.0.0.1:3210/api/v1",
		);
	});

	test("portable Web ports do not change the fixed daemon port", () => {
		expect(
			resolveLocalCoreBaseUrl({
				value: "http://127.0.0.1:3210/api/v1",
				browserOrigin: "http://localhost:3111",
			}),
		).toBe("http://localhost:3210/api/v1");
	});

	test("explicit non-default provider URLs are preserved", () => {
		expect(
			resolveLocalCoreBaseUrl({
				value: "https://daemon.example.com/api/v1",
				browserOrigin: "https://cut.example.com",
			}),
		).toBe("https://daemon.example.com/api/v1");
	});
});
