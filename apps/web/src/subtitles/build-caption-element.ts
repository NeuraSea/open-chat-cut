import type { CreateCaptionElement, CaptionWord } from "@/timeline";
import { generateUUID } from "@/utils/id";
import {
	mediaTimeFromSeconds,
	roundMediaTime,
	type MediaTime,
	ZERO_MEDIA_TIME,
} from "@/wasm";
import { buildSubtitleTextElement } from "./build-subtitle-text-element";
import { DEFAULT_CAPTION_PRESET, getCaptionPreset } from "./caption-presets";
import type { SubtitleCue } from "./types";

const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

function segmentText({ text }: { text: string }): string[] {
	const normalized = text.trim();
	if (!normalized) return [];
	if (typeof Intl.Segmenter === "function") {
		const segmenter = new Intl.Segmenter(undefined, { granularity: "word" });
		const segments = [...segmenter.segment(normalized)]
			.map((segment) => segment.segment.trim())
			.filter(Boolean);
		if (segments.length > 0) return segments;
	}
	if (CJK.test(normalized)) return Array.from(normalized).filter((value) => value.trim());
	return normalized.split(/\s+/u);
}

function buildWords({
	text,
	startTime,
	endTime,
}: {
	text: string;
	startTime: MediaTime;
	endTime: MediaTime;
}): CaptionWord[] {
	const tokens = segmentText({ text });
	const weights = tokens.map((token) => Math.max(1, Array.from(token).length));
	const totalWeight = weights.reduce((total, value) => total + value, 0);
	const duration = Math.max(1, endTime - startTime);
	let cursor = startTime;

	return tokens.map((token, index) => {
		const isLast = index === tokens.length - 1;
		const tokenDuration = isLast
			? endTime - cursor
			: Math.max(1, Math.round((duration * weights[index]) / totalWeight));
		const wordEnd = roundMediaTime({
			time: isLast ? endTime : Math.min(endTime, cursor + tokenDuration),
		});
		const word: CaptionWord = {
			id: generateUUID(),
			spokenText: token,
			displayText: token,
			startTime: cursor,
			endTime: roundMediaTime({ time: Math.max(cursor + 1, wordEnd) }),
		};
		cursor = word.endTime;
		return word;
	});
}

export function buildSemanticCaptionElement({
	captions,
	canvasSize,
	transcriptId = `imported:${generateUUID()}`,
	presetId = DEFAULT_CAPTION_PRESET.id,
}: {
	captions: SubtitleCue[];
	canvasSize: { width: number; height: number };
	transcriptId?: string;
	presetId?: string;
}): CreateCaptionElement {
	if (captions.length === 0) {
		throw new Error("At least one caption cue is required");
	}

	const preset = getCaptionPreset({ id: presetId });
	const firstTextElement = buildSubtitleTextElement({
		index: 0,
		caption: captions[0],
		canvasSize,
	});
	const absoluteStart = Math.min(...captions.map((caption) => caption.startTime));
	const absoluteEnd = Math.max(
		...captions.map((caption) => caption.startTime + caption.duration),
	);
	const elementStart = mediaTimeFromSeconds({ seconds: absoluteStart });

	return {
		type: "text",
		semanticType: "caption",
		name: "Captions",
		transcriptId,
		stylePresetId: preset.id,
		maxLines: preset.maxLines,
		maxCharactersPerLine: preset.maxCharactersPerLine,
		wordHighlight: preset.wordHighlight,
		highlightColor: preset.highlightColor,
		startTime: elementStart,
		duration: mediaTimeFromSeconds({ seconds: absoluteEnd - absoluteStart }),
		trimStart: ZERO_MEDIA_TIME,
		trimEnd: ZERO_MEDIA_TIME,
		params: {
			...(captions[0].style
				? { ...preset.params, ...firstTextElement.params }
				: { ...firstTextElement.params, ...preset.params }),
			content: captions[0].text,
		},
		cues: captions.map((caption) => {
			const cueStart = mediaTimeFromSeconds({
				seconds: caption.startTime - absoluteStart,
			});
			const cueEnd = mediaTimeFromSeconds({
				seconds: caption.startTime + caption.duration - absoluteStart,
			});
			return {
				id: generateUUID(),
				startTime: cueStart,
				endTime: cueEnd,
				words: buildWords({
					text: caption.text,
					startTime: cueStart,
					endTime: cueEnd,
				}),
			};
		}),
	};
}
