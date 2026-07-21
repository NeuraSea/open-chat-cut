import { mock } from "bun:test";

// Bun does not provide a DOM canvas in its test runtime. A small deterministic
// measurement context keeps text-mask geometry tests meaningful without
// pulling a native canvas implementation into the web bundle. The production
// browser path still uses the real OffscreenCanvas/CanvasRenderingContext2D.
if (typeof globalThis.OffscreenCanvas === "undefined") {
	class BunTextMeasurementContext {
		font = "normal normal 10px sans-serif";
		textBaseline = "alphabetic";
		letterSpacing = "0px";

		save(): void {}
		restore(): void {}

		measureText(text: string): TextMetrics {
			const fontSize =
				Number.parseFloat(/(\d+(?:\.\d+)?)px/.exec(this.font)?.[1] ?? "10") ||
				10;
			const letterSpacing =
				Number.parseFloat(
					/-?(?:\d+(?:\.\d+)?|\.\d+)px/.exec(this.letterSpacing)?.[0] ?? "0",
				) || 0;
			const width =
				text.length * fontSize * 0.6 +
				Math.max(0, text.length - 1) * letterSpacing;
			return {
				width,
				actualBoundingBoxAscent: fontSize * 0.8,
				actualBoundingBoxDescent: fontSize * 0.2,
			} as TextMetrics;
		}
	}

	class BunOffscreenCanvas {
		constructor(
			readonly width: number,
			readonly height: number,
		) {}

		getContext(contextId: string): BunTextMeasurementContext | null {
			return contextId === "2d" ? new BunTextMeasurementContext() : null;
		}
	}

	globalThis.OffscreenCanvas =
		BunOffscreenCanvas as unknown as typeof OffscreenCanvas;
}

// Bun's test runner currently exposes the wasm-bindgen binary import without
// the generated `__wbindgen_start` export used by the published opencut-wasm
// package. Keep unit tests deterministic and platform-neutral by replacing
// only the time helpers with the same integer-tick semantics as the Rust
// implementation. Browser/Next production builds continue to use the real
// WASM package.
const TICKS_PER_SECOND = 120_000;

function roundAwayFromZero(value: number): number {
	const magnitude = Math.round(Math.abs(value));
	return value < 0 ? -magnitude : magnitude;
}

function mediaTimeFromSeconds({ seconds }: { seconds: number }): number {
	return roundAwayFromZero(seconds * TICKS_PER_SECOND);
}

function mediaTimeToSeconds({ time }: { time: number }): number {
	return time / TICKS_PER_SECOND;
}

function frameRateValue(rate: {
	numerator: number;
	denominator: number;
}): number {
	return rate.numerator / rate.denominator;
}

function frameNumberFromTime({
	time,
	rate,
}: {
	time: number;
	rate: { numerator: number; denominator: number };
}): number {
	return (time / TICKS_PER_SECOND) * frameRateValue(rate);
}

function ticksFromFrame({
	frame,
	rate,
}: {
	frame: number;
	rate: { numerator: number; denominator: number };
}): number {
	return roundAwayFromZero(
		(frame * TICKS_PER_SECOND * rate.denominator) / rate.numerator,
	);
}

mock.module("opencut-wasm", () => ({
	TICKS_PER_SECOND: () => TICKS_PER_SECOND,
	mediaTimeFromSeconds,
	mediaTimeToSeconds,
	mediaTimeAdd: ({ a, b }: { a: number; b: number }) => a + b,
	mediaTimeSub: ({ a, b }: { a: number; b: number }) => a - b,
	mediaTimeMin: ({ a, b }: { a: number; b: number }) => Math.min(a, b),
	mediaTimeMax: ({ a, b }: { a: number; b: number }) => Math.max(a, b),
	mediaTimeClamp: ({
		time,
		min,
		max,
	}: {
		time: number;
		min: number;
		max: number;
	}) => Math.min(max, Math.max(min, time)),
	mediaTimeFromFrame: ({
		frame,
		rate,
	}: {
		frame: number;
		rate: { numerator: number; denominator: number };
	}) => ticksFromFrame({ frame, rate }),
	mediaTimeToFrame: ({
		time,
		rate,
	}: {
		time: number;
		rate: { numerator: number; denominator: number };
	}) =>
		roundAwayFromZero(
			(time * rate.numerator) / (TICKS_PER_SECOND * rate.denominator),
		),
	roundToFrame: ({
		time,
		rate,
	}: {
		time: number;
		rate: { numerator: number; denominator: number };
	}) => {
		const frame = roundAwayFromZero(
			(time * rate.numerator) / (TICKS_PER_SECOND * rate.denominator),
		);
		return roundAwayFromZero(
			(frame * TICKS_PER_SECOND * rate.denominator) / rate.numerator,
		);
	},
	floorToFrame: ({
		time,
		rate,
	}: {
		time: number;
		rate: { numerator: number; denominator: number };
	}) => {
		const frame = Math.floor(
			(time * rate.numerator) / (TICKS_PER_SECOND * rate.denominator),
		);
		return roundAwayFromZero(
			(frame * TICKS_PER_SECOND * rate.denominator) / rate.numerator,
		);
	},
	lastFrameTime: ({
		duration,
		rate,
	}: {
		duration: number;
		rate: { numerator: number; denominator: number };
	}) =>
		ticksFromFrame({
			frame: Math.max(
				0,
				Math.ceil(frameNumberFromTime({ time: duration, rate })) - 1,
			),
			rate,
		}),
	snappedSeekTime: ({
		time,
		duration,
		rate,
	}: {
		time: number;
		duration: number;
		rate: { numerator: number; denominator: number };
	}) => {
		const frame = roundAwayFromZero(
			(time * rate.numerator) / (TICKS_PER_SECOND * rate.denominator),
		);
		const lastFrame = Math.max(
			0,
			Math.ceil(
				(duration * rate.numerator) / (TICKS_PER_SECOND * rate.denominator),
			) - 1,
		);
		return roundAwayFromZero(
			(Math.min(lastFrame, Math.max(0, frame)) *
				TICKS_PER_SECOND *
				rate.denominator) /
				rate.numerator,
		);
	},
	parseTimecode: () => undefined,
	guessTimecodeFormat: () => "seconds",
	formatTimecode: ({ time }: { time: number }) =>
		mediaTimeToSeconds({ time }).toFixed(2),
}));
