import { describe, expect, test } from "bun:test";
import type { TProject } from "@/project/types";
import type { MediaTime } from "@/wasm";
import { LocalCoreClient, LocalCoreError } from "../client";
import type { DomainProjectDocument } from "../project-adapter";
import { LocalProjectStore } from "../project-store";
import type {
	LocalCommitResponse,
	LocalCoreEvent,
	LocalEditTransaction,
	LocalProjectEnvelope,
	LocalProjectSummary,
	ManagedMediaUploadResponse,
} from "../types";

const ticks = (value: number) => value as MediaTime;

function project(): TProject {
	const now = new Date("2026-07-15T00:00:00.000Z");
	return {
		metadata: {
			id: "project-store-fixture",
			name: "Fixture",
			duration: ticks(0),
			createdAt: now,
			updatedAt: now,
		},
		scenes: [
			{
				id: "scene-main",
				name: "Main",
				isMain: true,
				tracks: {
					overlay: [],
					main: {
						id: "track-main",
						name: "Main",
						type: "video",
						elements: [],
						muted: false,
						hidden: false,
					},
					audio: [],
				},
				bookmarks: [],
				createdAt: now,
				updatedAt: now,
			},
		],
		currentSceneId: "scene-main",
		settings: {
			fps: { numerator: 30, denominator: 1 },
			canvasSize: { width: 1920, height: 1080 },
			background: { type: "color", color: "#000000" },
		},
		version: 1,
	};
}

function emptyDocument({
	projectId,
	name,
}: {
	projectId: string;
	name: string;
}) {
	return {
		schemaVersion: 1,
		id: projectId,
		name,
		settings: {
			fps: { numerator: 30, denominator: 1 },
			canvasSize: { width: 1920, height: 1080 },
			background: { type: "color" as const, color: "#000000" },
		},
		scenes: [],
		assets: [],
		transcripts: [],
		storySequences: [],
	};
}

class FakeLocalCoreClient extends LocalCoreClient {
	envelope: LocalProjectEnvelope<DomainProjectDocument> | null = null;
	createCalls = 0;
	commitBaseRevisions: number[] = [];
	uploadBaseRevisions: number[] = [];
	uploadIdempotencyKeys: string[] = [];
	eventHandler: ((event: LocalCoreEvent) => void) | null = null;

	constructor() {
		super({ baseUrl: "http://127.0.0.1:3210/api/v1" });
	}

	override async listProjects(): Promise<LocalProjectSummary[]> {
		return [];
	}

	override connectEvents({
		onEvent,
	}: {
		onEvent: (event: LocalCoreEvent) => void;
		onError?: (error: Event) => void;
	}): () => void {
		this.eventHandler = onEvent;
		return () => {
			this.eventHandler = null;
		};
	}

	override async readProject<TDocument>({
		projectId,
	}: {
		projectId: string;
	}): Promise<LocalProjectEnvelope<TDocument>> {
		if (!this.envelope || this.envelope.document.id !== projectId) {
			throw new LocalCoreError({
				message: "not found",
				code: "not_found",
				status: 404,
			});
		}
		return this.envelope as LocalProjectEnvelope<TDocument>;
	}

	override async createProject<TDocument>({
		name,
		projectId,
	}: {
		name: string;
		projectId: string;
		idempotencyKey: string;
	}): Promise<LocalCommitResponse<TDocument>> {
		this.createCalls += 1;
		this.envelope = {
			document: emptyDocument({ projectId, name }),
			revision: 0,
			documentHash: "created",
		};
		return {
			replayed: false,
			envelope: this.envelope as LocalProjectEnvelope<TDocument>,
		};
	}

	override async commitTransaction<TDocument, TOperation>({
		transaction,
	}: {
		projectId: string;
		transaction: LocalEditTransaction<TOperation>;
	}): Promise<LocalCommitResponse<TDocument>> {
		if (!this.envelope) throw new Error("project was not created");
		this.commitBaseRevisions.push(transaction.baseRevision);
		if (transaction.baseRevision !== this.envelope.revision) {
			throw new LocalCoreError({
				message: "revision conflict",
				code: "revisionConflict",
				status: 409,
				details: { currentRevision: this.envelope.revision },
			});
		}
		const current = this.envelope.document;
		const operations = transaction.operations as Array<Record<string, unknown>>;
		const sceneGraph = operations.find(
			(operation) => operation.type === "replaceSceneGraph",
		);
		const name = operations.find(
			(operation) => operation.type === "setProjectName",
		);
		const settings = operations.find(
			(operation) => operation.type === "setProjectSettings",
		);
		this.envelope = {
			document: {
				...current,
				...(sceneGraph
					? {
							scenes: sceneGraph.scenes as DomainProjectDocument["scenes"],
							currentSceneId: sceneGraph.currentSceneId as string,
						}
					: {}),
				...(name ? { name: name.name as string } : {}),
				...(settings
					? { settings: settings.settings as DomainProjectDocument["settings"] }
					: {}),
			},
			revision: this.envelope.revision + 1,
			documentHash: `revision-${this.envelope.revision + 1}`,
		};
		return {
			replayed: false,
			envelope: this.envelope as LocalProjectEnvelope<TDocument>,
		};
	}

	override async uploadManagedMedia<TDocument, TAsset>({
		projectId,
		expectedRevision,
		idempotencyKey,
		assetId,
		file,
		durationTicks,
		hasAudio,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
		assetId: string;
		file: File;
		durationTicks?: number;
		width?: number;
		height?: number;
		hasAudio?: boolean;
	}): Promise<ManagedMediaUploadResponse<TDocument, TAsset>> {
		if (!this.envelope || this.envelope.document.id !== projectId) {
			throw new Error("project was not created");
		}
		this.uploadBaseRevisions.push(expectedRevision);
		this.uploadIdempotencyKeys.push(idempotencyKey);
		if (expectedRevision !== this.envelope.revision) {
			throw new LocalCoreError({
				message: "revision conflict",
				code: "revisionConflict",
				status: 409,
			});
		}
		const asset = {
			id: assetId,
			name: file.name,
			kind: "audio" as const,
			contentHash: "fixture-content-hash",
			...(durationTicks === undefined ? {} : { durationTicks }),
			hasAudio: hasAudio ?? true,
			provenance: { type: "imported" as const },
		};
		const revision = this.envelope.revision + 1;
		this.envelope = {
			document: {
				...this.envelope.document,
				assets: [asset],
			},
			revision,
			documentHash: `revision-${revision}`,
		};
		return {
			asset: asset as TAsset,
			revision,
			replayed: false,
			commit: {
				replayed: false,
				envelope: this.envelope as LocalProjectEnvelope<TDocument>,
			},
		};
	}
}

describe("daemon-authoritative browser project store", () => {
	test("creates once and skips a semantically unchanged save", async () => {
		const client = new FakeLocalCoreClient();
		const store = new LocalProjectStore(client);
		const fixture = project();

		expect(await store.saveProject({ project: fixture })).toBe(1);
		expect(await store.saveProject({ project: fixture })).toBe(1);
		expect(client.createCalls).toBe(1);
		expect(client.commitBaseRevisions).toEqual([0]);
	});

	test("does not silently overwrite an external revision", async () => {
		const client = new FakeLocalCoreClient();
		const store = new LocalProjectStore(client);
		const fixture = project();
		await store.saveProject({ project: fixture });
		if (!client.envelope) throw new Error("fixture envelope is missing");
		client.envelope = { ...client.envelope, revision: 3 };
		store.invalidate({ projectId: fixture.metadata.id });

		await expect(store.saveProject({ project: fixture })).rejects.toMatchObject(
			{
				status: 409,
				code: "revisionConflict",
			},
		);
		expect(client.commitBaseRevisions).toEqual([0]);
	});

	test("notifies the editor shell when a newer external revision arrives", async () => {
		const client = new FakeLocalCoreClient();
		const store = new LocalProjectStore(client);
		const fixture = project();
		await store.saveProject({ project: fixture });
		const events: Array<{ projectId: string; revision: number }> = [];
		store.subscribeExternalRevisions((event) => events.push(event));
		store.startRevisionEvents();

		client.eventHandler?.({
			type: "revision.changed",
			projectId: fixture.metadata.id,
			revision: 1,
		});
		expect(events).toEqual([]);

		client.eventHandler?.({
			type: "revision.changed",
			projectId: fixture.metadata.id,
			revision: 2,
		});
		expect(events).toEqual([{ projectId: fixture.metadata.id, revision: 2 }]);
		await expect(store.saveProject({ project: fixture })).rejects.toMatchObject(
			{
				status: 409,
				code: "revisionConflict",
			},
		);
	});

	test("serializes media uploads behind project writes using the committed revision", async () => {
		const client = new FakeLocalCoreClient();
		const store = new LocalProjectStore(client);
		const fixture = project();
		const file = new File(["RIFF....WAVE"], "dialogue.wav", {
			type: "audio/wav",
			lastModified: 1,
		});

		const [revision, asset] = await Promise.all([
			store.saveProject({ project: fixture }),
			store.uploadManagedMedia({
				projectId: fixture.metadata.id,
				assetId: "dialogue",
				file,
				durationTicks: 48_000,
				hasAudio: true,
			}),
		]);

		expect(revision).toBe(1);
		expect(asset).toMatchObject({
			id: "dialogue",
			contentHash: "fixture-content-hash",
		});
		expect(client.commitBaseRevisions).toEqual([0]);
		expect(client.uploadBaseRevisions).toEqual([1]);
		expect(client.envelope?.revision).toBe(2);
	});
});
