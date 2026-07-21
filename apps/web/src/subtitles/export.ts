import type { CaptionElement } from "@/timeline";
import { addMediaTime, mediaTimeToSeconds } from "@/wasm";
import { buildCaptionRenderState } from "./caption-element";

export type CaptionExportFormat = "srt" | "vtt" | "ass" | "txt";

function pad(value: number, width = 2): string {
	return Math.floor(value).toString().padStart(width, "0");
}
function formatTimestamp({ seconds, separator }: { seconds: number; separator: "." | "," }): string {
	const safe = Math.max(0, seconds);
	const hours = Math.floor(safe / 3600);
	const minutes = Math.floor((safe % 3600) / 60);
	const wholeSeconds = Math.floor(safe % 60);
	const milliseconds = Math.round((safe - Math.floor(safe)) * 1000) % 1000;
	return `${pad(hours)}:${pad(minutes)}:${pad(wholeSeconds)}${separator}${pad(milliseconds, 3)}`;
}

function cueText({ element, cueIndex }: { element: CaptionElement; cueIndex: number }): string {
	const cue = element.cues[cueIndex];
	return buildCaptionRenderState({
		cue,
		localTime: cue.startTime,
		maxCharactersPerLine: element.maxCharactersPerLine,
		maxLines: element.maxLines,
	}).content;
}

function absoluteSeconds({
	element,
	relative,
}: {
	element: CaptionElement;
	relative: CaptionElement["cues"][number]["startTime"];
}): number {
	return mediaTimeToSeconds({ time: addMediaTime({ a: element.startTime, b: relative }) });
}

export function exportCaptionElement({
	element,
	format,
}: {
	element: CaptionElement;
	format: CaptionExportFormat;
}): string {
	if (format === "txt") {
		return element.cues.map((_, index) => cueText({ element, cueIndex: index }).replaceAll("\n", " ")).join("\n\n");
	}
	if (format === "ass") {
		const header = `[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: OpenChatCut,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,0,2,80,80,50,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text`;
		const events = element.cues.map((cue, index) => {
			const start = formatTimestamp({ seconds: absoluteSeconds({ element, relative: cue.startTime }), separator: "." }).replace(/^0/, "").slice(0, -1);
			const end = formatTimestamp({ seconds: absoluteSeconds({ element, relative: cue.endTime }), separator: "." }).replace(/^0/, "").slice(0, -1);
			const text = cueText({ element, cueIndex: index }).replaceAll("\n", "\\N").replaceAll("{", "\\{");
			return `Dialogue: 0,${start},${end},OpenChatCut,${cue.speakerId ?? ""},0,0,0,,${text}`;
		});
		return `${header}\n${events.join("\n")}\n`;
	}
	const isVtt = format === "vtt";
	const cues = element.cues.map((cue, index) => {
		const start = formatTimestamp({ seconds: absoluteSeconds({ element, relative: cue.startTime }), separator: isVtt ? "." : "," });
		const end = formatTimestamp({ seconds: absoluteSeconds({ element, relative: cue.endTime }), separator: isVtt ? "." : "," });
		return `${isVtt ? "" : `${index + 1}\n`}${start} --> ${end}\n${cueText({ element, cueIndex: index })}`;
	});
	return `${isVtt ? "WEBVTT\n\n" : ""}${cues.join("\n\n")}\n`;
}
