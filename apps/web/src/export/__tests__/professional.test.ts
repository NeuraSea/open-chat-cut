import assert from "node:assert/strict";
import { describe, test } from "node:test";
import type { JobRecord } from "@/services/local-core/types";
import {
	buildProfessionalExportArguments,
	defaultProfessionalExportFileName,
	exportFormatFromJob,
	isExportJob,
	isPortableExportFileName,
	sanitizePortableFileStem,
	withProfessionalExportExtension,
} from "../professional";

describe("professional export request shaping", () => {
	test("creates a portable revision-stamped output name", () => {
		assert.equal(
			defaultProfessionalExportFileName({
				projectName: "Interview / final:*",
				revision: 42,
				format: "prores-4444",
			}),
			"Interview - final---r42.mov",
		);
		assert.equal(sanitizePortableFileStem("CON"), "_CON");
		assert.equal(isPortableExportFileName("Interview-r42.mp4"), true);
		assert.equal(isPortableExportFileName("../Interview.mp4"), false);
		assert.equal(isPortableExportFileName("folder/Interview.mp4"), false);
	});

	test("replaces extensions instead of creating double extensions", () => {
		assert.equal(
			withProfessionalExportExtension({
				fileName: "delivery.old.mp4",
				format: "png-sequence",
			}),
			"delivery.old.zip",
		);
	});

	test("only sends settings supported by the selected format", () => {
		const video = buildProfessionalExportArguments({
			projectId: "project-one",
			expectedRevision: 7,
			format: "mp4",
			outputPath: "delivery.mov",
			allowOverwrite: false,
			range: { startSeconds: 1, endSeconds: 5 },
			resolution: { width: 1920, height: 1080 },
			fps: 30,
			captionTrackId: "captions",
		});
		assert.deepEqual(video, {
			projectId: "project-one",
			expectedRevision: 7,
			format: "mp4",
			outputPath: "delivery.mp4",
			allowOverwrite: false,
			settings: {
				range: { startSeconds: 1, endSeconds: 5 },
				resolution: { width: 1920, height: 1080 },
				fps: 30,
			},
		});

		const subtitle = buildProfessionalExportArguments({
			projectId: "project-one",
			expectedRevision: 7,
			format: "srt",
			outputPath: "captions",
			allowOverwrite: false,
			range: { startSeconds: 1, endSeconds: 5 },
			resolution: { width: 1920, height: 1080 },
			fps: 30,
			captionTrackId: "caption-track",
		});
		assert.deepEqual(subtitle.settings, { captionTrackId: "caption-track" });
	});

	test("recognizes persisted export jobs and both worker/native formats", () => {
		const workerJob = {
			id: "job-one",
			kind: "headless_export",
			state: "running",
			progress: 0.5,
			input: { options: { plan: { format: "webm" } } },
			createdAt: "2026-07-18T00:00:00Z",
			updatedAt: "2026-07-18T00:00:01Z",
		} satisfies JobRecord;
		assert.equal(isExportJob(workerJob), true);
		assert.equal(exportFormatFromJob(workerJob), "webm");

		const nativeJob = {
			...workerJob,
			kind: "subtitle_export",
			input: { options: { format: "vtt" } },
		} satisfies JobRecord;
		assert.equal(exportFormatFromJob(nativeJob), "vtt");
	});
});
