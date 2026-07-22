import { mediaTime, mediaTimeToSeconds } from "@/wasm";

/** Convert the editor's 120 kHz timeline ticks into MG keyframe seconds. */
export function motionGraphicTimeSeconds(localTimeTicks: number): number {
	return mediaTimeToSeconds({
		time: mediaTime({ ticks: Math.max(0, localTimeTicks) }),
	});
}
