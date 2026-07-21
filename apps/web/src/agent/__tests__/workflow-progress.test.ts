import { describe, expect, test } from "bun:test";
import { workflowWithJobs, type WorkflowProgress } from "@/agent/workflow-progress";
import type { JobRecord } from "@/services/local-core";

function progress(overrides: Partial<WorkflowProgress> = {}): WorkflowProgress {
	return {
		proposalId: "proposal:test",
		callIndex: 1,
		callCount: 1,
		tool: "start_export",
		label: "Workflow dispatched to persistent jobs",
		status: "running",
		jobIds: ["job:test"],
		jobs: [],
		...overrides,
	};
}

function job(overrides: Partial<JobRecord> = {}): JobRecord {
	return {
		id: "job:test",
		projectId: "project:test",
		kind: "export",
		state: "queued",
		progress: 0,
		createdAt: "2026-01-01T00:00:00.000Z",
		updatedAt: "2026-01-01T00:00:00.000Z",
		...overrides,
	};
}

describe("Agent workflow job progress", () => {
	test("keeps an active workflow running while a durable job is queued", () => {
		const result = workflowWithJobs({
			progress: progress(),
			jobs: [job({ message: "Waiting for the worker" })],
		});
		expect(result.status).toBe("running");
		expect(result.jobs[0]?.message).toBe("Waiting for the worker");
		expect(result.label).toContain("1 persistent job");
	});

	test("marks the workflow complete only after every job succeeds", () => {
		const result = workflowWithJobs({
			progress: progress({ jobIds: ["job:a", "job:b"] }),
			jobs: [
				job({ id: "job:a", state: "succeeded", progress: 1 }),
				job({ id: "job:b", state: "succeeded", progress: 1 }),
			],
		});
		expect(result.status).toBe("completed");
		expect(result.label).toBe("Workflow completed");
	});

	test("surfaces a failed or cancelled job as a terminal failure", () => {
		const result = workflowWithJobs({
			progress: progress(),
			jobs: [
				job({
					state: "failed",
					error: { code: "PROVIDER_429", message: "Provider rate limited" },
				}),
			],
		});
		expect(result.status).toBe("failed");
		expect(result.label).toBe("Workflow failed");
		expect(result.error).toBe("Provider rate limited");
	});
});
