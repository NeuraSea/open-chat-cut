import { randomUUID } from "node:crypto";

const string = (description, extra = {}) => ({
	type: "string",
	description,
	...extra,
});

const integer = (description, extra = {}) => ({
	type: "integer",
	description,
	...extra,
});

const boolean = (description) => ({ type: "boolean", description });

const object = (properties, required = [], additionalProperties = false) => ({
	type: "object",
	properties,
	required,
	additionalProperties,
});

const freeObject = (description) => ({
	type: "object",
	description,
	additionalProperties: true,
});

const stableIdPattern = "^[A-Za-z0-9._:/@-]+$";
const AGENT_OPERATION_TYPES = Object.freeze([
	"setProjectName",
	"setProjectSettings",
	"removeAsset",
	"addScene",
	"removeScene",
	"setSceneName",
	"addTrack",
	"removeTrack",
	"setTrackProperties",
	"insertItem",
	"removeItem",
	"moveItem",
	"replaceItem",
	"trimItem",
	"splitItem",
	"setCaption",
	"setCaptionStyle",
	"upsertTranscript",
	"removeTranscript",
	"setTranscriptWordsDeleted",
	"deleteTranscriptSegment",
	"setTranscriptDisplayText",
	"setTranscriptSpeaker",
	"splitTranscriptSegment",
	"mergeTranscriptSegments",
	"reorderTranscriptSegments",
	"upsertStorySequence",
	"removeStorySequence",
	"reorderStoryClips",
	"closeStoryGaps",
]);
const projectId = string("Stable project identifier.", {
	minLength: 1,
	maxLength: 256,
	pattern: stableIdPattern,
});
const assetId = string("Stable managed-media asset identifier.", {
	minLength: 1,
	maxLength: 256,
	pattern: stableIdPattern,
});
const jobId = string("Persistent daemon job identifier.", {
	minLength: 1,
	maxLength: 256,
	pattern: stableIdPattern,
});
const expectedRevision = integer(
	"Project revision this operation was planned against. A mismatch must return a conflict instead of overwriting newer work.",
	{ minimum: 0 },
);
const idempotencyKey = string(
	"Unique retry key for this logical write, preferably a UUID. Reuse it only when retrying the exact same operation.",
	{ minLength: 8, maxLength: 200, pattern: stableIdPattern },
);
const operations = {
	type: "array",
	description:
		"Validated semantic operations. Whole-document and whole-scene-graph replacement operations are never accepted from MCP.",
	minItems: 1,
	items: {
		type: "object",
		description:
			"One allowlisted semantic operation accepted by the shared Operation Engine.",
		properties: {
			type: string("Semantic operation discriminator.", {
				enum: AGENT_OPERATION_TYPES,
			}),
		},
		required: ["type"],
		additionalProperties: true,
	},
};

function annotations({
	readOnly,
	destructive = false,
	idempotent = true,
	openWorld = false,
}) {
	return {
		readOnlyHint: readOnly,
		destructiveHint: destructive,
		idempotentHint: idempotent,
		openWorldHint: openWorld,
	};
}

function tool(name, title, description, inputSchema, toolAnnotations) {
	return Object.freeze({
		name,
		title,
		description,
		inputSchema,
		annotations: annotations(toolAnnotations),
	});
}

export const TOOL_DEFINITIONS = Object.freeze([
	tool(
		"get_status",
		"Get OpenChatCut Status",
		"Check daemon health, protocol compatibility, worker state, and enabled capabilities. Call this before a multi-step workflow.",
		object({}),
		{ readOnly: true },
	),
	tool(
		"list_projects",
		"List Projects",
		"List local projects and their current revisions without reading project documents.",
		object({}),
		{ readOnly: true },
	),
	tool(
		"create_project",
		"Create Project",
		"Create an empty local project. The daemon remains the sole authority for the new document.",
		object(
			{
				name: string("Human-readable project name.", {
					minLength: 1,
					maxLength: 160,
				}),
				idempotencyKey,
			},
			["name", "idempotencyKey"],
		),
		{ readOnly: false },
	),
	tool(
		"read_project",
		"Read Project",
		"Read the current project envelope, including its revision and document hash.",
		object({ projectId }, ["projectId"]),
		{ readOnly: true },
	),
	tool(
		"get_editor_url",
		"Get Editor URL",
		"Get the loopback editor URL for a project. This returns a URL but never opens a browser itself.",
		object({ projectId }, ["projectId"]),
		{ readOnly: true },
	),
	tool(
		"import_local_media",
		"Import Local Media",
		"Ask the daemon to import an authorized local file as managed media, or explicitly link it when requested.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				path: string("Absolute host path selected by the user.", {
					minLength: 1,
				}),
				mode: string(
					"Managed copies are portable; linked files remain external.",
					{
						enum: ["managed", "linked"],
					},
				),
				confirmLinkedRisk: boolean(
					"Required only for mode=linked after the user accepts that the project depends on an authorized external path and is not portable.",
				),
			},
			["projectId", "expectedRevision", "idempotencyKey", "path"],
		),
		{ readOnly: false },
	),
	tool(
		"import_remote_media",
		"Import Remote Media",
		"Download a user-approved HTTP(S) asset through daemon SSRF and size checks, then store it as managed media.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				url: string("Public HTTP(S) media URL.", {
					format: "uri",
					pattern: "^https?://",
				}),
				expectedMimeType: string(
					"Optional expected MIME type used as an additional validation signal.",
				),
				confirm: boolean(
					"Confirm that this public URL may be contacted and downloaded.",
				),
			},
			["projectId", "expectedRevision", "idempotencyKey", "url", "confirm"],
		),
		{ readOnly: false, openWorld: true },
	),
	tool(
		"import_project_package",
		"Import Project Package",
		"Restore a user-approved .occproj package from an authorized local path after validating its ZIP structure, canonical document hash, and every managed media digest.",
		object(
			{
				idempotencyKey,
				path: string("Absolute host path to an authorized .occproj package.", {
					minLength: 1,
				}),
				confirm: boolean(
					"Confirm creation of the packaged project and installation of its managed media.",
				),
			},
			["idempotencyKey", "path", "confirm"],
		),
		{ readOnly: false },
	),
	tool(
		"inspect_media",
		"Inspect Media",
		"Read technical metadata, provenance, proxy state, waveform state, and derived-asset relationships for media.",
		object({ projectId, assetId }, ["projectId", "assetId"]),
		{ readOnly: true },
	),
	tool(
		"search_broll",
		"Search B-roll",
		"Search managed local image/video assets first and resolve an optional transcript-word anchor against the current StorySequence. If no local result matches, returns available Codex image/video generator fallbacks without submitting work.",
		object(
			{
				projectId,
				query: string("Visual subject, concept, product, or scene to find.", {
					minLength: 1,
					maxLength: 500,
				}),
				transcriptId: string(
					"Optional transcript containing the stable anchor word.",
					{ minLength: 1, maxLength: 256, pattern: stableIdPattern },
				),
				wordId: string(
					"Optional active transcript word; provide together with transcriptId.",
					{ minLength: 1, maxLength: 256, pattern: stableIdPattern },
				),
				edge: string("Anchor to the word start or end.", {
					enum: ["start", "end"],
				}),
				bias: string("Fallback direction if later script edits delete the word.", {
					enum: ["before", "after", "nearest"],
				}),
				limit: integer("Maximum local results.", { minimum: 1, maximum: 50 }),
			},
			["projectId", "query"],
		),
		{ readOnly: true },
	),
	tool(
		"process_audio",
		"Process Audio",
		"Create a reversible derived audio asset for cleanup or mixing; never overwrite the source media.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				assetId,
				operation: string("Audio processing operation.", {
					enum: [
						"denoise",
						"normalize",
						"compress-dialogue",
						"duck-music",
						"loop",
						"crossfade",
					],
				}),
				options: freeObject(
					"Operation-specific bounded settings. Denoise accepts engine=auto|deepfilternet|rnnoise|ffmpeg; auto uses optional DeepFilterNet and falls back to the CPU-safe FFmpeg filter.",
				),
			},
			[
				"projectId",
				"expectedRevision",
				"idempotencyKey",
				"assetId",
				"operation",
			],
		),
		{ readOnly: false },
	),
	tool(
		"validate_timeline_edit",
		"Validate Timeline Edit",
		"Dry-run semantic timeline operations against a revision and return a normalized diff, dependencies, warnings, and estimated provider cost.",
		object(
			{
				projectId,
				expectedRevision,
				operations,
			},
			["projectId", "expectedRevision", "operations"],
		),
		{ readOnly: true },
	),
	tool(
		"apply_timeline_edit",
		"Apply Timeline Edit",
		"Atomically apply already-reviewed semantic operations using revision compare-and-swap. Operations may remove timeline content but remain revision-undoable.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				operations,
				transactionId: string(
					"Optional stable transaction ID; the bridge derives one from idempotencyKey when omitted.",
					{
						minLength: 1,
						maxLength: 256,
						pattern: stableIdPattern,
					},
				),
				proposalId: string(
					"Validated proposal identifier returned by the daemon.",
					{
						minLength: 1,
						maxLength: 256,
						pattern: stableIdPattern,
					},
				),
				confirm: boolean("Confirm application of the reviewed proposal."),
				summary: string("Short human-readable transaction summary.", {
					maxLength: 500,
				}),
			},
			[
				"projectId",
				"expectedRevision",
				"idempotencyKey",
				"operations",
				"proposalId",
				"confirm",
			],
		),
		{ readOnly: false, destructive: true },
	),
	tool(
		"change_history",
		"Read Change History",
		"List project revisions, named versions, transaction summaries, and undo/restore checkpoints.",
		object(
			{
				projectId,
				limit: integer("Maximum revisions to return.", {
					minimum: 1,
					maximum: 500,
				}),
			},
			["projectId"],
		),
		{ readOnly: true },
	),
	tool(
		"start_transcription",
		"Start Transcription",
		"Queue local word-timestamp transcription and optional speaker diarization for a managed media asset.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				assetId,
				language: string("BCP-47 language hint, or auto when omitted."),
				diarization: boolean(
					"Request speaker diarization when a configured model is available.",
				),
				minSpeakers: integer("Optional minimum number of speakers for pyannote.", {
					minimum: 1,
					maximum: 32,
				}),
				maxSpeakers: integer("Optional maximum number of speakers for pyannote.", {
					minimum: 1,
					maximum: 32,
				}),
				engine: string("Configured local transcription engine preference.", {
					enum: ["auto", "faster-whisper"],
				}),
			},
			["projectId", "expectedRevision", "idempotencyKey", "assetId"],
		),
		{ readOnly: false },
	),
	tool(
		"read_script",
		"Read Script",
		"Read transcript words, immutable spoken text, corrected display text, speakers, anchors, StorySequence mappings, and optional local cleanup suggestions.",
		object(
			{
				projectId,
				transcriptId: string(
					"Transcript identifier; omit when the project has one active transcript.",
				),
				includeDeleted: boolean(
					"Include words removed from the active StorySequence.",
				),
				includeSuggestions: boolean(
					"Analyze stable word timestamps locally for fillers, repeated takes, long pauses, and review-only highlights.",
				),
				cleanupOptions: freeObject(
					"Optional pause thresholds, confidence thresholds, repeated-take settings, and highlight limit.",
				),
			},
			["projectId"],
		),
		{ readOnly: true },
	),
	tool(
		"apply_script_edit",
		"Apply Script Edit",
		"Apply one reviewed transcript transaction containing delete words/ranges, split, reorder, close gaps, speaker/display-text changes, and semantic caption creation.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				edit: freeObject("One transcript edit to validate in dry-run mode."),
				edits: {
					type: "array",
					minItems: 1,
					description:
						"Transcript edit operations anchored to stable word identifiers.",
					items: freeObject(
						"A semantic transcript edit accepted by the daemon.",
					),
				},
				operations: {
					type: "array",
					minItems: 1,
					description:
						"Validated transcript operations returned in a proposal.",
					items: freeObject("A transcript operation accepted by the daemon."),
				},
				proposalId: string(
					"Validated proposal identifier when applying a prior dry run.",
					{
						minLength: 1,
						maxLength: 256,
						pattern: stableIdPattern,
					},
				),
				dryRun: boolean("Return a proposal without changing project state."),
				confirm: boolean(
					"Confirm application of a previously reviewed proposal.",
				),
				addCutCrossfades: boolean(
					"Add short frame-aligned audio crossfades at materialized cuts.",
				),
			},
			["projectId", "expectedRevision", "idempotencyKey"],
		),
		{ readOnly: false, destructive: true },
	),
	tool(
		"edit_captions",
		"Edit Captions",
		"Create or update semantic CaptionElements linked to transcript word anchors, including style, layout, speakers, and translation tracks.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				action: string("Caption mutation.", {
					enum: [
						"create",
						"update-style",
						"remap",
						"translate",
						"import",
						"remove",
					],
				}),
				captionTrackId: string(
					"Existing caption track identifier when the action targets one.",
				),
				confirm: boolean(
					"Confirm destructive caption removal after reviewing the affected track.",
				),
				options: freeObject(
					"Caption source, language, preset, Unicode layout, highlighting, or import options.",
				),
			},
			["projectId", "expectedRevision", "idempotencyKey", "action"],
		),
		{ readOnly: false, destructive: true },
	),
	tool(
		"list_generators",
		"List Generators",
		"List installed generation providers and their current availability, models, capabilities, and cost metadata without submitting work.",
		object({
			kind: string("Optional capability filter.", {
				enum: ["image", "video", "voice", "music", "sfx", "webCapture"],
			}),
		}),
		{ readOnly: true },
	),
	tool(
		"generate_asset",
		"Generate Asset",
		"Submit an approved durable image, video, voice, music, SFX, or isolated website-capture job. Generated audio/image/video can include options.placement and materialize as an editable timeline item after download. local-web-capture downloads through daemon SSRF checks and renders in offline script-disabled Chromium; codex-image uses the signed-in Codex allowance.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				kind: string("Asset kind.", {
					enum: ["image", "video", "voice", "music", "sfx", "webCapture"],
				}),
				provider: string(
					"Provider descriptor identifier returned by list_generators.",
					{ minLength: 1 },
				),
				model: string(
					"Optional provider model identifier. codex-image accepts only gpt-image-2.",
				),
				prompt: string("Approved generation prompt.", {
					minLength: 1,
					maxLength: 20000,
				}),
				confirm: boolean(
					"Explicitly confirm external data transfer and any provider charge after reviewing the prompt, model, and cost warning.",
				),
				options: freeObject(
					"Provider-specific duration, aspect, voice, seed, reference, normalization, or sourceUrl settings. For generated audio/image/video, placement may contain startSeconds or startTicks, durationSeconds or durationTicks, and optional sceneId, trackId, name, or timelineAnchor; placement remains local and is never sent to the provider. local-web-capture requires sourceUrl.",
				),
			},
			[
				"projectId",
				"expectedRevision",
				"idempotencyKey",
				"kind",
				"provider",
				"prompt",
				"confirm",
			],
		),
		{ readOnly: false, openWorld: true },
	),
	tool(
		"create_motion_graphic",
		"Create Motion Graphic",
		"Create an editable motion graphic from the safe DSL, a versioned built-in template, or explicitly requested advanced JSX compiled into bounded non-executable IR. Availability is reported by motionGraphicJsx status.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				mode: string(
					"Safe DSL is preferred; JSX requires explicit advanced-mode intent.",
					{
						enum: ["dsl", "jsx"],
					},
				),
				definition: {
					type: ["object", "string"],
					description:
						"Versioned DSL object or JSX source. Never include network or filesystem access.",
				},
				templateId: string(
					"Built-in template id returned by list_generators; provide this instead of definition.",
				),
				startSeconds: {
					type: "number",
					minimum: 0,
					description: "Timeline insertion time.",
				},
				durationSeconds: {
					type: "number",
					exclusiveMinimum: 0,
					description: "Graphic duration.",
				},
				trackId: string("Optional target graphics track identifier."),
			},
			[
				"projectId",
				"expectedRevision",
				"idempotencyKey",
				"mode",
				"startSeconds",
				"durationSeconds",
			],
		),
		{ readOnly: false },
	),
	tool(
		"render_preview_frames",
		"Render Preview Frames",
		"Render bounded representative frames from a pinned revision for visual review. This may create temporary preview artifacts but does not change the project revision.",
		object(
			{
				projectId,
				revision: integer("Pinned project revision to render.", { minimum: 0 }),
				timesSeconds: {
					type: "array",
					minItems: 1,
					maxItems: 24,
					items: { type: "number", minimum: 0 },
					description: "Timeline times to render, in seconds.",
				},
				width: integer("Preview width in pixels.", {
					minimum: 64,
					maximum: 3840,
				}),
			},
			["projectId", "revision", "timesSeconds"],
		),
		{ readOnly: false },
	),
	tool(
		"validate_project",
		"Validate Project",
		"Check a pinned revision for missing assets, invalid anchors, unsupported effects, unsafe MG, caption issues, and export blockers.",
		object(
			{
				projectId,
				revision: integer("Pinned revision to validate.", { minimum: 0 }),
				target: string(
					"Optional export target to include compatibility checks for.",
				),
			},
			["projectId", "revision"],
		),
		{ readOnly: true },
	),
	tool(
		"start_export",
		"Start Export",
		"Queue an export pinned to an exact revision. The daemon must refuse accidental overwrite unless explicitly allowed by a confirmed workflow.",
		object(
			{
				projectId,
				expectedRevision,
				idempotencyKey,
				format: string("Delivery format.", {
					enum: [
						"mp4",
						"webm",
						"wav",
						"mp3",
						"srt",
						"vtt",
						"ass",
						"txt",
						"png",
						"png-sequence",
						"prores-4444",
						"premiere-xml",
						"resolve-xml",
						"project-package",
					],
				}),
				outputPath: string(
					"Portable file name written inside the daemon-managed export directory; PNG sequences use a .zip archive containing sequence.json and numbered PNG frames.",
					{ minLength: 1, maxLength: 240, pattern: "^[^/\\\\]+$" },
				),
				allowOverwrite: boolean(
					"Overwrite an existing path only after explicit user confirmation.",
				),
				settings: freeObject(
					"Resolution, frame rate, range, codec, audio, and caption-burn settings.",
				),
			},
			[
				"projectId",
				"expectedRevision",
				"idempotencyKey",
				"format",
				"outputPath",
			],
		),
		{ readOnly: false, destructive: true },
	),
	tool(
		"track_jobs",
		"Track Jobs",
		"Read one persistent job or list jobs and their progress, retry state, provider status, output assets, and errors.",
		object({
			jobId,
			projectId,
			limit: integer("Maximum jobs to return.", { minimum: 1, maximum: 500 }),
		}),
		{ readOnly: true },
	),
]);

const TOOL_BY_NAME = new Map(
	TOOL_DEFINITIONS.map((definition) => [definition.name, definition]),
);

export function getToolDefinition(name) {
	return TOOL_BY_NAME.get(name);
}

function isPlainObject(value) {
	return value !== null && typeof value === "object" && !Array.isArray(value);
}

function matchesType(value, type) {
	switch (type) {
		case "object":
			return isPlainObject(value);
		case "array":
			return Array.isArray(value);
		case "string":
			return typeof value === "string";
		case "number":
			return typeof value === "number" && Number.isFinite(value);
		case "integer":
			return Number.isInteger(value);
		case "boolean":
			return typeof value === "boolean";
		case "null":
			return value === null;
		default:
			return true;
	}
}

function validateValue(schema, value, path) {
	const types = Array.isArray(schema.type)
		? schema.type
		: schema.type
			? [schema.type]
			: [];
	if (types.length > 0 && !types.some((type) => matchesType(value, type))) {
		throw new TypeError(`${path} must be ${types.join(" or ")}`);
	}
	if (schema.enum && !schema.enum.includes(value)) {
		throw new TypeError(`${path} must be one of: ${schema.enum.join(", ")}`);
	}
	if (typeof value === "string") {
		if (schema.minLength !== undefined && value.length < schema.minLength) {
			throw new TypeError(
				`${path} is shorter than ${schema.minLength} characters`,
			);
		}
		if (schema.maxLength !== undefined && value.length > schema.maxLength) {
			throw new TypeError(
				`${path} is longer than ${schema.maxLength} characters`,
			);
		}
		if (
			schema.pattern !== undefined &&
			!new RegExp(schema.pattern).test(value)
		) {
			throw new TypeError(`${path} contains unsupported characters`);
		}
		if (schema.format === "uri") {
			let parsed;
			try {
				parsed = new URL(value);
			} catch {
				throw new TypeError(`${path} must be an absolute URI`);
			}
			if (!parsed.protocol)
				throw new TypeError(`${path} must be an absolute URI`);
		}
	}
	if (typeof value === "number") {
		if (schema.minimum !== undefined && value < schema.minimum) {
			throw new TypeError(`${path} must be at least ${schema.minimum}`);
		}
		if (schema.maximum !== undefined && value > schema.maximum) {
			throw new TypeError(`${path} must be at most ${schema.maximum}`);
		}
		if (
			schema.exclusiveMinimum !== undefined &&
			value <= schema.exclusiveMinimum
		) {
			throw new TypeError(
				`${path} must be greater than ${schema.exclusiveMinimum}`,
			);
		}
	}
	if (Array.isArray(value)) {
		if (schema.minItems !== undefined && value.length < schema.minItems) {
			throw new TypeError(
				`${path} must contain at least ${schema.minItems} item(s)`,
			);
		}
		if (schema.maxItems !== undefined && value.length > schema.maxItems) {
			throw new TypeError(
				`${path} must contain at most ${schema.maxItems} item(s)`,
			);
		}
		if (schema.items)
			value.forEach((item, index) =>
				validateValue(schema.items, item, `${path}[${index}]`),
			);
	}
	if (isPlainObject(value)) {
		const properties = schema.properties ?? {};
		for (const required of schema.required ?? []) {
			if (!(required in value))
				throw new TypeError(`${path}.${required} is required`);
		}
		if (schema.additionalProperties === false) {
			const unknown = Object.keys(value).find((key) => !(key in properties));
			if (unknown) throw new TypeError(`${path}.${unknown} is not accepted`);
		}
		for (const [key, child] of Object.entries(value)) {
			if (properties[key])
				validateValue(properties[key], child, `${path}.${key}`);
		}
	}
}

export function validateToolArguments(name, args) {
	const definition = getToolDefinition(name);
	if (!definition) throw new TypeError(`Unknown OpenChatCut tool: ${name}`);
	validateValue(definition.inputSchema, args, "arguments");
	if (name === "apply_timeline_edit" && args.confirm !== true) {
		throw new TypeError(
			"arguments.confirm must be true after the proposal has been reviewed",
		);
	}
	if (
		name === "apply_script_edit" &&
		args.edit === undefined &&
		args.edits === undefined &&
		args.operations === undefined &&
		args.proposalId === undefined
	) {
		throw new TypeError(
			"arguments must include edit, edits, operations, or proposalId",
		);
	}
	if (name === "apply_script_edit" && args.dryRun !== true) {
		if (typeof args.proposalId !== "string" || args.proposalId.length === 0) {
			throw new TypeError(
				"arguments.proposalId is required when applying a script edit",
			);
		}
		if (args.confirm !== true) {
			throw new TypeError(
				"arguments.confirm must be true after the script proposal has been reviewed",
			);
		}
	}
	if (name === "apply_script_edit" && args.operations !== undefined) {
		validateValue(operations, args.operations, "arguments.operations");
	}
}

function addQuery(path, args, names) {
	const query = new URLSearchParams();
	for (const name of names) {
		const value = args[name];
		if (value === undefined) continue;
		query.set(name, Array.isArray(value) ? value.join(",") : String(value));
	}
	const encoded = query.toString();
	return encoded ? `${path}?${encoded}` : path;
}

function without(objectValue, ...keys) {
	return Object.fromEntries(
		Object.entries(objectValue).filter(([key]) => !keys.includes(key)),
	);
}

function transactionToolArguments(args, invocationKey) {
	const {
		idempotencyKey: _idempotencyKey,
		operations: transactionOperations,
		transactionId,
		...outerArguments
	} = args;
	return {
		...outerArguments,
		transaction: {
			transactionId: transactionId ?? `tx:${invocationKey}`,
			projectId: args.projectId,
			baseRevision: args.expectedRevision,
			idempotencyKey: invocationKey,
			actor: { kind: "agent", id: "codex", displayName: "Codex" },
			operations: transactionOperations,
		},
	};
}

const GENERIC_TOOLS = new Set([
	"get_editor_url",
	"import_local_media",
	"import_remote_media",
	"import_project_package",
	"inspect_media",
	"search_broll",
	"process_audio",
	"validate_timeline_edit",
	"apply_timeline_edit",
	"start_transcription",
	"read_script",
	"apply_script_edit",
	"edit_captions",
	"list_generators",
	"generate_asset",
	"create_motion_graphic",
	"render_preview_frames",
	"validate_project",
	"start_export",
]);

export function buildDaemonRequest(name, args) {
	const headers = {};
	if (args.idempotencyKey !== undefined)
		headers["Idempotency-Key"] = args.idempotencyKey;
	if (args.expectedRevision !== undefined) {
		headers["X-OpenChatCut-Expected-Revision"] = String(args.expectedRevision);
	}

	switch (name) {
		case "get_status":
			return { method: "GET", path: "/status", headers };
		case "list_projects":
			return { method: "GET", path: "/projects", headers };
		case "create_project":
			return {
				method: "POST",
				path: "/projects",
				headers,
				body: without(args, "idempotencyKey"),
			};
		case "read_project":
			return {
				method: "GET",
				path: `/projects/${encodeURIComponent(args.projectId)}`,
				headers,
			};
		case "change_history":
			return {
				method: "GET",
				path: addQuery(
					`/projects/${encodeURIComponent(args.projectId)}/revisions`,
					args,
					["limit"],
				),
				headers,
			};
		case "track_jobs":
			return args.jobId
				? {
						method: "GET",
						path: `/jobs/${encodeURIComponent(args.jobId)}`,
						headers,
					}
				: {
						method: "GET",
						path: addQuery("/jobs", args, ["projectId", "limit"]),
						headers,
					};
		default:
			if (GENERIC_TOOLS.has(name)) {
				const invocationKey = args.idempotencyKey ?? randomUUID();
				const wireArguments =
					name === "apply_timeline_edit" || name === "validate_timeline_edit"
						? transactionToolArguments(args, invocationKey)
						: without(args, "idempotencyKey");
				return {
					method: "POST",
					path: `/tools/${encodeURIComponent(name)}`,
					headers: { ...headers, "Idempotency-Key": invocationKey },
					body: {
						arguments: wireArguments,
						idempotencyKey: invocationKey,
					},
				};
			}
			throw new TypeError(`Unknown OpenChatCut tool: ${name}`);
	}
}
