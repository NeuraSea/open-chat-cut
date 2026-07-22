import type { MediaAsset } from "@/media/types";
import type {
	MotionGraphicDsl,
	MotionGraphicEasing,
	MotionGraphicKeyframe,
	MotionGraphicNode,
} from "./types";

export interface MotionGraphicMediaResolver {
	resolve(assetId: string, localTime: number): Promise<CanvasImageSource | null>;
}

type ResolvedNode = MotionGraphicNode & Record<string, unknown>;

function number(value: unknown, fallback: number): number {
	return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function string(value: unknown, fallback: string): string {
	return typeof value === "string" ? value : fallback;
}

function ease(value: number, easing: MotionGraphicEasing | undefined): number {
	const t = Math.max(0, Math.min(1, value));
	switch (easing) {
		case "ease-in":
			return t * t;
		case "ease-out":
			return 1 - (1 - t) * (1 - t);
		case "ease-in-out":
			return t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
		case "back-in": {
			const c = 1.70158;
			return (c + 1) * t ** 3 - c * t ** 2;
		}
		case "back-out": {
			const c = 1.70158;
			const x = t - 1;
			return 1 + (c + 1) * x ** 3 + c * x ** 2;
		}
		case "elastic-out": {
			if (t === 0 || t === 1) return t;
			return 2 ** (-10 * t) * Math.sin(((t * 10 - 0.75) * 2 * Math.PI) / 3) + 1;
		}
		case "bounce-out": {
			const n = 7.5625;
			const d = 2.75;
			if (t < 1 / d) return n * t * t;
			if (t < 2 / d) {
				const x = t - 1.5 / d;
				return n * x * x + 0.75;
			}
			if (t < 2.5 / d) {
				const x = t - 2.25 / d;
				return n * x * x + 0.9375;
			}
			const x = t - 2.625 / d;
			return n * x * x + 0.984375;
		}
		default:
			return t;
	}
}

function interpolateKeyframes(
	frames: MotionGraphicKeyframe[],
	time: number,
): unknown {
	if (frames.length === 0) return undefined;
	if (time <= frames[0].time) return frames[0].value;
	for (let index = 1; index < frames.length; index += 1) {
		const right = frames[index];
		if (time > right.time) continue;
		const left = frames[index - 1];
		if (
			typeof left.value !== "number" ||
			typeof right.value !== "number" ||
			right.time <= left.time
		) {
			return time < right.time ? left.value : right.value;
		}
		const progress = ease(
			(time - left.time) / (right.time - left.time),
			right.easing ?? left.easing,
		);
		return left.value + (right.value - left.value) * progress;
	}
	return frames[frames.length - 1].value;
}

function resolveNode(node: MotionGraphicNode, time: number): ResolvedNode {
	const resolved: ResolvedNode = { ...node };
	for (const [property, frames] of Object.entries(node.animations ?? {})) {
		if (!Array.isArray(frames)) continue;
		const value = interpolateKeyframes(frames, time);
		if (value !== undefined && !property.includes(".")) resolved[property] = value;
	}
	return resolved;
}

function roundedRect(
	context: OffscreenCanvasRenderingContext2D,
	x: number,
	y: number,
	width: number,
	height: number,
	radius: number,
): void {
	const r = Math.max(0, Math.min(Math.abs(width) / 2, Math.abs(height) / 2, radius));
	context.beginPath();
	context.moveTo(x + r, y);
	context.lineTo(x + width - r, y);
	context.quadraticCurveTo(x + width, y, x + width, y + r);
	context.lineTo(x + width, y + height - r);
	context.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
	context.lineTo(x + r, y + height);
	context.quadraticCurveTo(x, y + height, x, y + height - r);
	context.lineTo(x, y + r);
	context.quadraticCurveTo(x, y, x + r, y);
	context.closePath();
}

function fillAndStroke(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
): void {
	const fill = typeof node.fill === "string" ? node.fill : undefined;
	const stroke = typeof node.stroke === "string" ? node.stroke : undefined;
	if (fill && fill !== "none") {
		context.fillStyle = fill;
		context.fill();
	}
	if (stroke && stroke !== "none") {
		context.strokeStyle = stroke;
		context.lineWidth = number(node.strokeWidth, 1);
		context.stroke();
	}
}

function drawShape(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
): void {
	const width = number(node.width, 100);
	const height = number(node.height, 100);
	const x = -number(node.anchorX, 0.5) * width;
	const y = -number(node.anchorY, 0.5) * height;
	switch (string(node.shape, "rectangle")) {
		case "ellipse":
		case "circle":
			context.beginPath();
			context.ellipse(x + width / 2, y + height / 2, Math.abs(width / 2), Math.abs(height / 2), 0, 0, Math.PI * 2);
			break;
		case "line":
			context.beginPath();
			context.moveTo(x, y);
			context.lineTo(x + width, y + height);
			break;
		default:
			roundedRect(context, x, y, width, height, number(node.borderRadius, 0));
			break;
	}
	fillAndStroke(context, node);
}

function drawPath(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
): void {
	const data = typeof node.pathData === "string" ? node.pathData : "";
	if (!data || typeof Path2D === "undefined") return;
	const path = new Path2D(data);
	const width = number(node.width, 100);
	const height = number(node.height, 100);
	context.translate(
		-number(node.anchorX, 0.5) * width,
		-number(node.anchorY, 0.5) * height,
	);
	const fill = typeof node.fill === "string" ? node.fill : undefined;
	const stroke = typeof node.stroke === "string" ? node.stroke : undefined;
	if (fill && fill !== "none") {
		context.fillStyle = fill;
		context.fill(path, node.fillRule === "evenodd" ? "evenodd" : "nonzero");
	}
	if (stroke && stroke !== "none") {
		context.strokeStyle = stroke;
		context.lineWidth = number(node.strokeWidth, 1);
		context.stroke(path);
	}
}

function drawText(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
): void {
	const text = string(node.text, "");
	const size = number(node.fontSize, 64);
	const family = string(node.fontFamily, "Inter, sans-serif");
	const weight = number(node.fontWeight, 600);
	const style = string(node.fontStyle, "normal");
	context.font = `${style} ${weight} ${size}px ${family}`;
	context.textBaseline = "middle";
	const align = string(node.textAlign, "center");
	context.textAlign = align === "start" || align === "left" ? "left" : align === "end" || align === "right" ? "right" : "center";
	context.fillStyle = string(node.color, "#ffffff");
	const maxWidth = number(node.maxWidth, number(node.width, Number.POSITIVE_INFINITY));
	if (Number.isFinite(maxWidth)) context.fillText(text, 0, 0, maxWidth);
	else context.fillText(text, 0, 0);
}

function chartValues(node: ResolvedNode): number[] {
	if (!Array.isArray(node.data)) return [];
	return node.data
		.map((value) =>
			typeof value === "number"
				? value
				: value && typeof value === "object" && "value" in value
					? Number((value as { value: unknown }).value)
					: Number.NaN,
		)
		.filter(Number.isFinite);
}

function drawChart(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
): void {
	const values = chartValues(node);
	if (values.length === 0) return;
	const width = number(node.width, 600);
	const height = number(node.height, 360);
	const left = -number(node.anchorX, 0.5) * width;
	const top = -number(node.anchorY, 0.5) * height;
	const colors = Array.isArray(node.colors)
		? node.colors.filter((color): color is string => typeof color === "string")
		: ["#4f8cff", "#77d6aa", "#ffc857", "#f97b8b"];
	const min = number(node.min, Math.min(0, ...values));
	const max = number(node.max, Math.max(...values));
	const span = Math.max(Number.EPSILON, max - min);
	const kind = string(node.chartType, "bar");
	if (kind === "pie" || kind === "donut") {
		const total = values.reduce((sum, value) => sum + Math.max(0, value), 0);
		if (total <= 0) return;
		let angle = -Math.PI / 2;
		const radius = Math.min(width, height) / 2;
		for (let index = 0; index < values.length; index += 1) {
			const next = angle + (Math.max(0, values[index]) / total) * Math.PI * 2;
			context.beginPath();
			context.moveTo(0, 0);
			context.arc(0, 0, radius, angle, next);
			context.closePath();
			context.fillStyle = colors[index % colors.length] ?? "#4f8cff";
			context.fill();
			angle = next;
		}
		if (kind === "donut") {
			context.globalCompositeOperation = "destination-out";
			context.beginPath();
			context.arc(0, 0, radius * 0.55, 0, Math.PI * 2);
			context.fill();
			context.globalCompositeOperation = "source-over";
		}
		return;
	}
	if (kind === "line") {
		context.beginPath();
		values.forEach((value, index) => {
			const x = left + (index / Math.max(1, values.length - 1)) * width;
			const y = top + height - ((value - min) / span) * height;
			if (index === 0) context.moveTo(x, y);
			else context.lineTo(x, y);
		});
		context.strokeStyle = colors[0] ?? "#4f8cff";
		context.lineWidth = number(node.strokeWidth, 4);
		context.stroke();
		return;
	}
	const gap = Math.max(0, number(node.gap, width * 0.02));
	const barWidth = Math.max(1, (width - gap * (values.length - 1)) / values.length);
	values.forEach((value, index) => {
		const normalized = Math.max(0, (value - min) / span);
		const barHeight = normalized * height;
		context.fillStyle = colors[index % colors.length] ?? "#4f8cff";
		context.fillRect(left + index * (barWidth + gap), top + height - barHeight, barWidth, barHeight);
	});
}

async function drawMedia(
	context: OffscreenCanvasRenderingContext2D,
	node: ResolvedNode,
	time: number,
	media: MotionGraphicMediaResolver,
): Promise<void> {
	const assetId = typeof node.assetId === "string" ? node.assetId : "";
	const source = assetId ? await media.resolve(assetId, time) : null;
	if (!source) return;
	const sourceWidth = "videoWidth" in source
		? source.videoWidth
		: "naturalWidth" in source
			? source.naturalWidth
			: "width" in source
				? Number(source.width)
				: 0;
	const sourceHeight = "videoHeight" in source
		? source.videoHeight
		: "naturalHeight" in source
			? source.naturalHeight
			: "height" in source
				? Number(source.height)
				: 0;
	if (!sourceWidth || !sourceHeight) return;
	const width = number(node.width, sourceWidth);
	const height = number(node.height, sourceHeight);
	const x = -number(node.anchorX, 0.5) * width;
	const y = -number(node.anchorY, 0.5) * height;
	const fit = string(node.fit, "cover");
	if (fit === "fill") {
		context.drawImage(source, x, y, width, height);
		return;
	}
	const scale = fit === "contain"
		? Math.min(width / sourceWidth, height / sourceHeight)
		: Math.max(width / sourceWidth, height / sourceHeight);
	const drawWidth = sourceWidth * scale;
	const drawHeight = sourceHeight * scale;
	context.save();
	context.beginPath();
	context.rect(x, y, width, height);
	context.clip();
	context.drawImage(
		source,
		x + (width - drawWidth) / 2,
		y + (height - drawHeight) / 2,
		drawWidth,
		drawHeight,
	);
	context.restore();
}

async function renderNode(
	context: OffscreenCanvasRenderingContext2D,
	node: MotionGraphicNode,
	time: number,
	media: MotionGraphicMediaResolver,
	staggerOffset: number,
): Promise<void> {
	const localTime = Math.max(0, time - staggerOffset);
	const resolved = resolveNode(node, localTime);
	if (resolved.visible === false || number(resolved.opacity, 1) <= 0) return;
	context.save();
	context.globalAlpha *= Math.max(0, Math.min(1, number(resolved.opacity, 1)));
	if (typeof resolved.blendMode === "string") {
		context.globalCompositeOperation = resolved.blendMode as GlobalCompositeOperation;
	}
	context.translate(number(resolved.x, 0), number(resolved.y, 0));
	context.rotate((number(resolved.rotation, 0) * Math.PI) / 180);
	const scale = number(resolved.scale, 1);
	context.scale(
		scale * number(resolved.scaleX, 1),
		scale * number(resolved.scaleY, 1),
	);
	if (resolved.type === "group" && resolved.clip === true) {
		const width = number(resolved.width, 0);
		const height = number(resolved.height, 0);
		if (width > 0 && height > 0) {
			context.beginPath();
			context.rect(
				-number(resolved.anchorX, 0) * width,
				-number(resolved.anchorY, 0) * height,
				width,
				height,
			);
			context.clip();
		}
	}
	switch (resolved.type) {
		case "text":
			drawText(context, resolved);
			break;
		case "shape":
			drawShape(context, resolved);
			break;
		case "path":
		case "svg":
			drawPath(context, resolved);
			break;
		case "chart":
			drawChart(context, resolved);
			break;
		case "media":
			await drawMedia(context, resolved, localTime, media);
			break;
		case "group":
			break;
	}
	const stagger = number(resolved.stagger, 0);
	for (const [index, child] of (resolved.children ?? []).entries()) {
		await renderNode(context, child, localTime, media, index * stagger);
	}
	context.restore();
}

export async function renderMotionGraphicDsl({
	context,
	definition,
	localTime,
	media,
}: {
	context: OffscreenCanvasRenderingContext2D;
	definition: MotionGraphicDsl;
	localTime: number;
	media: MotionGraphicMediaResolver;
}): Promise<void> {
	context.clearRect(0, 0, definition.width, definition.height);
	if (definition.background && definition.background !== "transparent") {
		context.fillStyle = definition.background;
		context.fillRect(0, 0, definition.width, definition.height);
	}
	for (const node of definition.nodes) {
		await renderNode(context, node, localTime, media, 0);
	}
}

export function createMotionGraphicMediaResolver(
	mediaMap: Map<string, MediaAsset>,
): MotionGraphicMediaResolver {
	const images = new Map<string, Promise<ImageBitmap | null>>();
	const videos = new Map<string, HTMLVideoElement>();
	return {
		async resolve(assetId, localTime) {
			const asset = mediaMap.get(assetId);
			if (!asset) return null;
			if (asset.type === "image") {
				let promise = images.get(assetId);
				if (!promise) {
					promise = createImageBitmap(asset.file).catch(() => null);
					images.set(assetId, promise);
				}
				return promise;
			}
			if (asset.type !== "video" || !asset.url) return null;
			let video = videos.get(assetId);
			if (!video) {
				video = document.createElement("video");
				video.src = asset.url;
				video.muted = true;
				video.preload = "auto";
				video.playsInline = true;
				videos.set(assetId, video);
			}
			if (video.readyState < HTMLMediaElement.HAVE_METADATA) {
				await new Promise<void>((resolve, reject) => {
					video?.addEventListener("loadedmetadata", () => resolve(), { once: true });
					video?.addEventListener("error", () => reject(new Error("MG video failed to load")), { once: true });
				}).catch(() => undefined);
			}
			if (!Number.isFinite(video.duration) || video.duration <= 0) return video;
			const target = Math.min(Math.max(0, localTime), Math.max(0, video.duration - 0.001));
			if (Math.abs(video.currentTime - target) > 0.001) {
				await new Promise<void>((resolve) => {
					const timeout = window.setTimeout(resolve, 2_000);
					video?.addEventListener("seeked", () => {
						window.clearTimeout(timeout);
						resolve();
					}, { once: true });
					if (video) video.currentTime = target;
				});
			}
			return video;
		},
	};
}
