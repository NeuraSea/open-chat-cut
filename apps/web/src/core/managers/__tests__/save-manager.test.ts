import assert from "node:assert/strict";
import { describe, test } from "node:test";

import type { EditorCore } from "@/core";
import { SaveManager } from "../save-manager";

function deferred() {
	let resolve!: () => void;
	const promise = new Promise<void>((nextResolve) => {
		resolve = nextResolve;
	});
	return { promise, resolve };
}

async function waitFor(predicate: () => boolean): Promise<void> {
	for (let attempt = 0; attempt < 100; attempt += 1) {
		if (predicate()) return;
		await new Promise((resolve) => setTimeout(resolve, 1));
	}
	throw new Error("condition was not reached");
}

describe("SaveManager durability barriers", () => {
	test("flush waits for an in-flight save and persists edits made during it", async () => {
		const saves: ReturnType<typeof deferred>[] = [];
		// This focused fixture implements only the collaborators SaveManager owns.
		// eslint-disable-next-line @typescript-eslint/no-unsafe-type-assertion
		const editor = {
			project: {
				getActive: () => ({ metadata: { id: "project" } }),
				getIsLoading: () => false,
				getMigrationState: () => ({ isMigrating: false }),
				saveCurrentProject: () => {
					const save = deferred();
					saves.push(save);
					return save.promise;
				},
			},
		} as unknown as EditorCore;
		const manager = new SaveManager({ editor, debounceMs: 0 });

		manager.markDirty();
		await waitFor(() => saves.length === 1);

		// This edit arrives after the first snapshot was handed to storage.
		manager.markDirty();
		const flushed = manager.flush();
		saves[0].resolve();

		await waitFor(() => saves.length === 2);
		let finished = false;
		void flushed.then(() => {
			finished = true;
		});
		await new Promise((resolve) => setTimeout(resolve, 1));
		assert.equal(finished, false);

		saves[1].resolve();
		await flushed;
		assert.equal(manager.getIsDirty(), false);
		assert.equal(saves.length, 2);
	});
});
