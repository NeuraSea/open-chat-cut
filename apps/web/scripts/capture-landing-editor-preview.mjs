#!/usr/bin/env node

import { writeFile } from "node:fs/promises";

function readArgument(name, fallback) {
	const index = process.argv.indexOf(name);
	return index === -1 ? fallback : process.argv[index + 1];
}

const cdpUrl = readArgument("--cdp-url", "http://127.0.0.1:9229");
const editorUrl = readArgument("--editor-url");
const output = readArgument(
	"--output",
	"public/landing/openchatcut-editor-fixture.png",
);

if (!editorUrl) {
	throw new Error(
		"--editor-url is required and must point to a deterministic fixture project",
	);
}

const targetsResponse = await fetch(`${cdpUrl}/json`);
if (!targetsResponse.ok) {
	throw new Error(`Cannot read Chrome targets from ${cdpUrl}`);
}
const targets = await targetsResponse.json();
const pageTarget = targets.find((target) => target.type === "page");
if (!pageTarget) throw new Error("Chrome has no page target");

const socket = new WebSocket(pageTarget.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
	socket.addEventListener("open", resolve, { once: true });
	socket.addEventListener("error", reject, { once: true });
});

let messageId = 0;
const pending = new Map();

socket.addEventListener("message", (event) => {
	const message = JSON.parse(event.data);
	const request = pending.get(message.id);
	if (!request) return;
	pending.delete(message.id);
	if (message.error) request.reject(new Error(JSON.stringify(message.error)));
	else request.resolve(message.result);
});

function call(method, params = {}) {
	return new Promise((resolve, reject) => {
		messageId += 1;
		pending.set(messageId, { resolve, reject });
		socket.send(JSON.stringify({ id: messageId, method, params }));
	});
}

await call("Emulation.setDeviceMetricsOverride", {
	width: 1600,
	height: 1000,
	deviceScaleFactor: 1,
	mobile: false,
});
await call("Page.enable");
await call("Page.addScriptToEvaluateOnNewDocument", {
	source: `
		try {
			localStorage.setItem("hasSeenOnboarding", "true");
			localStorage.setItem("theme", "dark");
		} catch {}
	`,
});
await call("Page.navigate", { url: editorUrl });

const deadline = Date.now() + 30_000;
let editorReady = false;
while (Date.now() < deadline) {
	const state = await call("Runtime.evaluate", {
		expression: `JSON.stringify({
			ready: document.body.innerText.includes("Export") &&
				document.body.innerText.includes("Main"),
			dialog: Boolean(document.querySelector('[role="dialog"]')),
		})`,
		returnByValue: true,
	});
	const value = JSON.parse(state.result.value);
	if (value.ready && !value.dialog) {
		editorReady = true;
		break;
	}
	await new Promise((resolve) => setTimeout(resolve, 250));
}

if (!editorReady) {
	throw new Error("The editor fixture did not become ready within 30 seconds");
}

await new Promise((resolve) => setTimeout(resolve, 1_000));
const screenshot = await call("Page.captureScreenshot", {
	format: "png",
	captureBeyondViewport: false,
	fromSurface: true,
});
await writeFile(output, Buffer.from(screenshot.data, "base64"));
socket.close();

console.log(`Captured real editor UI to ${output}`);
