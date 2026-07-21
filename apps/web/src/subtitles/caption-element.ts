import type {
	CaptionCue,
	CaptionElement,
	CaptionWord,
	TimelineElement,
} from "@/timeline";
import type { MediaTime } from "@/wasm";

const NO_SPACE_BEFORE = /^[,.;:!?%\)\]\}，。！？、；：）》】」』]/u;
const NO_SPACE_AFTER = /[\(\[\{（《【「『]$/u;
const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

export interface CaptionRenderState {
	content: string;
	activeWordId: string | null;
	activeWordText: string | null;
	activeLineIndex: number;
	activePrefix: string;
}
export function isCaptionElement(
	element: TimelineElement,
): element is CaptionElement {
	return element.type === "text" && element.semanticType === "caption";
}

export function findActiveCaptionCue({
	element,
	localTime,
}: {
	element: CaptionElement;
	localTime: MediaTime;
}): CaptionCue | null {
	return (
		element.cues.find(
			(cue) => localTime >= cue.startTime && localTime < cue.endTime,
		) ?? null
	);
}

function separatorBetween({
	previous,
	current,
}: {
	previous: string;
	current: string;
}): string {
	if (!previous || !current) return "";
	if (NO_SPACE_BEFORE.test(current) || NO_SPACE_AFTER.test(previous)) return "";
	if (CJK.test(previous.at(-1) ?? "") && CJK.test(current.at(0) ?? "")) return "";
	return " ";
}

function displayText({ word }: { word: CaptionWord }): string {
	return word.displayText.trim();
}

export function buildCaptionRenderState({
	cue,
	localTime,
	maxCharactersPerLine,
	maxLines,
}: {
	cue: CaptionCue;
	localTime: MediaTime;
	maxCharactersPerLine: number;
	maxLines: number;
}): CaptionRenderState {
	const lines: Array<{ text: string; wordIds: string[]; prefixes: string[] }> = [
		{ text: "", wordIds: [], prefixes: [] },
	];
	let activeWord =
		cue.words.find(
			(word) => localTime >= word.startTime && localTime < word.endTime,
		) ?? null;

	for (const word of cue.words) {
		const text = displayText({ word });
		if (!text) continue;
		let line = lines[lines.length - 1];
		const separator = separatorBetween({
			previous: line.text,
			current: text,
		});
		const candidate = `${line.text}${separator}${text}`;
		if (
			line.text &&
			Array.from(candidate).length > maxCharactersPerLine &&
			lines.length < maxLines
		) {
			line = { text: "", wordIds: [], prefixes: [] };
			lines.push(line);
		}
		const nextSeparator = separatorBetween({
			previous: line.text,
			current: text,
		});
		line.prefixes.push(`${line.text}${nextSeparator}`);
		line.wordIds.push(word.id);
		line.text = `${line.text}${nextSeparator}${text}`;
	}

	if (cue.translation) {
		if (lines.length < maxLines) {
			lines.push({ text: cue.translation, wordIds: [], prefixes: [] });
		} else {
			lines[lines.length - 1].text += `\n${cue.translation}`;
		}
	}

	let activeLineIndex = -1;
	let activePrefix = "";
	if (activeWord) {
		for (let index = 0; index < lines.length; index++) {
			const wordIndex = lines[index].wordIds.indexOf(activeWord.id);
			if (wordIndex >= 0) {
				activeLineIndex = index;
				activePrefix = lines[index].prefixes[wordIndex];
				break;
			}
		}
		if (activeLineIndex < 0) activeWord = null;
	}

	return {
		content: lines.map((line) => line.text).join("\n"),
		activeWordId: activeWord?.id ?? null,
		activeWordText: activeWord ? displayText({ word: activeWord }) : null,
		activeLineIndex,
		activePrefix,
	};
}
