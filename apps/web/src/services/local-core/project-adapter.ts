import type { FrameRate } from "opencut-wasm";
import type { TProject, TProjectSettings } from "@/project/types";
import type {
	CaptionCue,
	CaptionElement,
	CaptionWord,
	SceneTracks,
	TimelineElement,
	TimelineTrack,
	TScene,
} from "@/timeline/types";
import type { MediaTime } from "@/wasm";
import type { MotionGraphicDefinition } from "@/motion-graphics/types";

type JsonPrimitive = boolean | number | string | null;
type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
type JsonRecord = { [key: string]: JsonValue };

export type DomainTrackKind =
	| "video"
	| "audio"
	| "text"
	| "caption"
	| "graphic"
	| "effect";

export type DomainMediaKind = "video" | "image" | "audio";

export interface DomainCaptionStyle {
	fontFamily: string;
	fontSize: number;
	textColor: string;
	activeWordColor: string;
	backgroundColor: string;
	outlineColor: string;
	outlineWidth: number;
	positionX: number;
	positionY: number;
	maxWidth: number;
	lineHeight: number;
	textAlign: "start" | "center" | "end";
	[key: string]: unknown;
}

export interface DomainCaptionElement {
	transcriptId: string;
	wordIds: string[];
	language: string;
	translationOfLanguage?: string;
	speakerId?: string;
	presetId?: string;
	style: DomainCaptionStyle;
	[key: string]: unknown;
}

interface DomainTimelineItemBase {
	id: string;
	name: string;
	startTicks: number;
	durationTicks: number;
	sourceRange?: { inTicks: number; outTicks: number };
	sourceDurationTicks?: number;
	linkGroupId?: string;
	timelineAnchor?: {
		transcriptId: string;
		wordId: string;
		edge: "start" | "end";
		bias: "before" | "after" | "nearest";
		fallbackTicks: number;
	};
	enabled: boolean;
	[key: string]: unknown;
}

export type DomainTimelineItemContent =
	| { type: "media"; assetId: string; mediaKind: DomainMediaKind }
	| { type: "text"; text: string }
	| { type: "caption"; caption: DomainCaptionElement }
	| {
			type: "motionGraphic";
			motionGraphic: MotionGraphicDefinition;
	  }
	| { type: "sticker"; stickerId: string }
	| { type: "effect"; effectType: string }
	| { type: "custom"; customType: string; data: unknown };

export type DomainTimelineItem = DomainTimelineItemBase & {
	content: DomainTimelineItemContent;
};

export interface DomainTrack {
	id: string;
	name: string;
	kind: DomainTrackKind;
	muted: boolean;
	hidden: boolean;
	locked: boolean;
	items: DomainTimelineItem[];
	[key: string]: unknown;
}

export interface DomainScene {
	id: string;
	name: string;
	isMain: boolean;
	tracks: DomainTrack[];
	bookmarks: Array<{
		timeTicks: number;
		durationTicks?: number;
		note?: string;
		color?: string;
	}>;
	[key: string]: unknown;
}

export interface DomainTranscriptWord {
	id: string;
	spokenText: string;
	displayText: string;
	startTicks: number;
	endTicks: number;
	speakerId?: string;
	deleted: boolean;
	confidence?: number;
	[key: string]: unknown;
}

export interface DomainTranscriptDocument {
	id: string;
	assetId?: string;
	language: string;
	speakers: Array<{ id: string; label: string; color?: string }>;
	words: DomainTranscriptWord[];
	segments: Array<{ id: string; wordIds: string[]; speakerId?: string }>;
	[key: string]: unknown;
}

export interface DomainAsset {
	id: string;
	name: string;
	kind: "video" | "image" | "audio" | "font" | "other";
	contentHash?: string;
	durationTicks?: number;
	width?: number;
	height?: number;
	hasAudio: boolean;
	provenance: {
		type: "imported" | "generated" | "derived";
		[key: string]: unknown;
	};
	managedMedia?: {
		byteSize?: number;
		mimeType?: string;
		lastModified?: number;
		source?: string;
	};
	linkedFile?: {
		version: 1;
		path?: string;
		byteSize: number;
		fingerprintSha256: string;
		mimeType?: string;
		portable: false;
	};
	derivatives?: {
		thumbnail?: { contentHash: string; mimeType?: string };
		contactSheet?: { contentHash: string; mimeType?: string };
		waveform?: { contentHash: string; mimeType?: string };
		proxy?: { contentHash: string; mimeType?: string };
		audio?: { contentHash: string; mimeType?: string };
	};
	mediaAnalysis?: {
		version: number;
		durationSeconds: number;
		representativeFrameTimesSeconds: number[];
		sceneChangeTimesSeconds: number[];
		sceneThreshold: number;
		method?: string;
	};
	[key: string]: unknown;
}

export interface DomainProjectDocument {
	schemaVersion: number;
	id: string;
	name: string;
	settings: {
		fps: { numerator: number; denominator: number };
		canvasSize: { width: number; height: number };
		background:
			| { type: "color"; color: string }
			| { type: "blur"; blurIntensity: number };
		[key: string]: unknown;
	};
	scenes: DomainScene[];
	currentSceneId?: string;
	assets: DomainAsset[];
	transcripts: DomainTranscriptDocument[];
	storySequences: unknown[];
	[key: string]: unknown;
}

export interface DomainProjectEnvelope {
	document: DomainProjectDocument;
	revision: number;
	documentHash: string;
}

interface ConversionContext {
	assets: Map<string, DomainAsset>;
	transcripts: Map<
		string,
		{
			document: DomainTranscriptDocument;
			wordIds: Set<string>;
			segmentIds: Set<string>;
			speakerIds: Set<string>;
		}
	>;
	settings: TProjectSettings;
}

const RUNTIME_ONLY_KEYS = new Set(["buffer"]);
const UNSAFE_OBJECT_KEYS = new Set(["__proto__", "constructor", "prototype"]);

function toJsonValue({
	value,
	seen,
}: {
	value: unknown;
	seen: WeakSet<object>;
}): JsonValue | undefined {
	if (value === null) return null;
	if (value instanceof Date) return value.toISOString();
	if (typeof value === "string" || typeof value === "boolean") return value;
	if (typeof value === "number") return Number.isFinite(value) ? value : null;
	if (typeof value === "bigint") return value.toString();
	if (typeof value !== "object") return undefined;
	if (seen.has(value)) return undefined;
	seen.add(value);

	if (Array.isArray(value)) {
		const result: JsonValue[] = [];
		for (const entry of value) {
			const converted = toJsonValue({ value: entry, seen });
			if (converted !== undefined) result.push(converted);
		}
		seen.delete(value);
		return result;
	}

	if (ArrayBuffer.isView(value)) {
		seen.delete(value);
		return Array.from(
			new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
		);
	}

	const result: JsonRecord = {};
	for (const [key, entry] of Object.entries(value)) {
		if (RUNTIME_ONLY_KEYS.has(key) || UNSAFE_OBJECT_KEYS.has(key)) continue;
		const converted = toJsonValue({ value: entry, seen });
		if (converted !== undefined) result[key] = converted;
	}
	seen.delete(value);
	return result;
}

function serializableRecord(value: unknown): JsonRecord {
	const converted = toJsonValue({ value, seen: new WeakSet() });
	return isRecord(converted) ? converted : {};
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringValue(value: unknown, fallback: string): string {
	return typeof value === "string" ? value : fallback;
}

function finiteNumber(value: unknown, fallback: number): number {
	return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function positiveInteger(value: unknown, fallback: number): number {
	const number = finiteNumber(value, fallback);
	return Number.isInteger(number) && number > 0 ? number : fallback;
}

function optionalString(value: unknown): string | undefined {
	return typeof value === "string" && value.length > 0 ? value : undefined;
}

function optionalNumber(value: unknown): number | undefined {
	return typeof value === "number" && Number.isFinite(value)
		? value
		: undefined;
}

function optionalBoolean(value: unknown): boolean | undefined {
	return typeof value === "boolean" ? value : undefined;
}

function toMediaTime(value: unknown, fallback = 0): MediaTime {
	// The daemon contract already stores integer ticks. Re-brand after enforcing
	// that invariant here instead of importing the browser-only Wasm runtime.
	return Math.round(finiteNumber(value, fallback)) as MediaTime;
}

function dateValue(value: unknown, fallback: Date): Date {
	if (value instanceof Date && !Number.isNaN(value.getTime())) {
		return new Date(value.getTime());
	}
	if (typeof value === "string" || typeof value === "number") {
		const parsed = new Date(value);
		if (!Number.isNaN(parsed.getTime())) return parsed;
	}
	return new Date(fallback.getTime());
}

function getParam(
	params: Record<string, unknown>,
	key: string,
	fallback: unknown,
): unknown {
	return Object.hasOwn(params, key) ? params[key] : fallback;
}

function textAlignToDomain(value: unknown): "start" | "center" | "end" {
	if (value === "left" || value === "start") return "start";
	if (value === "right" || value === "end") return "end";
	return "center";
}

function textAlignToClassic(value: unknown): "left" | "center" | "right" {
	if (value === "start" || value === "left") return "left";
	if (value === "end" || value === "right") return "right";
	return "center";
}

function alphaIsVisible(color: string): boolean {
	if (/^#[0-9a-f]{8}$/iu.test(color)) return color.slice(7, 9) !== "00";
	return color !== "transparent";
}

function captionStyle({
	element,
	settings,
}: {
	element: CaptionElement;
	settings: TProjectSettings;
}): DomainCaptionStyle {
	const params = element.params as Record<string, unknown>;
	const positionX = finiteNumber(getParam(params, "transform.positionX", 0), 0);
	const positionY = finiteNumber(getParam(params, "transform.positionY", 0), 0);
	const backgroundEnabled =
		getParam(params, "background.enabled", false) === true;
	const backgroundColor = stringValue(
		getParam(params, "background.color", "#00000000"),
		"#00000000",
	);

	return {
		fontFamily: stringValue(getParam(params, "fontFamily", "Inter"), "Inter"),
		fontSize: finiteNumber(getParam(params, "fontSize", 64), 64),
		textColor: stringValue(getParam(params, "color", "#ffffff"), "#ffffff"),
		activeWordColor: element.highlightColor,
		backgroundColor: backgroundEnabled ? backgroundColor : "#00000000",
		outlineColor: stringValue(
			getParam(params, "stroke.color", "#000000"),
			"#000000",
		),
		outlineWidth: finiteNumber(getParam(params, "stroke.width", 0), 0),
		positionX: 0.5 + positionX / settings.canvasSize.width,
		positionY: 0.5 + positionY / settings.canvasSize.height,
		maxWidth: finiteNumber(getParam(params, "maxWidth", 0.85), 0.85),
		lineHeight: finiteNumber(getParam(params, "lineHeight", 1.15), 1.15),
		textAlign: textAlignToDomain(getParam(params, "textAlign", "center")),
		classicCaptionStyle: serializableRecord(params),
	};
}

function transcriptWordId(word: CaptionWord): string {
	return word.transcriptWordId ?? word.id;
}

function ensureTranscript({
	context,
	transcriptId,
	language,
}: {
	context: ConversionContext;
	transcriptId: string;
	language: string;
}) {
	let transcript = context.transcripts.get(transcriptId);
	if (!transcript) {
		transcript = {
			document: {
				id: transcriptId,
				language,
				speakers: [],
				words: [],
				segments: [],
				classicTranscript: { source: "classic-caption-element" },
			},
			wordIds: new Set(),
			segmentIds: new Set(),
			speakerIds: new Set(),
		};
		context.transcripts.set(transcriptId, transcript);
	}
	return transcript;
}

function registerCaptionTranscript({
	element,
	context,
}: {
	element: CaptionElement;
	context: ConversionContext;
}): string[] {
	const language = element.language ?? "und";
	const transcript = ensureTranscript({
		context,
		transcriptId: element.transcriptId,
		language,
	});
	const captionWordIds: string[] = [];

	for (const cue of element.cues) {
		const segmentWordIds: string[] = [];
		if (cue.speakerId && !transcript.speakerIds.has(cue.speakerId)) {
			transcript.speakerIds.add(cue.speakerId);
			transcript.document.speakers.push({
				id: cue.speakerId,
				label: cue.speakerId,
			});
		}

		for (const word of cue.words) {
			const wordId = transcriptWordId(word);
			if (!captionWordIds.includes(wordId)) captionWordIds.push(wordId);
			if (!segmentWordIds.includes(wordId)) segmentWordIds.push(wordId);
			if (transcript.wordIds.has(wordId)) continue;
			transcript.wordIds.add(wordId);
			transcript.document.words.push({
				id: wordId,
				spokenText: word.spokenText || word.displayText || "Caption",
				displayText: word.displayText || word.spokenText || "Caption",
				startTicks: element.startTime + word.startTime,
				endTicks: Math.max(
					element.startTime + word.startTime + 1,
					element.startTime + word.endTime,
				),
				...(cue.speakerId ? { speakerId: cue.speakerId } : {}),
				deleted: false,
				classicCaptionWord: serializableRecord(word),
			});
		}

		if (segmentWordIds.length > 0) {
			let segmentId = `segment:${element.id}:${cue.id}`;
			let suffix = 1;
			while (transcript.segmentIds.has(segmentId)) {
				segmentId = `segment:${element.id}:${cue.id}:${suffix++}`;
			}
			transcript.segmentIds.add(segmentId);
			transcript.document.segments.push({
				id: segmentId,
				wordIds: segmentWordIds,
				...(cue.speakerId ? { speakerId: cue.speakerId } : {}),
			});
		}
	}

	if (captionWordIds.length === 0) {
		const wordId = `caption-word:${element.id}`;
		const content =
			stringValue(
				(element.params as Record<string, unknown>).content,
				"Caption",
			).trim() || "Caption";
		captionWordIds.push(wordId);
		if (!transcript.wordIds.has(wordId)) {
			transcript.wordIds.add(wordId);
			transcript.document.words.push({
				id: wordId,
				spokenText: content,
				displayText: content,
				startTicks: element.startTime,
				endTicks: element.startTime + Math.max(1, element.duration),
				deleted: false,
			});
		}
	}

	return captionWordIds;
}

function registerAsset({
	context,
	id,
	name,
	kind,
	durationTicks,
	hasAudio,
	extensions,
}: {
	context: ConversionContext;
	id: string;
	name: string;
	kind: DomainMediaKind;
	durationTicks?: number;
	hasAudio?: boolean;
	extensions?: JsonRecord;
}): void {
	const existing = context.assets.get(id);
	if (existing) {
		if (
			(existing.kind === "video" && kind === "audio") ||
			(existing.kind === "audio" && kind === "video")
		) {
			// Classic represents a video's picture and embedded sound as two
			// timeline elements with the same mediaId. Keep one managed video
			// asset and mark it audible instead of creating an invalid duplicate.
			existing.kind = "video";
			existing.hasAudio = true;
		}
		if (existing.kind === kind && (hasAudio || kind === "audio")) {
			existing.hasAudio = true;
		}
		if (durationTicks && durationTicks > (existing.durationTicks ?? 0)) {
			existing.durationTicks = durationTicks;
		}
		if (extensions) Object.assign(existing, extensions);
		return;
	}
	context.assets.set(id, {
		id,
		name,
		kind,
		...(durationTicks && durationTicks > 0 ? { durationTicks } : {}),
		hasAudio: hasAudio ?? false,
		provenance: { type: "imported", sourceName: name },
		...(extensions ?? {}),
	});
}

function itemSourceRange(
	element: TimelineElement,
): { inTicks: number; outTicks: number } | undefined {
	const trimStart = Math.max(0, element.trimStart);
	const sourceDuration = element.sourceDuration;
	const outTicks = sourceDuration
		? sourceDuration - Math.max(0, element.trimEnd)
		: trimStart + element.duration;
	return outTicks > trimStart ? { inTicks: trimStart, outTicks } : undefined;
}

function toDomainItem({
	element,
	context,
}: {
	element: TimelineElement;
	context: ConversionContext;
}): DomainTimelineItem {
	const dynamicElement = element as TimelineElement & { linkGroupId?: string };
	const timelineAnchor = isRecord(
		(dynamicElement as TimelineElement & { timelineAnchor?: unknown }).timelineAnchor,
	)
		? ((dynamicElement as TimelineElement & { timelineAnchor: DomainTimelineItemBase["timelineAnchor"] })
				.timelineAnchor)
		: undefined;
	const common: DomainTimelineItemBase = {
		id: element.id,
		name: element.name,
		startTicks: element.startTime,
		durationTicks: element.duration,
		...(itemSourceRange(element)
			? { sourceRange: itemSourceRange(element) }
			: {}),
		...(element.sourceDuration && element.sourceDuration > 0
			? { sourceDurationTicks: element.sourceDuration }
			: {}),
		...(dynamicElement.linkGroupId
			? { linkGroupId: dynamicElement.linkGroupId }
			: {}),
		...(timelineAnchor ? { timelineAnchor } : {}),
		...(isRecord(dynamicElement.storyCrossfade)
			? { storyCrossfade: serializableRecord(dynamicElement.storyCrossfade) }
			: {}),
		enabled: !("hidden" in element && element.hidden === true),
		classicElement: serializableRecord(element),
	};

	switch (element.type) {
		case "video":
			registerAsset({
				context,
				id: element.mediaId,
				name: element.name,
				kind: "video",
				durationTicks: element.sourceDuration ?? element.duration,
				hasAudio: element.isSourceAudioEnabled,
			});
			return {
				...common,
				content: {
					type: "media",
					assetId: element.mediaId,
					mediaKind: "video",
				},
			};
		case "image":
			registerAsset({
				context,
				id: element.mediaId,
				name: element.name,
				kind: "image",
				durationTicks: element.sourceDuration ?? element.duration,
			});
			return {
				...common,
				content: {
					type: "media",
					assetId: element.mediaId,
					mediaKind: "image",
				},
			};
		case "audio": {
			const assetId =
				element.sourceType === "upload"
					? element.mediaId
					: `library:${element.id}`;
			registerAsset({
				context,
				id: assetId,
				name: element.name,
				kind: "audio",
				durationTicks: element.sourceDuration ?? element.duration,
				extensions:
					element.sourceType === "library"
						? { classicSourceUrl: element.sourceUrl }
						: undefined,
			});
			return {
				...common,
				content: { type: "media", assetId, mediaKind: "audio" },
			};
		}
		case "text":
			if (element.semanticType === "caption") {
				const wordIds = registerCaptionTranscript({ element, context });
				const speakers = new Set(
					element.cues
						.map((cue) => cue.speakerId)
						.filter((speaker): speaker is string => Boolean(speaker)),
				);
				const caption: DomainCaptionElement = {
					transcriptId: element.transcriptId,
					wordIds,
					language: element.language ?? "und",
					...(element.translationLanguage
						? { translationOfLanguage: element.translationLanguage }
						: {}),
					...(speakers.size === 1 ? { speakerId: [...speakers][0] } : {}),
					...(element.stylePresetId ? { presetId: element.stylePresetId } : {}),
					style: captionStyle({ element, settings: context.settings }),
					classicCaption: serializableRecord({
						language: element.language,
						translationLanguage: element.translationLanguage,
						stylePresetId: element.stylePresetId,
						maxLines: element.maxLines,
						maxCharactersPerLine: element.maxCharactersPerLine,
						wordHighlight: element.wordHighlight,
						highlightColor: element.highlightColor,
						cues: element.cues,
					}),
				};
				return { ...common, content: { type: "caption", caption } };
			}
			return {
				...common,
				content: {
					type: "text",
					text: stringValue(
						(element.params as Record<string, unknown>).content,
						"",
					),
				},
			};
		case "sticker":
			return {
				...common,
				content: { type: "sticker", stickerId: element.stickerId },
			};
		case "graphic":
			return {
				...common,
				content: {
					type: "motionGraphic",
					motionGraphic: element.motionGraphic ?? {
						dslVersion: 1,
						definition: {
							type: "classicGraphic",
							definitionId: element.definitionId,
							params: serializableRecord(element.params),
						},
						templateId: element.definitionId,
					},
				},
			};
		case "effect":
			return {
				...common,
				content: { type: "effect", effectType: element.effectType },
			};
	}
}

function trackKind(track: TimelineTrack): DomainTrackKind {
	if (
		track.type === "text" &&
		track.elements.length > 0 &&
		track.elements.every((element) => element.semanticType === "caption")
	) {
		return "caption";
	}
	return track.type;
}

function toDomainTrack({
	track,
	zone,
	index,
	context,
}: {
	track: TimelineTrack;
	zone: "overlay" | "main" | "audio";
	index: number;
	context: ConversionContext;
}): DomainTrack {
	const dynamicTrack = track as TimelineTrack & { locked?: boolean };
	return {
		id: track.id,
		name: track.name,
		kind: trackKind(track),
		muted: "muted" in track ? track.muted : false,
		hidden: "hidden" in track ? track.hidden : false,
		locked: dynamicTrack.locked ?? false,
		items: track.elements.map((element) => toDomainItem({ element, context })),
		classicZone: zone,
		classicIndex: index,
		classicTrack: serializableRecord(track),
	};
}

function toDomainScene({
	scene,
	context,
}: {
	scene: TScene;
	context: ConversionContext;
}): DomainScene {
	return {
		id: scene.id,
		name: scene.name,
		isMain: scene.isMain,
		tracks: [
			...scene.tracks.overlay.map((track, index) =>
				toDomainTrack({ track, zone: "overlay", index, context }),
			),
			toDomainTrack({
				track: scene.tracks.main,
				zone: "main",
				index: 0,
				context,
			}),
			...scene.tracks.audio.map((track, index) =>
				toDomainTrack({ track, zone: "audio", index, context }),
			),
		],
		bookmarks: scene.bookmarks.map((bookmark) => ({
			timeTicks: bookmark.time,
			...(bookmark.duration !== undefined
				? { durationTicks: bookmark.duration }
				: {}),
			...(bookmark.note !== undefined ? { note: bookmark.note } : {}),
			...(bookmark.color !== undefined ? { color: bookmark.color } : {}),
		})),
		classicScene: serializableRecord(scene),
	};
}

function finalizeTranscripts(
	transcripts: ConversionContext["transcripts"],
): DomainTranscriptDocument[] {
	return [...transcripts.values()].map(({ document }) => {
		document.words.sort(
			(left, right) =>
				left.startTicks - right.startTicks || left.endTicks - right.endTicks,
		);
		return document;
	});
}

/**
 * Convert the Classic browser project into the daemon's serde JSON shape.
 *
 * Rust `Extensions` fields are `#[serde(flatten)]`, so adapter metadata such as
 * `classicZone` is deliberately emitted beside typed fields instead of inside a
 * literal `extensions` object.
 */
export function toDomainProjectDocument({
	project,
}: {
	project: TProject;
}): DomainProjectDocument {
	const context: ConversionContext = {
		assets: new Map(),
		transcripts: new Map(),
		settings: project.settings,
	};
	const settings = project.settings;
	return {
		schemaVersion: 1,
		id: project.metadata.id,
		name: project.metadata.name,
		settings: {
			fps: {
				numerator: settings.fps.numerator,
				denominator: settings.fps.denominator,
			},
			canvasSize: {
				width: settings.canvasSize.width,
				height: settings.canvasSize.height,
			},
			background:
				settings.background.type === "blur"
					? {
							type: "blur",
							blurIntensity: settings.background.blurIntensity,
						}
					: { type: "color", color: settings.background.color },
			classicSettings: serializableRecord(settings),
		},
		scenes: project.scenes.map((scene) => toDomainScene({ scene, context })),
		...(project.currentSceneId
			? { currentSceneId: project.currentSceneId }
			: {}),
		assets: [...context.assets.values()],
		transcripts: finalizeTranscripts(context.transcripts),
		storySequences: [],
		classicMetadata: serializableRecord(project.metadata),
		classicProject: serializableRecord({
			version: project.version,
			timelineViewState: project.timelineViewState,
		}),
	};
}

function transcriptMap(document: DomainProjectDocument) {
	return new Map(
		document.transcripts.map((transcript) => [transcript.id, transcript]),
	);
}

function materializeCaptionWords({
	item,
	caption,
	base,
	transcripts,
}: {
	item: DomainTimelineItem;
	caption: DomainCaptionElement;
	base: Record<string, unknown>;
	transcripts: Map<string, DomainTranscriptDocument>;
}): CaptionCue[] {
	const transcript = transcripts.get(caption.transcriptId);
	const words = new Map(transcript?.words.map((word) => [word.id, word]) ?? []);
	const translatedDisplayText = isRecord(caption.translatedDisplayText)
		? caption.translatedDisplayText
		: {};
	const allowed = new Set(caption.wordIds);
	const used = new Set<string>();
	const classicCues = Array.isArray(base.cues) ? base.cues : [];
	const cues: CaptionCue[] = [];

	for (const [cueIndex, rawCue] of classicCues.entries()) {
		if (!isRecord(rawCue)) continue;
		const classicWords = Array.isArray(rawCue.words) ? rawCue.words : [];
		const cueWords: CaptionWord[] = [];
		for (const rawWord of classicWords) {
			if (!isRecord(rawWord)) continue;
			const originalId = stringValue(rawWord.id, "");
			const wordId = stringValue(rawWord.transcriptWordId, originalId);
			if (!wordId || !allowed.has(wordId)) continue;
			const domainWord = words.get(wordId);
			used.add(wordId);
			cueWords.push({
				id: originalId || wordId,
				spokenText:
					domainWord?.spokenText ?? stringValue(rawWord.spokenText, "Caption"),
				displayText: stringValue(
					translatedDisplayText[wordId],
					domainWord?.displayText ??
						stringValue(rawWord.displayText, "Caption"),
				),
				startTime: toMediaTime(rawWord.startTime),
				endTime: toMediaTime(
					rawWord.endTime,
					finiteNumber(rawWord.startTime, 0) + 1,
				),
				...(wordId !== (originalId || wordId)
					? { transcriptWordId: wordId }
					: optionalString(rawWord.transcriptWordId)
						? { transcriptWordId: wordId }
						: {}),
			});
		}
		if (cueWords.length === 0) continue;
		cues.push({
			...serializableRecord(rawCue),
			id: stringValue(rawCue.id, `caption-cue:${item.id}:${cueIndex}`),
			startTime: toMediaTime(rawCue.startTime, cueWords[0].startTime),
			endTime: toMediaTime(
				rawCue.endTime,
				cueWords[cueWords.length - 1].endTime,
			),
			words: cueWords,
			...(caption.speakerId ? { speakerId: caption.speakerId } : {}),
		} as CaptionCue);
	}

	const missing = caption.wordIds.filter((wordId) => !used.has(wordId));
	if (missing.length > 0) {
		const missingWords = missing.flatMap((wordId): CaptionWord[] => {
			const word = words.get(wordId);
			if (!word) return [];
			const relativeStart = Math.max(0, word.startTicks - item.startTicks);
			const relativeEnd = Math.max(
				relativeStart + 1,
				word.endTicks - item.startTicks,
			);
			return [
				{
					id: wordId,
					spokenText: word.spokenText,
					displayText: stringValue(
						translatedDisplayText[wordId],
						word.displayText,
					),
					startTime: toMediaTime(relativeStart),
					endTime: toMediaTime(relativeEnd),
				},
			];
		});
		if (missingWords.length > 0) {
			cues.push({
				id: `caption-cue:${item.id}:materialized`,
				startTime: missingWords[0].startTime,
				endTime: missingWords[missingWords.length - 1].endTime,
				words: missingWords,
				...(caption.speakerId ? { speakerId: caption.speakerId } : {}),
			});
		}
	}

	return cues;
}

function materializeCaption({
	item,
	caption,
	base,
	settings,
	transcripts,
}: {
	item: DomainTimelineItem;
	caption: DomainCaptionElement;
	base: Record<string, unknown>;
	settings: TProjectSettings;
	transcripts: Map<string, DomainTranscriptDocument>;
}): Record<string, unknown> {
	const classicCaption = isRecord(caption.classicCaption)
		? caption.classicCaption
		: {};
	const params = isRecord(base.params) ? { ...base.params } : {};
	const style = caption.style;
	params.fontFamily = style.fontFamily;
	params.fontSize = style.fontSize;
	params.color = style.textColor;
	params.lineHeight = style.lineHeight;
	params.textAlign = textAlignToClassic(style.textAlign);
	params.maxWidth = style.maxWidth;
	params["background.enabled"] = alphaIsVisible(style.backgroundColor);
	params["background.color"] = style.backgroundColor;
	params["stroke.color"] = style.outlineColor;
	params["stroke.width"] = style.outlineWidth;
	params["transform.positionX"] =
		(style.positionX - 0.5) * settings.canvasSize.width;
	params["transform.positionY"] =
		(style.positionY - 0.5) * settings.canvasSize.height;

	const captionBase = {
		...base,
		type: "text",
		semanticType: "caption",
		transcriptId: caption.transcriptId,
		language: caption.language,
		...(caption.translationOfLanguage
			? { translationLanguage: caption.translationOfLanguage }
			: {}),
		stylePresetId:
			caption.presetId ?? stringValue(classicCaption.stylePresetId, "classic"),
		maxLines: positiveInteger(classicCaption.maxLines, 2),
		maxCharactersPerLine: positiveInteger(
			classicCaption.maxCharactersPerLine,
			32,
		),
		wordHighlight:
			optionalBoolean(classicCaption.wordHighlight) ??
			style.activeWordColor !== style.textColor,
		highlightColor: style.activeWordColor,
		params,
	};
	return {
		...captionBase,
		cues: materializeCaptionWords({
			item,
			caption,
			base: captionBase,
			transcripts,
		}),
	};
}

function materializeSourceTiming({
	item,
	base,
}: {
	item: DomainTimelineItem;
	base: Record<string, unknown>;
}): void {
	if (item.sourceRange) {
		base.trimStart = toMediaTime(item.sourceRange.inTicks);
		if (item.sourceDurationTicks !== undefined) {
			base.trimEnd = toMediaTime(
				Math.max(0, item.sourceDurationTicks - item.sourceRange.outTicks),
			);
		} else {
			base.trimEnd = toMediaTime(0);
		}
	}
	if (item.sourceDurationTicks !== undefined) {
		base.sourceDuration = toMediaTime(item.sourceDurationTicks);
	}
}

function fromDomainItem({
	item,
	settings,
	assets,
	transcripts,
}: {
	item: DomainTimelineItem;
	settings: TProjectSettings;
	assets: Map<string, DomainAsset>;
	transcripts: Map<string, DomainTranscriptDocument>;
}): TimelineElement {
	let base = isRecord(item.classicElement) ? { ...item.classicElement } : {};
	base.id = item.id;
	base.name = item.name;
	base.startTime = toMediaTime(item.startTicks);
	base.duration = toMediaTime(item.durationTicks);
	if (!("trimStart" in base)) base.trimStart = toMediaTime(0);
	if (!("trimEnd" in base)) base.trimEnd = toMediaTime(0);
	if (!isRecord(base.params)) base.params = {};
	materializeSourceTiming({ item, base });
	if (item.linkGroupId) base.linkGroupId = item.linkGroupId;
	if (item.timelineAnchor) base.timelineAnchor = { ...item.timelineAnchor };
	if (isRecord(item.storyCrossfade)) {
		base.storyCrossfade = { ...item.storyCrossfade };
	}
	const content = item.content;

	switch (content.type) {
		case "media": {
			const asset = assets.get(content.assetId);
			if (content.mediaKind === "audio") {
				base.type = "audio";
				if (base.sourceType === "library") {
					base.sourceUrl =
						optionalString(base.sourceUrl) ??
						optionalString(asset?.classicSourceUrl) ??
						"";
				} else {
					base.sourceType = "upload";
					base.mediaId = content.assetId;
				}
			} else {
				base.type = content.mediaKind;
				base.mediaId = content.assetId;
			}
			break;
		}
		case "text": {
			base.type = "text";
			if (base.semanticType === "caption") base.semanticType = "text";
			const params = isRecord(base.params) ? { ...base.params } : {};
			params.content = content.text;
			base.params = params;
			break;
		}
		case "caption":
			base = materializeCaption({
				item,
				caption: content.caption,
				base,
				settings,
				transcripts,
			});
			break;
		case "motionGraphic":
			base.type = "graphic";
			base.definitionId =
				content.motionGraphic.templateId ??
				optionalString(base.definitionId) ??
				`motion-graphic:${item.id}`;
			base.motionGraphic = content.motionGraphic;
			break;
		case "sticker":
			base.type = "sticker";
			base.stickerId = content.stickerId;
			break;
		case "effect":
			base.type = "effect";
			base.effectType = content.effectType;
			break;
		case "custom":
			base.type = optionalString(base.type) ?? content.customType;
			base.customData = content.data;
			break;
	}

	if (
		"hidden" in base ||
		content.type !== "media" ||
		content.mediaKind !== "audio"
	) {
		base.hidden = !item.enabled;
	}
	return base as unknown as TimelineElement;
}

function classicTrackType(kind: DomainTrackKind): TimelineTrack["type"] {
	if (kind === "caption") return "text";
	return kind;
}

function fromDomainTrack({
	track,
	settings,
	assets,
	transcripts,
}: {
	track: DomainTrack;
	settings: TProjectSettings;
	assets: Map<string, DomainAsset>;
	transcripts: Map<string, DomainTranscriptDocument>;
}): TimelineTrack {
	const base = isRecord(track.classicTrack) ? { ...track.classicTrack } : {};
	const type = classicTrackType(track.kind);
	base.id = track.id;
	base.name = track.name;
	base.type = type;
	base.elements = track.items.map((item) =>
		fromDomainItem({ item, settings, assets, transcripts }),
	);
	if (type === "video" || type === "audio") base.muted = track.muted;
	else base.hidden = track.hidden;
	if (track.locked || "locked" in base) base.locked = track.locked;
	return base as unknown as TimelineTrack;
}

function trackZone(track: DomainTrack): "overlay" | "main" | "audio" | null {
	if (
		track.classicZone === "overlay" ||
		track.classicZone === "main" ||
		track.classicZone === "audio"
	) {
		return track.classicZone;
	}
	return null;
}

function sortZoneTracks(
	tracks: Array<{ track: DomainTrack; domainIndex: number }>,
): Array<{ track: DomainTrack; domainIndex: number }> {
	return tracks.sort((left, right) => {
		const leftIndex =
			optionalNumber(left.track.classicIndex) ?? left.domainIndex;
		const rightIndex =
			optionalNumber(right.track.classicIndex) ?? right.domainIndex;
		return leftIndex - rightIndex || left.domainIndex - right.domainIndex;
	});
}

function emptyMainTrack(sceneId: string): TimelineTrack {
	return {
		id: `main:${sceneId}`,
		name: "Main",
		type: "video",
		elements: [],
		muted: false,
		hidden: false,
	};
}

function fromDomainScene({
	scene,
	settings,
	assets,
	transcripts,
	metadataCreatedAt,
	metadataUpdatedAt,
}: {
	scene: DomainScene;
	settings: TProjectSettings;
	assets: Map<string, DomainAsset>;
	transcripts: Map<string, DomainTranscriptDocument>;
	metadataCreatedAt: Date;
	metadataUpdatedAt: Date;
}): TScene {
	const base = isRecord(scene.classicScene) ? { ...scene.classicScene } : {};
	const classified = scene.tracks.map((track, domainIndex) => ({
		track,
		domainIndex,
		zone: trackZone(track),
	}));
	let main = classified.find(({ zone }) => zone === "main");
	if (!main) {
		main = classified.find(
			({ track, zone }) => zone === null && track.kind === "video",
		);
	}
	const audio = sortZoneTracks(
		classified
			.filter(
				({ track, zone }) =>
					zone === "audio" || (zone === null && track.kind === "audio"),
			)
			.map(({ track, domainIndex }) => ({ track, domainIndex })),
	).map(({ track }) =>
		fromDomainTrack({ track, settings, assets, transcripts }),
	);
	const overlay = sortZoneTracks(
		classified
			.filter(({ track, zone }) => {
				if (main?.track.id === track.id) return false;
				if (zone === "audio" || track.kind === "audio") return false;
				return zone === "overlay" || zone === null || zone === "main";
			})
			.map(({ track, domainIndex }) => ({ track, domainIndex })),
	).map(({ track }) =>
		fromDomainTrack({ track, settings, assets, transcripts }),
	);
	const mainTrack = main
		? fromDomainTrack({ track: main.track, settings, assets, transcripts })
		: emptyMainTrack(scene.id);

	const tracks: SceneTracks = {
		overlay: overlay as SceneTracks["overlay"],
		main: mainTrack as SceneTracks["main"],
		audio: audio as SceneTracks["audio"],
	};
	return {
		...(base as Partial<TScene>),
		id: scene.id,
		name: scene.name,
		isMain: scene.isMain,
		tracks,
		bookmarks: scene.bookmarks.map((bookmark) => ({
			time: toMediaTime(bookmark.timeTicks),
			...(bookmark.durationTicks !== undefined
				? { duration: toMediaTime(bookmark.durationTicks) }
				: {}),
			...(bookmark.note !== undefined ? { note: bookmark.note } : {}),
			...(bookmark.color !== undefined ? { color: bookmark.color } : {}),
		})),
		createdAt: dateValue(base.createdAt, metadataCreatedAt),
		updatedAt: dateValue(base.updatedAt, metadataUpdatedAt),
	};
}

function fromDomainSettings(
	domain: DomainProjectDocument["settings"],
): TProjectSettings {
	const classic = isRecord(domain.classicSettings)
		? { ...domain.classicSettings }
		: {};
	return {
		...(classic as Partial<TProjectSettings>),
		fps: {
			numerator: positiveInteger(domain.fps.numerator, 30),
			denominator: positiveInteger(domain.fps.denominator, 1),
		} as FrameRate,
		canvasSize: {
			width: positiveInteger(domain.canvasSize.width, 1920),
			height: positiveInteger(domain.canvasSize.height, 1080),
		},
		background:
			domain.background.type === "blur"
				? {
						type: "blur",
						blurIntensity: finiteNumber(domain.background.blurIntensity, 0),
					}
				: {
						type: "color",
						color: stringValue(domain.background.color, "#000000"),
					},
	};
}

/** Materialize a revisioned daemon document back into the Classic editor model. */
export function fromDomainProjectEnvelope({
	envelope,
}: {
	envelope: DomainProjectEnvelope;
}): TProject {
	const document = envelope.document;
	const classicMetadata = isRecord(document.classicMetadata)
		? document.classicMetadata
		: {};
	const classicProject = isRecord(document.classicProject)
		? document.classicProject
		: {};
	const createdAt = dateValue(classicMetadata.createdAt, new Date(0));
	const updatedAt = dateValue(classicMetadata.updatedAt, createdAt);
	const settings = fromDomainSettings(document.settings);
	const assets = new Map(document.assets.map((asset) => [asset.id, asset]));
	const transcripts = transcriptMap(document);
	const scenes = document.scenes.map((scene) =>
		fromDomainScene({
			scene,
			settings,
			assets,
			transcripts,
			metadataCreatedAt: createdAt,
			metadataUpdatedAt: updatedAt,
		}),
	);
	const currentSceneId =
		document.currentSceneId &&
		scenes.some((scene) => scene.id === document.currentSceneId)
			? document.currentSceneId
			: (scenes[0]?.id ?? "");

	return {
		metadata: {
			...(classicMetadata as Partial<TProject["metadata"]>),
			id: document.id,
			name: document.name,
			duration: toMediaTime(classicMetadata.duration),
			createdAt,
			updatedAt,
		},
		scenes,
		currentSceneId,
		settings,
		version: positiveInteger(classicProject.version, 1),
		...(isRecord(classicProject.timelineViewState)
			? {
					timelineViewState:
						classicProject.timelineViewState as unknown as TProject["timelineViewState"],
				}
			: {}),
	};
}
