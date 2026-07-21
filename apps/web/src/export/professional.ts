import type { JobRecord } from "@/services/local-core/types";

export const PROFESSIONAL_EXPORT_FORMATS = [
	"mp4",
	"webm",
	"wav",
	"mp3",
	"png",
	"png-sequence",
	"prores-4444",
	"srt",
	"vtt",
	"ass",
	"txt",
	"premiere-xml",
	"resolve-xml",
	"project-package",
] as const;

export type ProfessionalExportFormat =
	(typeof PROFESSIONAL_EXPORT_FORMATS)[number];

export type ExportFormatCategory =
	| "video"
	| "audio"
	| "image"
	| "subtitle"
	| "interchange";

export interface ProfessionalExportFormatDescriptor {
	id: ProfessionalExportFormat;
	label: string;
	description: string;
	category: ExportFormatCategory;
	extension: string;
	supportsRange: boolean;
	supportsResolution: boolean;
	supportsFrameRate: boolean;
}

export const PROFESSIONAL_EXPORT_DESCRIPTORS: readonly ProfessionalExportFormatDescriptor[] =
	[
		{
			id: "mp4",
			label: "MP4 · H.264 / AAC",
			description: "Universal delivery file",
			category: "video",
			extension: "mp4",
			supportsRange: true,
			supportsResolution: true,
			supportsFrameRate: true,
		},
		{
			id: "webm",
			label: "WebM",
			description: "Open web delivery",
			category: "video",
			extension: "webm",
			supportsRange: true,
			supportsResolution: true,
			supportsFrameRate: true,
		},
		{
			id: "prores-4444",
			label: "ProRes 4444",
			description: "MOV with alpha channel",
			category: "video",
			extension: "mov",
			supportsRange: true,
			supportsResolution: true,
			supportsFrameRate: true,
		},
		{
			id: "wav",
			label: "WAV",
			description: "Uncompressed timeline mix",
			category: "audio",
			extension: "wav",
			supportsRange: true,
			supportsResolution: false,
			supportsFrameRate: false,
		},
		{
			id: "mp3",
			label: "MP3",
			description: "Compressed timeline mix",
			category: "audio",
			extension: "mp3",
			supportsRange: true,
			supportsResolution: false,
			supportsFrameRate: false,
		},
		{
			id: "png",
			label: "PNG frame",
			description: "Still frame with alpha",
			category: "image",
			extension: "png",
			supportsRange: true,
			supportsResolution: true,
			supportsFrameRate: true,
		},
		{
			id: "png-sequence",
			label: "PNG sequence",
			description: "ZIP of numbered alpha frames",
			category: "image",
			extension: "zip",
			supportsRange: true,
			supportsResolution: true,
			supportsFrameRate: true,
		},
		...(["srt", "vtt", "ass", "txt"] as const).map((id) => ({
			id,
			label: id.toUpperCase(),
			description:
				id === "ass" ? "Styled subtitle track" : "Subtitle delivery file",
			category: "subtitle" as const,
			extension: id,
			supportsRange: false,
			supportsResolution: false,
			supportsFrameRate: false,
		})),
		{
			id: "premiere-xml",
			label: "Premiere XML",
			description: "Editable NLE handoff",
			category: "interchange",
			extension: "xml",
			supportsRange: false,
			supportsResolution: false,
			supportsFrameRate: false,
		},
		{
			id: "resolve-xml",
			label: "Resolve XML",
			description: "Editable NLE handoff",
			category: "interchange",
			extension: "xml",
			supportsRange: false,
			supportsResolution: false,
			supportsFrameRate: false,
		},
		{
			id: "project-package",
			label: "OpenChatCut project",
			description: "Portable project and managed media",
			category: "interchange",
			extension: "occproj",
			supportsRange: false,
			supportsResolution: false,
			supportsFrameRate: false,
		},
	];

const DESCRIPTORS = new Map(
	PROFESSIONAL_EXPORT_DESCRIPTORS.map((descriptor) => [
		descriptor.id,
		descriptor,
	]),
);

export const EXPORT_JOB_KINDS = new Set([
	"export",
	"headless_export",
	"timeline_audio_export",
	"subtitle_export",
	"nle_xml_export",
	"project_package_export",
]);

export function getProfessionalExportDescriptor(
	format: ProfessionalExportFormat,
): ProfessionalExportFormatDescriptor {
	const descriptor = DESCRIPTORS.get(format);
	if (!descriptor) throw new TypeError(`Unsupported export format: ${format}`);
	return descriptor;
}

export function isProfessionalExportFormat(
	value: string,
): value is ProfessionalExportFormat {
	return PROFESSIONAL_EXPORT_FORMATS.some((format) => format === value);
}

export function isExportJob(job: JobRecord): boolean {
	return EXPORT_JOB_KINDS.has(job.kind);
}

export function sanitizePortableFileStem(value: string): string {
	const cleaned = value
		.normalize("NFKC")
		.split("")
		.map((character) => {
			const code = character.charCodeAt(0);
			return '<>:"/\\|?*'.includes(character) || code < 32 || code === 127
				? "-"
				: character;
		})
		.join("")
		.replace(/\s+/g, " ")
		.replace(/[. ]+$/g, "")
		.trim()
		.slice(0, 160);
	const stem = cleaned || "OpenChatCut-export";
	return /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i.test(stem)
		? `_${stem}`
		: stem;
}

export function isPortableExportFileName(value: string): boolean {
	const trimmed = value.trim();
	return (
		trimmed.length > 0 &&
		trimmed !== "." &&
		trimmed !== ".." &&
		![...trimmed].some((character) => {
			const code = character.charCodeAt(0);
			return (
				character === "/" || character === "\\" || code < 32 || code === 127
			);
		})
	);
}

export function defaultProfessionalExportFileName({
	projectName,
	revision,
	format,
}: {
	projectName: string;
	revision: number;
	format: ProfessionalExportFormat;
}): string {
	const descriptor = getProfessionalExportDescriptor(format);
	return `${sanitizePortableFileStem(projectName)}-r${revision}.${descriptor.extension}`;
}

export function withProfessionalExportExtension({
	fileName,
	format,
}: {
	fileName: string;
	format: ProfessionalExportFormat;
}): string {
	const extension = getProfessionalExportDescriptor(format).extension;
	const current = fileName.trim();
	const withoutExtension = current.replace(/\.[^.]+$/, "");
	return `${sanitizePortableFileStem(withoutExtension)}.${extension}`;
}

export interface BuildProfessionalExportArgumentsInput {
	projectId: string;
	expectedRevision: number;
	format: ProfessionalExportFormat;
	outputPath: string;
	allowOverwrite: boolean;
	range?: { startSeconds: number; endSeconds: number };
	resolution?: { width: number; height: number };
	fps?: number | { numerator: number; denominator: number };
	captionTrackId?: string;
}

export function buildProfessionalExportArguments(
	input: BuildProfessionalExportArgumentsInput,
): Record<string, unknown> {
	const descriptor = getProfessionalExportDescriptor(input.format);
	const settings: Record<string, unknown> = {};
	if (descriptor.supportsRange && input.range) settings.range = input.range;
	if (descriptor.supportsResolution && input.resolution)
		settings.resolution = input.resolution;
	if (descriptor.supportsFrameRate && input.fps) settings.fps = input.fps;
	if (descriptor.category === "subtitle" && input.captionTrackId)
		settings.captionTrackId = input.captionTrackId;
	return {
		projectId: input.projectId,
		expectedRevision: input.expectedRevision,
		format: input.format,
		outputPath: withProfessionalExportExtension({
			fileName: input.outputPath,
			format: input.format,
		}),
		allowOverwrite: input.allowOverwrite,
		...(Object.keys(settings).length > 0 ? { settings } : {}),
	};
}

export function exportFileNameFromJob(job: JobRecord): string {
	const value = job.input?.outputFileName;
	return typeof value === "string" ? value : `export-${job.id}`;
}

export function exportFormatFromJob(
	job: JobRecord,
): ProfessionalExportFormat | null {
	const options = isRecord(job.input?.options) ? job.input.options : undefined;
	const plan = isRecord(options?.plan) ? options.plan : undefined;
	const value = plan?.format ?? options?.format;
	return typeof value === "string" && isProfessionalExportFormat(value)
		? value
		: null;
}

export function exportOutputPath(job: JobRecord): string | null {
	const value = job.output?.outputPath;
	return typeof value === "string" ? value : null;
}

export function exportOutputVerified(job: JobRecord): boolean {
	return job.output?.verified === true;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}
