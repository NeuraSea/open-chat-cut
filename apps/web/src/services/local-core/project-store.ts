import type { TProject } from "@/project/types";
import { LocalCoreError, type LocalCoreClient } from "./client";
import { localCoreClient } from "./client";
import {
	fromDomainProjectEnvelope,
	toDomainProjectDocument,
	type DomainProjectDocument,
} from "./project-adapter";
import type {
	LocalEditTransaction,
	LocalProjectEnvelope,
	LocalProjectSummary,
} from "./types";

function newRequestId(): string {
	return crypto.randomUUID();
}

function isNotFound(error: unknown): boolean {
	return error instanceof LocalCoreError && error.status === 404;
}

async function retryTransportOnce<T>(operation: () => Promise<T>): Promise<T> {
	try {
		return await operation();
	} catch (error) {
		// Structured daemon responses are definitive. A fetch/network failure is
		// ambiguous, so retry the exact same idempotency key once.
		if (error instanceof LocalCoreError) throw error;
		return operation();
	}
}

/**
 * Browser facade over the daemon project store.
 *
 * The revision cache advances only after reading or committing a full envelope.
 * An SSE revision notification cannot advance it: doing so without the matching
 * document would let a stale browser snapshot overwrite a Codex transaction.
 */
export class LocalProjectStore {
	private readonly revisions = new Map<string, number>();
	private readonly creationKeys = new Map<string, string>();
	private readonly documents = new Map<string, DomainProjectDocument>();
	private readonly staleProjects = new Set<string>();
	private revisionEventsDispose: (() => void) | null = null;
	private readonly writeQueues = new Map<string, Promise<void>>();
	private readonly pendingWrites = new Map<
		string,
		{ documentHash: string; transaction: LocalEditTransaction<unknown> }
	>();
	private readonly externalRevisionListeners = new Set<
		(event: { projectId: string; revision: number }) => void
	>();
	private readonly client: LocalCoreClient;

	constructor(client: LocalCoreClient = localCoreClient) {
		this.client = client;
	}

	startRevisionEvents(): void {
		if (this.revisionEventsDispose) return;
		this.revisionEventsDispose = this.client.connectEvents({
			onEvent: (event) => {
				if (event.type === "revision.changed") {
					const cachedRevision = this.revisions.get(event.projectId);
					if (cachedRevision === undefined || event.revision > cachedRevision) {
						const localWriteInFlight =
							this.pendingWrites.has(event.projectId) ||
							this.writeQueues.has(event.projectId);
						this.invalidate({ projectId: event.projectId });
						if (!localWriteInFlight) {
							for (const listener of this.externalRevisionListeners) {
								listener({
									projectId: event.projectId,
									revision: event.revision,
								});
							}
						}
					}
				}
			},
		});
	}

	subscribeExternalRevisions(
		listener: (event: { projectId: string; revision: number }) => void,
	): () => void {
		this.externalRevisionListeners.add(listener);
		return () => this.externalRevisionListeners.delete(listener);
	}

	async listProjects(): Promise<LocalProjectSummary[]> {
		return this.client.listProjects();
	}

	async readProject({ projectId }: { projectId: string }): Promise<TProject> {
		const envelope = await this.client.readProject<DomainProjectDocument>({
			projectId,
		});
		this.rememberEnvelope({ envelope });
		return fromDomainProjectEnvelope({ envelope });
	}

	async saveProject({ project }: { project: TProject }): Promise<number> {
		return this.enqueueProjectWrite({
			projectId: project.metadata.id,
			operation: () => this.saveProjectNow({ project }),
		});
	}

	private async saveProjectNow({
		project,
	}: {
		project: TProject;
	}): Promise<number> {
		const projectId = project.metadata.id;
		if (this.staleProjects.has(projectId)) {
			throw new LocalCoreError({
				message:
					"The project changed outside this editor; reload before saving",
				code: "revisionConflict",
				status: 409,
				details: { reason: "external_revision_changed" },
			});
		}
		const baseRevision = await this.resolveRevision({ project });
		const nextDocument = toDomainProjectDocument({ project });
		const previousDocument = this.documents.get(projectId);
		if (previousDocument) {
			// The Classic model cannot represent daemon-only collections (unplaced
			// generated assets, transcripts, and StorySequences). Preserve them while
			// translating the browser's scene edits into semantic operations.
			nextDocument.assets = mergeAssets({
				previous: previousDocument,
				next: nextDocument,
			});
			nextDocument.transcripts = mergeById({
				previous: previousDocument.transcripts,
				next: nextDocument.transcripts,
			});
			nextDocument.storySequences = nextDocument.storySequences.length
				? nextDocument.storySequences
				: previousDocument.storySequences;
		}
		const documentHash = canonicalDocumentHash(nextDocument);
		if (
			previousDocument &&
			canonicalDocumentHash(previousDocument) === documentHash
		) {
			// UI-only state such as regenerated thumbnails can notify Classic
			// managers without changing the versioned daemon document. Do not
			// manufacture a revision for a semantic no-op.
			return baseRevision;
		}
		const pending = this.pendingWrites.get(projectId);
		const requestId = newRequestId();
		const transaction: LocalEditTransaction<unknown> =
			pending?.documentHash === documentHash
				? pending.transaction
				: {
						transactionId: requestId,
						projectId,
						baseRevision,
						idempotencyKey: requestId,
						actor: {
							kind: "user",
							id: "web-editor",
							displayName: "Web editor",
						},
						operations: [
							{
								type: "replaceSceneGraph",
								scenes: nextDocument.scenes,
								...(nextDocument.currentSceneId
									? { currentSceneId: nextDocument.currentSceneId }
									: {}),
							},
							{ type: "setProjectName", name: nextDocument.name },
							{ type: "setProjectSettings", settings: nextDocument.settings },
						],
					};
		this.pendingWrites.set(projectId, { documentHash, transaction });

		let result: import("./types").LocalCommitResponse<DomainProjectDocument>;
		try {
			result = await retryTransportOnce(() =>
				this.client.commitTransaction<DomainProjectDocument, unknown>({
					projectId,
					transaction,
				}),
			);
		} catch (error) {
			if (error instanceof LocalCoreError) this.pendingWrites.delete(projectId);
			throw error;
		}
		this.pendingWrites.delete(projectId);
		this.rememberEnvelope({ envelope: result.envelope });
		return result.envelope.revision;
	}

	async uploadManagedMedia({
		projectId,
		assetId,
		file,
		durationTicks,
		width,
		height,
		hasAudio,
	}: {
		projectId: string;
		assetId: string;
		file: File;
		durationTicks?: number;
		width?: number;
		height?: number;
		hasAudio?: boolean;
	}): Promise<DomainProjectDocument["assets"][number]> {
		return this.enqueueProjectWrite({
			projectId,
			operation: async () => {
				const expectedRevision = await this.resolveExistingRevision({
					projectId,
				});
				const idempotencyKey = newRequestId();
				const result = await retryTransportOnce(() =>
					this.client.uploadManagedMedia<
						DomainProjectDocument,
						DomainProjectDocument["assets"][number]
					>({
						projectId,
						expectedRevision,
						idempotencyKey,
						assetId,
						file,
						durationTicks,
						width,
						height,
						hasAudio,
					}),
				);
				this.rememberEnvelope({ envelope: result.commit.envelope });
				return result.asset;
			},
		});
	}

	async listManagedAssets({
		projectId,
	}: {
		projectId: string;
	}): Promise<DomainProjectDocument["assets"]> {
		const envelope = await this.client.readProject<DomainProjectDocument>({
			projectId,
		});
		this.rememberEnvelope({ envelope });
		return envelope.document.assets.filter(
			(asset) =>
				typeof asset.contentHash === "string" ||
				(typeof asset.linkedFile?.fingerprintSha256 === "string" &&
					asset.linkedFile.portable === false),
		);
	}

	downloadManagedMedia({
		projectId,
		assetId,
	}: {
		projectId: string;
		assetId: string;
	}): Promise<Blob> {
		return this.client.downloadManagedMedia({ projectId, assetId });
	}

	downloadMediaDerivative({
		projectId,
		assetId,
		kind,
	}: {
		projectId: string;
		assetId: string;
		kind: "thumbnail" | "waveform" | "proxy" | "audio";
	}): Promise<Blob> {
		return this.client.downloadMediaDerivative({ projectId, assetId, kind });
	}

	async removeManagedMedia({
		projectId,
		assetId,
	}: {
		projectId: string;
		assetId: string;
	}): Promise<void> {
		return this.enqueueProjectWrite({
			projectId,
			operation: async () => {
				const expectedRevision = await this.resolveExistingRevision({
					projectId,
				});
				const document = this.documents.get(projectId);
				if (!document) throw new Error("Project document is unavailable");
				const operations: Array<Record<string, unknown>> = [];
				for (const scene of document.scenes) {
					for (const track of scene.tracks) {
						for (const item of track.items) {
							if (
								item.content.type === "media" &&
								item.content.assetId === assetId
							) {
								operations.push({ type: "removeItem", itemId: item.id });
							}
						}
					}
				}
				operations.push({ type: "removeAsset", assetId });
				const requestId = newRequestId();
				const result = await retryTransportOnce(() =>
					this.client.commitTransaction<
						DomainProjectDocument,
						Record<string, unknown>
					>({
						projectId,
						transaction: {
							transactionId: requestId,
							projectId,
							baseRevision: expectedRevision,
							idempotencyKey: requestId,
							actor: {
								kind: "user",
								id: "web-editor",
								displayName: "Web editor",
							},
							operations,
						},
					}),
				);
				this.rememberEnvelope({ envelope: result.envelope });
			},
		});
	}

	async deleteProject({ projectId }: { projectId: string }): Promise<void> {
		return this.enqueueProjectWrite({
			projectId,
			operation: () => this.deleteProjectNow({ projectId }),
		});
	}

	private async deleteProjectNow({
		projectId,
	}: {
		projectId: string;
	}): Promise<void> {
		const expectedRevision = await this.resolveExistingRevision({ projectId });
		const idempotencyKey = newRequestId();
		await retryTransportOnce(() =>
			this.client.deleteProject({
				projectId,
				expectedRevision,
				idempotencyKey,
			}),
		);
		this.revisions.delete(projectId);
		this.creationKeys.delete(projectId);
		this.documents.delete(projectId);
		this.staleProjects.delete(projectId);
		this.pendingWrites.delete(projectId);
	}

	invalidate({ projectId }: { projectId: string }): void {
		this.revisions.delete(projectId);
		this.staleProjects.add(projectId);
	}

	private async resolveRevision({
		project,
	}: {
		project: TProject;
	}): Promise<number> {
		const cached = this.revisions.get(project.metadata.id);
		if (cached !== undefined) return cached;

		try {
			const envelope = await this.client.readProject<DomainProjectDocument>({
				projectId: project.metadata.id,
			});
			this.rememberEnvelope({ envelope });
			return envelope.revision;
		} catch (error) {
			if (!isNotFound(error)) throw error;
		}

		const idempotencyKey =
			this.creationKeys.get(project.metadata.id) ?? newRequestId();
		this.creationKeys.set(project.metadata.id, idempotencyKey);
		const created = await retryTransportOnce(() =>
			this.client.createProject<DomainProjectDocument>({
				name: project.metadata.name,
				projectId: project.metadata.id,
				idempotencyKey,
			}),
		);
		this.rememberEnvelope({ envelope: created.envelope });
		return created.envelope.revision;
	}

	private async resolveExistingRevision({
		projectId,
	}: {
		projectId: string;
	}): Promise<number> {
		const cached = this.revisions.get(projectId);
		if (cached !== undefined) return cached;
		const envelope = await this.client.readProject<DomainProjectDocument>({
			projectId,
		});
		this.rememberEnvelope({ envelope });
		return envelope.revision;
	}

	private rememberEnvelope({
		envelope,
	}: {
		envelope: LocalProjectEnvelope<DomainProjectDocument>;
	}): void {
		this.revisions.set(envelope.document.id, envelope.revision);
		this.documents.set(envelope.document.id, envelope.document);
		this.staleProjects.delete(envelope.document.id);
	}

	private async enqueueProjectWrite<T>({
		projectId,
		operation,
	}: {
		projectId: string;
		operation: () => Promise<T>;
	}): Promise<T> {
		const previous = this.writeQueues.get(projectId) ?? Promise.resolve();
		let release: (() => void) | undefined;
		const gate = new Promise<void>((resolve) => {
			release = resolve;
		});
		const queued = previous.catch(() => undefined).then(() => gate);
		this.writeQueues.set(projectId, queued);
		await previous.catch(() => undefined);
		try {
			return await operation();
		} finally {
			release?.();
			if (this.writeQueues.get(projectId) === queued) {
				this.writeQueues.delete(projectId);
			}
		}
	}
}

function mergeById<T extends { id: string }>({
	previous,
	next,
}: {
	previous: T[];
	next: T[];
}): T[] {
	const merged = new Map(previous.map((item) => [item.id, item]));
	for (const item of next) merged.set(item.id, item);
	return [...merged.values()];
}

/**
 * Compare daemon envelopes independently of object insertion order. The
 * Classic adapter and serde can legitimately enumerate the same fields in a
 * different order; using JSON.stringify directly would turn that harmless
 * representation difference into a new revision on every autosave.
 */
function canonicalJson(value: unknown): string {
	if (Array.isArray(value)) {
		return `[${value.map((entry) => canonicalJson(entry)).join(",")}]`;
	}
	if (value && typeof value === "object") {
		const entries = Object.entries(value as Record<string, unknown>)
			.filter(([, entry]) => entry !== undefined)
			.sort(([left], [right]) => left.localeCompare(right));
		return `{${entries
			.map(([key, entry]) => `${JSON.stringify(key)}:${canonicalJson(entry)}`)
			.join(",")}}`;
	}
	return JSON.stringify(value);
}

// These compatibility snapshots are retained so Classic can round-trip the
// project, but they are not part of the daemon's semantic document. Classic
// may recreate them on every save (or omit them after a server transaction),
// and treating that representation churn as an edit would create phantom
// revisions.
const NON_SEMANTIC_DOCUMENT_KEYS = new Set([
	"classicMetadata",
	"classicProject",
	"classicSettings",
]);

function canonicalDocumentHash(document: DomainProjectDocument): string {
	return canonicalJson(stripNonSemanticDocumentFields(document));
}

function stripNonSemanticDocumentFields(value: unknown): unknown {
	if (Array.isArray(value)) {
		return value.map(stripNonSemanticDocumentFields);
	}
	if (value && typeof value === "object") {
		return Object.fromEntries(
			Object.entries(value as Record<string, unknown>)
				.filter(([key]) => !NON_SEMANTIC_DOCUMENT_KEYS.has(key))
				.map(([key, entry]) => [key, stripNonSemanticDocumentFields(entry)]),
		);
	}
	return value;
}

function referencedAssetIds(document: DomainProjectDocument): Set<string> {
	const ids = new Set<string>();
	for (const scene of document.scenes) {
		for (const track of scene.tracks) {
			for (const item of track.items) {
				if (item.content.type === "media") ids.add(item.content.assetId);
			}
		}
	}
	return ids;
}

function mergeAssets({
	previous,
	next,
}: {
	previous: DomainProjectDocument;
	next: DomainProjectDocument;
}): DomainProjectDocument["assets"] {
	const previousReferenced = referencedAssetIds(previous);
	const nextReferenced = referencedAssetIds(next);
	const merged = new Map(next.assets.map((asset) => [asset.id, asset]));
	for (const asset of previous.assets) {
		// Assets previously placed in the Classic timeline follow explicit user
		// removal. Assets never represented by Classic remain daemon-owned and are
		// retained for Codex/jobs/export.
		if (!previousReferenced.has(asset.id)) {
			merged.set(asset.id, asset);
		} else if (nextReferenced.has(asset.id)) {
			const current = merged.get(asset.id);
			merged.set(asset.id, current ? { ...asset, ...current } : asset);
		}
	}
	return [...merged.values()];
}

export const localProjectStore = new LocalProjectStore();
