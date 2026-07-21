import type { TProject, TProjectMetadata } from "@/project/types";
import { getProjectDurationFromScenes } from "@/timeline/scenes";
import type { MediaAsset } from "@/media/types";
import { IndexedDBAdapter } from "./indexeddb-adapter";
import { OPFSAdapter } from "./opfs-adapter";
import {
	type StorageCapacityCheckResult,
	evaluateStorageCapacity,
	isStorageQuotaExceededError,
	readStorageQuotaStatus,
} from "./quota";
import type {
	MediaAssetData,
	StorageConfig,
	SerializedProject,
	SerializedScene,
} from "./types";
import type { SavedSoundsData, SavedSound, SoundEffect } from "@/sounds/types";
import {
	migrations,
	runStorageMigrations,
} from "@/services/storage/migrations";
import type { Bookmark, SceneTracks } from "@/timeline";
import {
	mediaTimeFromSeconds,
	mediaTimeToSeconds,
	roundMediaTime,
} from "@/wasm";
import { LocalCoreError, localProjectStore } from "@/services/local-core";
import type { DomainAsset } from "@/services/local-core/project-adapter";

function normalizeBookmarks({ raw }: { raw: unknown }): Bookmark[] {
	if (!Array.isArray(raw)) return [];
	return raw
		.map((item): Bookmark | null => {
			if (typeof item === "number") {
				return { time: roundMediaTime({ time: item }) };
			}
			if (!isRecord(item)) return null;
			const obj = item;
			if (
				typeof obj !== "object" ||
				obj === null ||
				typeof obj.time !== "number"
			) {
				return null;
			}
			return {
				time: roundMediaTime({ time: obj.time }),
				...(typeof obj.note === "string" && { note: obj.note }),
				...(typeof obj.color === "string" && { color: obj.color }),
				...(typeof obj.duration === "number" && {
					duration: roundMediaTime({ time: obj.duration }),
				}),
			};
		})
		.filter((b): b is Bookmark => b !== null);
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function daemonTimestamp({
	value,
	fallback,
}: {
	value: string;
	fallback: Date;
}): Date {
	const parsed = new Date(value);
	return Number.isNaN(parsed.getTime()) ? fallback : parsed;
}

type BrowserLocalMediaAsset = DomainAsset & {
	kind: MediaAsset["type"];
};

function isBrowserMediaAsset(
	asset: DomainAsset,
): asset is BrowserLocalMediaAsset {
	return (
		["image", "video", "audio"].includes(asset.kind) &&
		(typeof asset.contentHash === "string" ||
			(typeof asset.linkedFile?.fingerprintSha256 === "string" &&
				asset.linkedFile.portable === false))
	);
}

function browserContentIdentity(asset: BrowserLocalMediaAsset): string {
	return asset.contentHash ?? `linked:${asset.linkedFile?.fingerprintSha256 ?? "invalid"}`;
}

class StorageService {
	private projectsAdapter: IndexedDBAdapter<SerializedProject>;
	private savedSoundsAdapter: IndexedDBAdapter<SavedSoundsData>;
	private config: StorageConfig;
	private migrationsPromise: Promise<void> | null = null;
	private readonly managedProjectMarkerKey =
		"openchatcut.daemon-managed-projects.v1";
	private readonly managedProjectIds = new Set<string>(
		this.readManagedProjectIds(),
	);

	private readManagedProjectIds(): string[] {
		try {
			const raw = globalThis.localStorage?.getItem(
				this.managedProjectMarkerKey,
			);
			const parsed = raw ? JSON.parse(raw) : [];
			return Array.isArray(parsed)
				? parsed.filter((id): id is string => typeof id === "string")
				: [];
		} catch {
			return [];
		}
	}

	private markDaemonManaged(id: string): void {
		this.managedProjectIds.add(id);
		try {
			globalThis.localStorage?.setItem(
				this.managedProjectMarkerKey,
				JSON.stringify([...this.managedProjectIds].sort()),
			);
		} catch {
			// The marker is defense-in-depth. Daemon state remains authoritative.
		}
	}

	constructor() {
		if (typeof window !== "undefined") localProjectStore.startRevisionEvents();
		this.config = {
			projectsDb: "video-editor-projects",
			mediaDb: "video-editor-media",
			savedSoundsDb: "video-editor-saved-sounds",
			version: 1,
		};

		this.projectsAdapter = new IndexedDBAdapter<SerializedProject>({
			dbName: this.config.projectsDb,
			storeName: "projects",
			version: this.config.version,
		});

		this.savedSoundsAdapter = new IndexedDBAdapter<SavedSoundsData>({
			dbName: this.config.savedSoundsDb,
			storeName: "saved-sounds",
			version: this.config.version,
		});
	}

	private async ensureMigrations(): Promise<void> {
		if (this.migrationsPromise) {
			await this.migrationsPromise;
			return;
		}

		this.migrationsPromise = runStorageMigrations({ migrations }).then(
			() => undefined,
		);
		await this.migrationsPromise;
	}

	private getProjectMediaAdapters({ projectId }: { projectId: string }) {
		const mediaMetadataAdapter = new IndexedDBAdapter<MediaAssetData>({
			dbName: `${this.config.mediaDb}-${projectId}`,
			storeName: "media-metadata",
			version: this.config.version,
		});

		const mediaAssetsAdapter = new OPFSAdapter(`media-files-${projectId}`);

		return { mediaMetadataAdapter, mediaAssetsAdapter };
	}

	async canStoreFile({
		size,
	}: {
		size: number;
	}): Promise<StorageCapacityCheckResult> {
		const quotaStatus = await readStorageQuotaStatus();
		return evaluateStorageCapacity({
			requiredBytes: size,
			quotaStatus,
		});
	}

	isQuotaExceededError({ error }: { error: unknown }): boolean {
		return isStorageQuotaExceededError({ error });
	}

	private stripAudioBuffers({ tracks }: { tracks: SceneTracks }): SceneTracks {
		return {
			...tracks,
			audio: tracks.audio.map((track) => ({
				...track,
				elements: track.elements.map((element) => {
					const { buffer: _buffer, ...rest } = element;
					return rest;
				}),
			})),
		};
	}

	private serializeProject({
		project,
	}: {
		project: TProject;
	}): SerializedProject {
		const duration =
			project.metadata.duration ??
			getProjectDurationFromScenes({ scenes: project.scenes });
		const serializedScenes: SerializedScene[] = project.scenes.map((scene) => ({
			id: scene.id,
			name: scene.name,
			isMain: scene.isMain,
			tracks: this.stripAudioBuffers({ tracks: scene.tracks }),
			bookmarks: scene.bookmarks,
			createdAt: scene.createdAt.toISOString(),
			updatedAt: scene.updatedAt.toISOString(),
		}));

		return {
			metadata: {
				id: project.metadata.id,
				name: project.metadata.name,
				thumbnail: project.metadata.thumbnail,
				duration,
				createdAt: project.metadata.createdAt.toISOString(),
				updatedAt: project.metadata.updatedAt.toISOString(),
			},
			scenes: serializedScenes,
			currentSceneId: project.currentSceneId,
			settings: project.settings,
			version: project.version,
			timelineViewState: project.timelineViewState,
		};
	}

	private deserializeProject({
		id,
		serializedProject,
	}: {
		id: string;
		serializedProject: SerializedProject;
	}): TProject | null {
		if (
			typeof serializedProject !== "object" ||
			serializedProject === null ||
			typeof serializedProject.metadata !== "object" ||
			serializedProject.metadata === null
		) {
			console.warn(
				"[storage] Skipping malformed project entry (missing metadata):",
				{ id, entry: serializedProject },
			);
			return null;
		}

		const scenes =
			serializedProject.scenes?.map((scene) => ({
				id: scene.id,
				name: scene.name,
				isMain: scene.isMain,
				tracks: scene.tracks,
				bookmarks: normalizeBookmarks({ raw: scene.bookmarks }),
				createdAt: new Date(scene.createdAt),
				updatedAt: new Date(scene.updatedAt),
			})) ?? [];

		return {
			metadata: {
				id: serializedProject.metadata.id,
				name: serializedProject.metadata.name,
				thumbnail: serializedProject.metadata.thumbnail,
				duration: roundMediaTime({
					time:
						serializedProject.metadata.duration ??
						getProjectDurationFromScenes({ scenes }),
				}),
				createdAt: new Date(serializedProject.metadata.createdAt),
				updatedAt: new Date(serializedProject.metadata.updatedAt),
			},
			scenes,
			currentSceneId: serializedProject.currentSceneId || "",
			settings: serializedProject.settings,
			version: serializedProject.version,
			timelineViewState: serializedProject.timelineViewState,
		};
	}

	private async loadCachedProject({
		id,
	}: {
		id: string;
	}): Promise<TProject | null> {
		await this.ensureMigrations();
		const serializedProject = await this.projectsAdapter.get(id);
		if (!serializedProject) return null;
		return this.deserializeProject({ id, serializedProject });
	}

	private async cacheProject({
		project,
	}: {
		project: TProject;
	}): Promise<void> {
		await this.projectsAdapter.set({
			key: project.metadata.id,
			value: this.serializeProject({ project }),
		});
	}

	async saveProject({ project }: { project: TProject }): Promise<void> {
		// The daemon commit is authoritative. IndexedDB is written only after CAS
		// succeeds, so a stale browser never becomes a second source of truth.
		await localProjectStore.saveProject({ project });
		this.markDaemonManaged(project.metadata.id);
		try {
			await this.cacheProject({ project });
		} catch (error) {
			console.warn(
				"[storage] daemon commit succeeded but browser cache update failed",
				error,
			);
		}
	}

	async loadProject({
		id,
	}: {
		id: string;
	}): Promise<{ project: TProject } | null> {
		try {
			const project = await localProjectStore.readProject({ projectId: id });
			this.markDaemonManaged(id);
			try {
				await this.cacheProject({ project });
			} catch (error) {
				console.warn(
					"[storage] daemon read succeeded but browser cache update failed",
					error,
				);
			}
			return { project };
		} catch (error) {
			if (!(error instanceof LocalCoreError) || error.status !== 404) {
				throw error;
			}
		}

		// Classic IndexedDB projects are copied into the daemon on first access.
		// The original cache remains intact, making migration non-destructive.
		if (this.managedProjectIds.has(id)) return null;
		const legacyProject = await this.loadCachedProject({ id });
		if (!legacyProject) return null;
		await localProjectStore.saveProject({ project: legacyProject });
		this.markDaemonManaged(id);
		try {
			await this.cacheProject({ project: legacyProject });
		} catch (error) {
			console.warn(
				"[storage] legacy migration committed but cache update failed",
				error,
			);
		}
		return { project: legacyProject };
	}

	async loadAllProjects(): Promise<TProject[]> {
		await this.ensureMigrations();
		const [daemonProjects, cachedProjectIds] = await Promise.all([
			localProjectStore.listProjects(),
			this.projectsAdapter.list(),
		]);
		for (const project of daemonProjects) this.markDaemonManaged(project.id);
		const projectIds = Array.from(
			new Set([
				...daemonProjects.map((project) => project.id),
				...cachedProjectIds,
			]),
		);
		const projects: TProject[] = [];

		for (const id of projectIds) {
			const result = await this.loadProject({ id });
			if (result?.project) {
				projects.push(result.project);
			}
		}

		return projects.sort(
			(a, b) => b.metadata.updatedAt.getTime() - a.metadata.updatedAt.getTime(),
		);
	}

	async loadAllProjectsMetadata(): Promise<TProjectMetadata[]> {
		await this.ensureMigrations();
		const daemonProjects = await localProjectStore.listProjects();
		const daemonIds = new Set(daemonProjects.map((project) => project.id));
		const daemonMetadata = await Promise.all(
			daemonProjects.map(async (summary) => {
				const project = await localProjectStore.readProject({
					projectId: summary.id,
				});
				const metadata = {
					...project.metadata,
					createdAt: daemonTimestamp({
						value: summary.createdAt,
						fallback: project.metadata.createdAt,
					}),
					updatedAt: daemonTimestamp({
						value: summary.updatedAt,
						fallback: project.metadata.updatedAt,
					}),
				};
				const projectWithPersistenceTimestamps = { ...project, metadata };
				this.markDaemonManaged(summary.id);
				try {
					await this.cacheProject({
						project: projectWithPersistenceTimestamps,
					});
				} catch (error) {
					console.warn("[storage] metadata read cache update failed", error);
				}
				return metadata;
			}),
		);

		const legacyMetadata = (await this.projectsAdapter.getAll()).flatMap(
			(serializedProject) => {
				const id = serializedProject.metadata?.id;
				if (!id || daemonIds.has(id) || this.managedProjectIds.has(id))
					return [];
				const project = this.deserializeProject({ id, serializedProject });
				return project ? [project.metadata] : [];
			},
		);

		return [...daemonMetadata, ...legacyMetadata].sort(
			(a, b) => b.updatedAt.getTime() - a.updatedAt.getTime(),
		);
	}

	async deleteProject({ id }: { id: string }): Promise<void> {
		try {
			await localProjectStore.deleteProject({ projectId: id });
		} catch (error) {
			if (!(error instanceof LocalCoreError) || error.status !== 404) {
				throw error;
			}
		}
		await this.projectsAdapter.remove(id);
		// Keep the managed marker as a tombstone. Removing it would allow a
		// stale IndexedDB entry to be interpreted as an importable legacy project.
	}

	async migrateLegacyProjects(): Promise<{
		migrated: number;
		failed: string[];
	}> {
		await this.ensureMigrations();
		const daemonIds = new Set(
			(await localProjectStore.listProjects()).map((project) => project.id),
		);
		for (const id of daemonIds) this.markDaemonManaged(id);
		const cachedIds = await this.projectsAdapter.list();
		let migrated = 0;
		const failed: string[] = [];

		for (const id of cachedIds) {
			if (daemonIds.has(id)) continue;
			try {
				const project = await this.loadCachedProject({ id });
				if (!project) {
					failed.push(id);
					continue;
				}
				await localProjectStore.saveProject({ project });
				this.markDaemonManaged(id);
				migrated += 1;
			} catch {
				failed.push(id);
			}
		}

		return { migrated, failed };
	}

	async countLegacyProjects(): Promise<number> {
		await this.ensureMigrations();
		const [daemonProjects, cachedIds] = await Promise.all([
			localProjectStore.listProjects(),
			this.projectsAdapter.list(),
		]);
		const daemonIds = new Set(daemonProjects.map((project) => project.id));
		for (const id of daemonIds) this.markDaemonManaged(id);
		return cachedIds.filter(
			(id) => !daemonIds.has(id) && !this.managedProjectIds.has(id),
		).length;
	}

	async saveMediaAsset({
		projectId,
		mediaAsset,
	}: {
		projectId: string;
		mediaAsset: MediaAsset;
	}): Promise<void> {
		const managedAsset = await localProjectStore.uploadManagedMedia({
			projectId,
			assetId: mediaAsset.id,
			file: mediaAsset.file,
			...(mediaAsset.duration !== undefined && mediaAsset.duration > 0
				? {
						durationTicks: mediaTimeFromSeconds({
							seconds: mediaAsset.duration,
						}),
					}
				: {}),
			...(mediaAsset.width !== undefined ? { width: mediaAsset.width } : {}),
			...(mediaAsset.height !== undefined ? { height: mediaAsset.height } : {}),
			...(mediaAsset.hasAudio !== undefined
				? { hasAudio: mediaAsset.hasAudio }
				: {}),
		});

		const { mediaMetadataAdapter, mediaAssetsAdapter } =
			this.getProjectMediaAdapters({ projectId });

		const metadata: MediaAssetData = {
			id: mediaAsset.id,
			name: mediaAsset.name,
			type: mediaAsset.type,
			size: mediaAsset.file.size,
			lastModified: mediaAsset.file.lastModified,
			width: mediaAsset.width,
			height: mediaAsset.height,
			duration: mediaAsset.duration,
			fps: mediaAsset.fps,
			hasAudio: mediaAsset.hasAudio,
			thumbnailUrl: mediaAsset.thumbnailUrl,
			thumbnailContentHash: mediaAsset.thumbnailContentHash,
			ephemeral: mediaAsset.ephemeral,
			contentHash: managedAsset.contentHash,
		};

		try {
			await mediaAssetsAdapter.set({
				key: mediaAsset.id,
				value: mediaAsset.file,
			});
			await mediaMetadataAdapter.set({
				key: mediaAsset.id,
				value: metadata,
			});
		} catch (error) {
			try {
				await mediaAssetsAdapter.remove(mediaAsset.id);
			} catch {
				// Ignore cleanup failures so the original storage error is preserved.
			}

			// The daemon already owns the bytes. IndexedDB/OPFS is only a cache, so
			// browser quota failure must not roll back or retry the authoritative
			// revision.
			console.warn(
				"[storage] managed media committed but browser cache failed",
				{
					projectId,
					assetId: mediaAsset.id,
					error,
				},
			);
		}
	}

	async loadMediaAsset({
		projectId,
		id,
	}: {
		projectId: string;
		id: string;
	}): Promise<MediaAsset | null> {
		const { mediaMetadataAdapter, mediaAssetsAdapter } =
			this.getProjectMediaAdapters({ projectId });

		const [file, metadata] = await Promise.all([
			mediaAssetsAdapter.get(id),
			mediaMetadataAdapter.get(id),
		]);

		if (!file || !metadata) return null;

		let url: string;
		if (metadata.type === "image" && (!file.type || file.type === "")) {
			try {
				const text = await file.text();
				if (text.trim().startsWith("<svg")) {
					const svgBlob = new Blob([text], { type: "image/svg+xml" });
					url = URL.createObjectURL(svgBlob);
				} else {
					url = URL.createObjectURL(file);
				}
			} catch {
				url = URL.createObjectURL(file);
			}
		} else {
			url = URL.createObjectURL(file);
		}

		return {
			id: metadata.id,
			name: metadata.name,
			type: metadata.type,
			file,
			url,
			width: metadata.width,
			height: metadata.height,
			duration: metadata.duration,
			fps: metadata.fps,
			hasAudio: metadata.hasAudio,
			contentHash: metadata.contentHash,
			thumbnailUrl: metadata.thumbnailUrl,
			thumbnailContentHash: metadata.thumbnailContentHash,
			ephemeral: metadata.ephemeral,
		};
	}

	async loadAllMediaAssets({
		projectId,
	}: {
		projectId: string;
	}): Promise<MediaAsset[]> {
		const { mediaMetadataAdapter } = this.getProjectMediaAdapters({
			projectId,
		});

		const mediaIds = await mediaMetadataAdapter.list();
		const cachedItems: MediaAsset[] = [];

		for (const id of mediaIds) {
			const item = await this.loadMediaAsset({ projectId, id });
			if (item) {
				cachedItems.push(item);
			}
		}
		let managedAssets = await localProjectStore.listManagedAssets({
			projectId,
		});
		const managedIds = new Set(managedAssets.map((asset) => asset.id));

		// One-time migration for pre-daemon OPFS media. The authoritative asset is
		// upserted with the existing stable ID before the cache can be discarded.
		for (const cached of cachedItems) {
			if (managedIds.has(cached.id)) continue;
			try {
				await this.saveMediaAsset({ projectId, mediaAsset: cached });
			} catch (error) {
				console.error("Failed to migrate browser media into local core", {
					projectId,
					assetId: cached.id,
					error,
				});
			}
		}
		if (cachedItems.some((asset) => !managedIds.has(asset.id))) {
			managedAssets = await localProjectStore.listManagedAssets({ projectId });
		}

		const cachedById = new Map(cachedItems.map((asset) => [asset.id, asset]));
		const mediaItems: MediaAsset[] = [];
		for (const asset of managedAssets) {
			if (!isBrowserMediaAsset(asset)) continue;
			const cached = cachedById.get(asset.id);
			if (cached?.contentHash === browserContentIdentity(asset)) {
				mediaItems.push(
					await this.attachManagedThumbnail({
						projectId,
						asset,
						mediaAsset: cached,
					}),
				);
				continue;
			}
			try {
				const hydrated = await this.hydrateManagedMediaAsset({
					projectId,
					asset,
				});
				mediaItems.push(hydrated);
			} catch (error) {
				console.error("Failed to hydrate managed media from local core", {
					projectId,
					assetId: asset.id,
					error,
				});
				if (cached) mediaItems.push(cached);
			}
		}

		// Keep a legacy cache entry visible when its one-time migration could not
		// reach the daemon. OPFS is never treated as authoritative once a managed
		// record exists, but hiding an otherwise usable local asset would turn a
		// transient daemon failure into apparent data loss.
		const finalManagedIds = new Set(managedAssets.map((asset) => asset.id));
		const returnedIds = new Set(mediaItems.map((asset) => asset.id));
		for (const cached of cachedItems) {
			if (!finalManagedIds.has(cached.id) && !returnedIds.has(cached.id)) {
				mediaItems.push(cached);
			}
		}

		return mediaItems;
	}

	private async hydrateManagedMediaAsset({
		projectId,
		asset,
	}: {
		projectId: string;
		asset: BrowserLocalMediaAsset;
	}): Promise<MediaAsset> {
		const blob = await localProjectStore.downloadManagedMedia({
			projectId,
			assetId: asset.id,
		});
		const file = new File([blob], asset.name, {
			type:
				blob.type ||
				asset.managedMedia?.mimeType ||
				asset.linkedFile?.mimeType ||
				"application/octet-stream",
			lastModified: asset.managedMedia?.lastModified ?? 0,
		});
		let mediaAsset: MediaAsset = {
			id: asset.id,
			name: asset.name,
			type: asset.kind,
			file,
			url: URL.createObjectURL(file),
			...(asset.width !== undefined ? { width: asset.width } : {}),
			...(asset.height !== undefined ? { height: asset.height } : {}),
			...(asset.durationTicks !== undefined
				? {
						duration: mediaTimeToSeconds({
							time: roundMediaTime({ time: asset.durationTicks }),
						}),
					}
				: {}),
			hasAudio: asset.hasAudio,
			contentHash: browserContentIdentity(asset),
		};
		mediaAsset = await this.attachManagedThumbnail({
			projectId,
			asset,
			mediaAsset,
		});
		await this.cacheManagedMediaAsset({ projectId, mediaAsset });
		return mediaAsset;
	}

	private async attachManagedThumbnail({
		projectId,
		asset,
		mediaAsset,
	}: {
		projectId: string;
		asset: BrowserLocalMediaAsset;
		mediaAsset: MediaAsset;
	}): Promise<MediaAsset> {
		const hash = asset.derivatives?.thumbnail?.contentHash;
		if (!hash) return mediaAsset;
		try {
			const thumbnail = await localProjectStore.downloadMediaDerivative({
				projectId,
				assetId: asset.id,
				kind: "thumbnail",
			});
			if (mediaAsset.thumbnailUrl?.startsWith("blob:")) {
				URL.revokeObjectURL(mediaAsset.thumbnailUrl);
			}
			return {
				...mediaAsset,
				thumbnailUrl: URL.createObjectURL(thumbnail),
				thumbnailContentHash: hash,
			};
		} catch (error) {
			console.warn("Failed to hydrate managed media thumbnail", {
				projectId,
				assetId: asset.id,
				error,
			});
			return mediaAsset;
		}
	}

	private async cacheManagedMediaAsset({
		projectId,
		mediaAsset,
	}: {
		projectId: string;
		mediaAsset: MediaAsset;
	}): Promise<void> {
		const { mediaMetadataAdapter, mediaAssetsAdapter } =
			this.getProjectMediaAdapters({ projectId });
		await mediaAssetsAdapter.set({
			key: mediaAsset.id,
			value: mediaAsset.file,
		});
		await mediaMetadataAdapter.set({
			key: mediaAsset.id,
			value: {
				id: mediaAsset.id,
				name: mediaAsset.name,
				type: mediaAsset.type,
				size: mediaAsset.file.size,
				lastModified: mediaAsset.file.lastModified,
				width: mediaAsset.width,
				height: mediaAsset.height,
				duration: mediaAsset.duration,
				fps: mediaAsset.fps,
				hasAudio: mediaAsset.hasAudio,
				contentHash: mediaAsset.contentHash,
				thumbnailUrl: mediaAsset.thumbnailUrl,
				thumbnailContentHash: mediaAsset.thumbnailContentHash,
				ephemeral: mediaAsset.ephemeral,
			},
		});
	}

	async deleteMediaAsset({
		projectId,
		id,
	}: {
		projectId: string;
		id: string;
	}): Promise<void> {
		await localProjectStore.removeManagedMedia({ projectId, assetId: id });
		const { mediaMetadataAdapter, mediaAssetsAdapter } =
			this.getProjectMediaAdapters({ projectId });

		await Promise.all([
			mediaAssetsAdapter.remove(id),
			mediaMetadataAdapter.remove(id),
		]);
	}

	async deleteProjectMedia({
		projectId,
	}: {
		projectId: string;
	}): Promise<void> {
		const { mediaMetadataAdapter, mediaAssetsAdapter } =
			this.getProjectMediaAdapters({ projectId });

		await Promise.all([
			mediaMetadataAdapter.clear(),
			mediaAssetsAdapter.clear(),
		]);
	}

	async clearAllData(): Promise<void> {
		const projects = await localProjectStore.listProjects();
		for (const project of projects) {
			await localProjectStore.deleteProject({ projectId: project.id });
		}
		await this.projectsAdapter.clear();
		// project-specific media and timelines cleaned up when projects are deleted
	}

	async getStorageInfo(): Promise<{
		projects: number;
		isOPFSSupported: boolean;
		isIndexedDBSupported: boolean;
	}> {
		const [daemonProjects, cachedProjectIds] = await Promise.all([
			localProjectStore.listProjects(),
			this.projectsAdapter.list(),
		]);
		const projectIds = new Set([
			...daemonProjects.map((project) => project.id),
			...cachedProjectIds,
		]);

		return {
			projects: projectIds.size,
			isOPFSSupported: this.isOPFSSupported(),
			isIndexedDBSupported: this.isIndexedDBSupported(),
		};
	}

	async getProjectStorageInfo({ projectId }: { projectId: string }): Promise<{
		mediaItems: number;
	}> {
		const { mediaMetadataAdapter } = this.getProjectMediaAdapters({
			projectId,
		});

		const mediaIds = await mediaMetadataAdapter.list();

		return {
			mediaItems: mediaIds.length,
		};
	}

	async loadSavedSounds(): Promise<SavedSoundsData> {
		try {
			const savedSoundsData = await this.savedSoundsAdapter.get("user-sounds");
			return (
				savedSoundsData || {
					sounds: [],
					lastModified: new Date().toISOString(),
				}
			);
		} catch (error) {
			console.error("Failed to load saved sounds:", error);
			return { sounds: [], lastModified: new Date().toISOString() };
		}
	}

	async saveSoundEffect({
		soundEffect,
	}: {
		soundEffect: SoundEffect;
	}): Promise<void> {
		try {
			const currentData = await this.loadSavedSounds();

			if (currentData.sounds.some((sound) => sound.id === soundEffect.id)) {
				return; // Already saved
			}

			const savedSound: SavedSound = {
				id: soundEffect.id,
				name: soundEffect.name,
				username: soundEffect.username,
				previewUrl: soundEffect.previewUrl,
				downloadUrl: soundEffect.downloadUrl,
				duration: soundEffect.duration,
				tags: soundEffect.tags,
				license: soundEffect.license,
				savedAt: new Date().toISOString(),
			};

			const updatedData: SavedSoundsData = {
				sounds: [...currentData.sounds, savedSound],
				lastModified: new Date().toISOString(),
			};

			await this.savedSoundsAdapter.set({
				key: "user-sounds",
				value: updatedData,
			});
		} catch (error) {
			console.error("Failed to save sound effect:", error);
			throw error;
		}
	}

	async removeSavedSound({ soundId }: { soundId: number }): Promise<void> {
		try {
			const currentData = await this.loadSavedSounds();

			const updatedData: SavedSoundsData = {
				sounds: currentData.sounds.filter((sound) => sound.id !== soundId),
				lastModified: new Date().toISOString(),
			};

			await this.savedSoundsAdapter.set({
				key: "user-sounds",
				value: updatedData,
			});
		} catch (error) {
			console.error("Failed to remove saved sound:", error);
			throw error;
		}
	}

	async isSoundSaved({ soundId }: { soundId: number }): Promise<boolean> {
		try {
			const currentData = await this.loadSavedSounds();
			return currentData.sounds.some((sound) => sound.id === soundId);
		} catch (error) {
			console.error("Failed to check if sound is saved:", error);
			return false;
		}
	}

	async clearSavedSounds(): Promise<void> {
		try {
			await this.savedSoundsAdapter.remove("user-sounds");
		} catch (error) {
			console.error("Failed to clear saved sounds:", error);
			throw error;
		}
	}

	isOPFSSupported(): boolean {
		return OPFSAdapter.isSupported();
	}

	isIndexedDBSupported(): boolean {
		return "indexedDB" in window;
	}

	isFullySupported(): boolean {
		return this.isIndexedDBSupported() && this.isOPFSSupported();
	}
}

export const storageService = new StorageService();
export { StorageService };
