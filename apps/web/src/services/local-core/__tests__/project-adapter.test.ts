import { describe, expect, test } from "bun:test";
import type { TProject } from "@/project/types";
import type { CaptionElement, VideoElement } from "@/timeline/types";
import type { MediaTime } from "@/wasm";
import {
	fromDomainProjectEnvelope,
	toDomainProjectDocument,
	type DomainProjectEnvelope,
} from "../project-adapter";

const ticks = (value: number) => value as MediaTime;

function representativeProject(): TProject {
	const createdAt = new Date("2026-07-01T01:02:03.000Z");
	const updatedAt = new Date("2026-07-02T04:05:06.000Z");
	const video: VideoElement = {
		id: "item-video",
		name: "Camera A",
		type: "video",
		mediaId: "asset-video",
		startTime: ticks(0),
		duration: ticks(1_200_000),
		trimStart: ticks(120_000),
		trimEnd: ticks(80_000),
		sourceDuration: ticks(1_400_000),
		isSourceAudioEnabled: true,
		hidden: false,
		retime: { rate: 1, maintainPitch: true },
		params: {
			"transform.positionX": 12,
			"transform.positionY": -8,
			"transform.scaleX": 1.1,
			opacity: 0.92,
		},
		animations: {
			opacity: {
				keys: [
					{
						id: "key-opacity",
						time: ticks(0),
						value: 0.25,
						segmentToNext: "linear",
						tangentMode: "auto",
					},
				],
			},
		},
		effects: [
			{
				id: "effect-color",
				type: "brightness-contrast",
				params: { brightness: 0.1, contrast: 1.05 },
				enabled: true,
			},
		],
		masks: [
			{
				id: "mask-one",
				type: "rectangle",
				params: {
					feather: 4,
					inverted: false,
					strokeColor: "#ffffff",
					strokeWidth: 0,
					strokeAlign: "center",
					centerX: 0.5,
					centerY: 0.5,
					width: 0.8,
					height: 0.8,
					rotation: 0,
					scale: 1,
				},
			},
		],
	};
	const captions: CaptionElement = {
		id: "item-caption",
		name: "English captions",
		type: "text",
		semanticType: "caption",
		transcriptId: "transcript:main",
		language: "en",
		stylePresetId: "caption-clean",
		maxLines: 2,
		maxCharactersPerLine: 30,
		wordHighlight: true,
		highlightColor: "#ffd60a",
		startTime: ticks(0),
		duration: ticks(600_000),
		trimStart: ticks(0),
		trimEnd: ticks(0),
		hidden: false,
		params: {
			content: "Hello world",
			fontFamily: "Inter",
			fontSize: 64,
			color: "#ffffff",
			lineHeight: 1.15,
			textAlign: "center",
			maxWidth: 0.85,
			"background.enabled": true,
			"background.color": "#00000099",
			"stroke.color": "#000000",
			"stroke.width": 3,
			"transform.positionX": 0,
			"transform.positionY": 378,
		},
		cues: [
			{
				id: "cue-one",
				startTime: ticks(0),
				endTime: ticks(600_000),
				speakerId: "speaker-one",
				words: [
					{
						id: "caption-word-one",
						transcriptWordId: "word-one",
						spokenText: "Hello",
						displayText: "Hello",
						startTime: ticks(0),
						endTime: ticks(240_000),
					},
					{
						id: "caption-word-two",
						transcriptWordId: "word-two",
						spokenText: "world",
						displayText: "world",
						startTime: ticks(250_000),
						endTime: ticks(600_000),
					},
				],
			},
		],
	};

	return {
		metadata: {
			id: "project-one",
			name: "Interview",
			thumbnail: "data:image/png;base64,fixture",
			duration: ticks(1_200_000),
			createdAt,
			updatedAt,
		},
		scenes: [
			{
				id: "scene-one",
				name: "Main story",
				isMain: true,
				createdAt,
				updatedAt,
				bookmarks: [
					{
						time: ticks(300_000),
						duration: ticks(120_000),
						note: "Keep this beat",
						color: "#ff0000",
					},
				],
				tracks: {
					overlay: [
						{
							id: "track-caption",
							name: "Captions",
							type: "text",
							hidden: false,
							elements: [captions],
						},
					],
					main: {
						id: "track-main",
						name: "Main",
						type: "video",
						muted: false,
						hidden: false,
						elements: [video],
					},
					audio: [
						{
							id: "track-audio",
							name: "Music",
							type: "audio",
							muted: false,
							elements: [
								{
									id: "item-audio",
									name: "Theme",
									type: "audio",
									sourceType: "upload",
									mediaId: "asset-audio",
									startTime: ticks(0),
									duration: ticks(1_200_000),
									trimStart: ticks(0),
									trimEnd: ticks(0),
									sourceDuration: ticks(1_200_000),
									params: { volume: 0.6 },
								},
							],
						},
					],
				},
			},
		],
		currentSceneId: "scene-one",
		settings: {
			fps: { numerator: 30_000, denominator: 1_001 },
			canvasSize: { width: 1920, height: 1080 },
			canvasSizeMode: "custom",
			lastCustomCanvasSize: { width: 1920, height: 1080 },
			originalCanvasSize: { width: 3840, height: 2160 },
			background: { type: "color", color: "#101114" },
		},
		version: 23,
		timelineViewState: {
			zoomLevel: 1.5,
			scrollLeft: 420,
			playheadTime: ticks(480_000),
		},
	};
}

function daemonRoundTrip(document: ReturnType<typeof toDomainProjectDocument>) {
	return JSON.parse(JSON.stringify(document)) as typeof document;
}

describe("Classic/local-core project adapter", () => {
	test("round-trips zones, captions, dates, settings, and unknown element fields", () => {
		const project = representativeProject();
		const document = toDomainProjectDocument({ project });

		expect(document.schemaVersion).toBe(1);
		expect(document.scenes[0].tracks.map((track) => track.classicZone)).toEqual(
			["overlay", "main", "audio"],
		);
		expect(document.scenes[0].tracks[0].kind).toBe("caption");
		expect(document.transcripts[0].words.map((word) => word.id)).toEqual([
			"word-one",
			"word-two",
		]);
		expect(document.assets.map((asset) => asset.id).sort()).toEqual([
			"asset-audio",
			"asset-video",
		]);

		const envelope: DomainProjectEnvelope = {
			document: daemonRoundTrip(document),
			revision: 0,
			documentHash: "fixture-hash",
		};
		const restored = fromDomainProjectEnvelope({ envelope });

		expect(restored.metadata).toEqual(project.metadata);
		expect(restored.metadata.createdAt).toBeInstanceOf(Date);
		expect(restored.settings).toEqual(project.settings);
		expect(restored.timelineViewState).toEqual(project.timelineViewState);
		expect(restored.scenes[0].createdAt).toBeInstanceOf(Date);
		expect(restored.scenes[0].tracks.main).toEqual(
			project.scenes[0].tracks.main,
		);
		expect(restored.scenes[0].tracks.audio).toEqual(
			project.scenes[0].tracks.audio,
		);
		expect(restored.scenes[0].tracks.overlay[0]).toEqual(
			project.scenes[0].tracks.overlay[0],
		);
	});

	test("materializes agent project, timing, and transcript edits into Classic", () => {
		const document = daemonRoundTrip(
			toDomainProjectDocument({ project: representativeProject() }),
		);
		document.name = "Agent short cut";
		const videoItem = document.scenes[0].tracks
			.find((track) => track.id === "track-main")
			?.items.find((item) => item.id === "item-video");
		if (!videoItem) throw new Error("fixture video item is missing");
		videoItem.name = "Agent selected take";
		videoItem.startTicks = 120_000;
		videoItem.durationTicks = 840_000;

		const transcript = document.transcripts[0];
		const world = transcript.words.find((word) => word.id === "word-two");
		if (!world) throw new Error("fixture transcript word is missing");
		world.displayText = "OpenChatCut";
		const captionItem = document.scenes[0].tracks
			.find((track) => track.id === "track-caption")
			?.items.find((item) => item.id === "item-caption");
		if (!captionItem || captionItem.content.type !== "caption") {
			throw new Error("fixture caption item is missing");
		}
		captionItem.content.caption.wordIds = ["word-two"];

		const restored = fromDomainProjectEnvelope({
			envelope: {
				document,
				revision: 7,
				documentHash: "agent-revision-hash",
			},
		});
		expect(restored.metadata.name).toBe("Agent short cut");
		const restoredVideo = restored.scenes[0].tracks.main.elements[0];
		expect(restoredVideo.name).toBe("Agent selected take");
		expect(restoredVideo.startTime).toBe(ticks(120_000));
		expect(restoredVideo.duration).toBe(ticks(840_000));

		const restoredCaption = restored.scenes[0].tracks.overlay[0].elements[0];
		if (
			restoredCaption.type !== "text" ||
			restoredCaption.semanticType !== "caption"
		) {
			throw new Error("materialized element is not a caption");
		}
		expect(restoredCaption.cues).toHaveLength(1);
		expect(restoredCaption.cues[0].words).toHaveLength(1);
		expect(restoredCaption.cues[0].words[0].displayText).toBe("OpenChatCut");
		expect(restoredCaption.cues[0].words[0].transcriptWordId).toBe("word-two");
	});

	test("uses one audible video asset for linked picture and sound elements", () => {
		const project = representativeProject();
		project.scenes[0].tracks.audio[0].elements.push({
			id: "item-camera-audio",
			name: "Camera A audio",
			type: "audio",
			sourceType: "upload",
			mediaId: "asset-video",
			startTime: ticks(0),
			duration: ticks(1_200_000),
			trimStart: ticks(120_000),
			trimEnd: ticks(80_000),
			sourceDuration: ticks(1_400_000),
			params: { volume: 1 },
		});

		const document = toDomainProjectDocument({ project });
		const videoAsset = document.assets.find(
			(asset) => asset.id === "asset-video",
		);
		expect(videoAsset?.kind).toBe("video");
		expect(videoAsset?.hasAudio).toBe(true);
		expect(
			document.assets.filter((asset) => asset.id === "asset-video"),
		).toHaveLength(1);

		const audioItem = document.scenes[0].tracks
			.find((track) => track.id === "track-audio")
			?.items.find((item) => item.id === "item-camera-audio");
		expect(audioItem?.content).toEqual({
			type: "media",
			assetId: "asset-video",
			mediaKind: "audio",
		});

		const restored = fromDomainProjectEnvelope({
			envelope: {
				document: daemonRoundTrip(document),
				revision: 1,
				documentHash: "linked-av-hash",
			},
		});
		const restoredAudio = restored.scenes[0].tracks.audio[0].elements.find(
			(element) => element.id === "item-camera-audio",
		);
		expect(restoredAudio?.type).toBe("audio");
		if (
			!restoredAudio ||
			restoredAudio.type !== "audio" ||
			restoredAudio.sourceType !== "upload"
		) {
			throw new Error("linked video audio was not restored as audio");
		}
		expect(restoredAudio.mediaId).toBe("asset-video");
	});

	test("preserves an editable motion graphic DSL through the Classic shell", () => {
		const document = daemonRoundTrip(
			toDomainProjectDocument({ project: representativeProject() }),
		);
		const definition = {
			version: 1 as const,
			width: 1920,
			height: 1080,
			durationSeconds: 2,
			designStyle: "editorial-dark",
			nodes: [
				{
					id: "title",
					type: "text",
					text: "Editable MG",
					x: 960,
					y: 540,
					fontSize: 96,
					color: "#ffffff",
				},
			],
		};
		document.scenes[0].tracks.unshift({
			id: "track-mg",
			name: "Motion Graphics",
			kind: "graphic",
			muted: false,
			hidden: false,
			locked: false,
			items: [
				{
					id: "item-mg",
					name: "Title card",
					startTicks: 0,
					durationTicks: 240_000,
					enabled: true,
					content: {
						type: "motionGraphic",
						motionGraphic: {
							dslVersion: 1,
							definition,
							templateId: "editorial-dark",
						},
					},
				},
			],
		});

		const restored = fromDomainProjectEnvelope({
			envelope: {
				document,
				revision: 4,
				documentHash: "mg-hash",
			},
		});
		const graphicTrack = restored.scenes[0].tracks.overlay.find(
			(track) => track.id === "track-mg",
		);
		const graphic = graphicTrack?.elements[0];
		if (!graphic || graphic.type !== "graphic") {
			throw new Error("materialized element is not a graphic");
		}
		expect(graphic.motionGraphic?.definition).toEqual(definition);

		const roundTripped = toDomainProjectDocument({ project: restored });
		const content = roundTripped.scenes[0].tracks.find(
			(track) => track.id === "track-mg",
		)?.items[0].content;
		if (!content || content.type !== "motionGraphic") {
			throw new Error("round-tripped element is not a motion graphic");
		}
		expect(content.motionGraphic.definition).toEqual(definition);
	});
});
