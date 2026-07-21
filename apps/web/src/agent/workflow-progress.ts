import type { JobRecord } from "@/services/local-core";

export type WorkflowProgress = {
	proposalId: string;
	callIndex: number;
	callCount: number;
	tool: string;
	label: string;
	status: "running" | "completed" | "failed";
	jobIds: string[];
	jobs: JobRecord[];
	error?: string;
};

export type PersistedWorkflow = {
	proposalId: string;
	pinnedRevision: number;
	jobIds: string[];
};

export function workflowWithJobs({
	progress,
	jobs,
}: {
	progress: WorkflowProgress;
	jobs: JobRecord[];
}): WorkflowProgress {
	const byId = new Map(jobs.map((job) => [job.id, job]));
	const relevant = progress.jobIds
		.map((jobId) => byId.get(jobId))
		.filter((job): job is JobRecord => Boolean(job));
	const failed = relevant.find(
		(job) => job.state === "failed" || job.state === "cancelled",
	);
	if (failed) {
		return {
			...progress,
			jobs: relevant,
			status: "failed",
			label:
				failed.state === "cancelled" ? "Workflow cancelled" : "Workflow failed",
			error:
				failed.error?.message ??
				failed.message ??
				`Persistent job ${failed.id} ${failed.state}.`,
		};
	}
	const allSucceeded =
		progress.jobIds.length > 0 &&
		progress.jobIds.every((jobId) => byId.get(jobId)?.state === "succeeded");
	if (allSucceeded) {
		return {
			...progress,
			jobs: relevant,
			status: "completed",
			label: "Workflow completed",
			error: undefined,
		};
	}
	return {
		...progress,
		jobs: relevant,
		status: progress.jobIds.length > 0 ? "running" : progress.status,
		label:
			progress.jobIds.length > 0
				? `Running ${progress.jobIds.length} persistent job${progress.jobIds.length === 1 ? "" : "s"}`
				: progress.label,
		error: undefined,
	};
}
