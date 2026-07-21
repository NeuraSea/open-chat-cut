import type { MigrationResult, ProjectRecord } from "./transformers/types";

export interface StorageMigrationRunArgs {
	projectId: string;
	project: ProjectRecord;
}

export abstract class StorageMigration {
	abstract from: number;
	abstract to: number;

	abstract run({
		projectId,
		project,
	}: StorageMigrationRunArgs): Promise<MigrationResult<ProjectRecord>>;

	/** Cleanup is intentionally separate so transformed data is durable first. */
	async cleanup(_args: StorageMigrationRunArgs): Promise<void> {}
}
