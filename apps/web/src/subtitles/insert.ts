import type { EditorCore } from "@/core";
import {
	AddTrackCommand,
	BatchCommand,
	InsertElementCommand,
} from "@/commands";
import { buildSemanticCaptionElement } from "./build-caption-element";
import type { SubtitleCue } from "./types";

export function insertCaptionChunksAsTextTrack({
	editor,
	captions,
	presetId,
}: {
	editor: EditorCore;
	captions: SubtitleCue[];
	presetId?: string;
}): string | null {
	if (captions.length === 0) {
		return null;
	}

	const addTrackCommand = new AddTrackCommand({ type: "text", index: 0 });
	const trackId = addTrackCommand.getTrackId();
	const canvasSize = editor.project.getActive().settings.canvasSize;
	const insertCommand = new InsertElementCommand({
		placement: { mode: "explicit", trackId },
		element: buildSemanticCaptionElement({ captions, canvasSize, presetId }),
	});
	editor.command.execute({
		command: new BatchCommand([addTrackCommand, insertCommand]),
	});

	return trackId;
}
