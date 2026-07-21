import type { MotionGraphicMediaResolver } from "./render-dsl";
import type {
	MotionGraphicJsxExpression,
	MotionGraphicJsxIr,
	MotionGraphicJsxNode,
} from "./types";

const MAX_OPERATIONS = 50_000;
const MAX_DEPTH = 100;
const FORBIDDEN_PROPERTIES = new Set([
	"__proto__",
	"prototype",
	"constructor",
]);
const SAFE_TAGS = new Set([
	"AbsoluteFill",
	"Sequence",
	"Img",
	"Video",
	"Audio",
	"div",
	"span",
	"p",
	"strong",
	"em",
	"img",
	"video",
	"audio",
	"svg",
	"g",
	"path",
	"rect",
	"circle",
	"ellipse",
	"line",
	"polyline",
	"polygon",
	"text",
	"tspan",
	"defs",
	"linearGradient",
	"radialGradient",
	"stop",
	"clipPath",
	"mask",
]);

type SafeRecord = Record<string, unknown>;

export type EvaluatedMotionGraphicNode =
	| string
	| number
	| {
			tag: string;
			props: SafeRecord;
			children: EvaluatedMotionGraphicNode[];
	  };

interface EvaluationState {
	operations: number;
	frame: number;
	config: Readonly<{
		width: number;
		height: number;
		fps: number;
		durationInFrames: number;
	}>;
}

interface Bounds {
	x: number;
	y: number;
	width: number;
	height: number;
}

function operation(state: EvaluationState, depth: number): void {
	state.operations += 1;
	if (state.operations > MAX_OPERATIONS) {
		throw new Error("Advanced motion graphic exceeded its operation limit");
	}
	if (depth > MAX_DEPTH) {
		throw new Error("Advanced motion graphic exceeded its nesting limit");
	}
}

function safeObject(value: unknown): SafeRecord | null {
	if (!value || typeof value !== "object" || Array.isArray(value)) return null;
	return value as SafeRecord;
}

function finite(value: unknown, fallback: number): number {
	return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum: number, maximum: number): number {
	return Math.min(maximum, Math.max(minimum, value));
}

function interpolate(
	value: unknown,
	input: unknown,
	output: unknown,
	options?: unknown,
): number {
	if (
		typeof value !== "number" ||
		!Number.isFinite(value) ||
		!Array.isArray(input) ||
		!Array.isArray(output) ||
		input.length < 2 ||
		input.length !== output.length ||
		!input.every((item) => typeof item === "number" && Number.isFinite(item)) ||
		!output.every((item) => typeof item === "number" && Number.isFinite(item))
	) {
		throw new Error("interpolate requires equally-sized finite numeric ranges");
	}
	const settings = safeObject(options);
	let sample = value;
	if (settings?.extrapolateLeft === "clamp") sample = Math.max(sample, input[0]);
	if (settings?.extrapolateRight === "clamp") {
		sample = Math.min(sample, input[input.length - 1]);
	}
	let index = input.length - 2;
	for (let candidate = 0; candidate < input.length - 1; candidate += 1) {
		if (sample <= input[candidate + 1]) {
			index = candidate;
			break;
		}
	}
	const left = input[index];
	const right = input[index + 1];
	const progress = right === left ? 1 : (sample - left) / (right - left);
	return output[index] + (output[index + 1] - output[index]) * progress;
}

function spring(value: unknown, state: EvaluationState): number {
	const options = safeObject(value) ?? Object.create(null);
	const frame = finite(options.frame, state.frame);
	const fps = Math.max(1, finite(options.fps, state.config.fps));
	const config = safeObject(options.config) ?? Object.create(null);
	const damping = clamp(finite(config.damping, 10), 0.1, 100);
	const stiffness = clamp(finite(config.stiffness, 100), 0.1, 1_000);
	const mass = clamp(finite(config.mass, 1), 0.01, 100);
	const time = Math.max(0, frame) / fps;
	const decay = Math.exp((-damping * time) / (2 * mass));
	const angular = Math.sqrt(stiffness / mass);
	return 1 - decay * Math.cos(angular * time);
}

const SAFE_MATH: Readonly<SafeRecord> = Object.freeze({
	abs: Math.abs,
	ceil: Math.ceil,
	cos: Math.cos,
	floor: Math.floor,
	max: Math.max,
	min: Math.min,
	pow: Math.pow,
	round: Math.round,
	sin: Math.sin,
	sqrt: Math.sqrt,
	tan: Math.tan,
});

function callSafeRuntime(
	name: string,
	args: unknown[],
	state: EvaluationState,
): unknown {
	switch (name) {
		case "useCurrentFrame":
			return state.frame;
		case "useVideoConfig":
			return state.config;
		case "interpolate":
			return interpolate(args[0], args[1], args[2], args[3]);
		case "spring":
			return spring(args[0], state);
		case "sequence":
			return clamp(
				finite(args[0], state.frame) - finite(args[1], 0),
				0,
				Math.max(0, finite(args[2], state.config.durationInFrames)),
			);
		case "clamp":
			return clamp(
				finite(args[0], 0),
				finite(args[1], 0),
				finite(args[2], 1),
			);
		default: {
			if (!name.startsWith("Math.")) {
				throw new Error(`Safe runtime call is not available: ${name}`);
			}
			const method = name.slice(5);
			const callable = SAFE_MATH[method];
			if (typeof callable !== "function") {
				throw new Error(`Safe Math call is not available: ${name}`);
			}
			const numbers = args.map((argument) => {
				if (typeof argument !== "number" || !Number.isFinite(argument)) {
					throw new Error(`${name} only accepts finite numbers`);
				}
				return argument;
			});
			return callable(...numbers);
		}
	}
}

function evaluateExpression(
	expression: MotionGraphicJsxExpression,
	environment: Map<string, unknown>,
	state: EvaluationState,
	depth: number,
): unknown {
	operation(state, depth);
	if (!expression || typeof expression !== "object") {
		throw new Error("Advanced motion graphic contains an invalid expression");
	}
	switch (expression.kind) {
		case "literal":
			if (
				expression.value !== null &&
				!["string", "number", "boolean"].includes(typeof expression.value)
			) {
				throw new Error("Safe IR literal has an unsupported value");
			}
			return expression.value;
		case "identifier":
			if (expression.name === "Math") return SAFE_MATH;
			if (!environment.has(expression.name)) {
				throw new Error(`Safe IR binding is unavailable: ${expression.name}`);
			}
			return environment.get(expression.name);
		case "array":
			if (!Array.isArray(expression.items)) throw new Error("Invalid safe array");
			return expression.items.map((item) =>
				evaluateExpression(item, environment, state, depth + 1),
			);
		case "object": {
			if (!Array.isArray(expression.entries)) throw new Error("Invalid safe object");
			const result: SafeRecord = Object.create(null);
			for (const entry of expression.entries) {
				if (
					!Array.isArray(entry) ||
					entry.length !== 2 ||
					typeof entry[0] !== "string" ||
					FORBIDDEN_PROPERTIES.has(entry[0])
				) {
					throw new Error("Safe IR object contains a forbidden property");
				}
				result[entry[0]] = evaluateExpression(
					entry[1],
					environment,
					state,
					depth + 1,
				);
			}
			return result;
		}
		case "unary": {
			const value = evaluateExpression(
				expression.argument,
				environment,
				state,
				depth + 1,
			);
			if (expression.operator === "!") return !value;
			if (typeof value !== "number" || !Number.isFinite(value)) {
				throw new Error("Numeric unary expression requires a finite number");
			}
			return expression.operator === "-" ? -value : value;
		}
		case "binary": {
			const left = evaluateExpression(expression.left, environment, state, depth + 1);
			const right = evaluateExpression(
				expression.right,
				environment,
				state,
				depth + 1,
			);
			switch (expression.operator) {
				case "+":
					if (typeof left === "string" || typeof right === "string") {
						return `${String(left)}${String(right)}`;
					}
					return finite(left, 0) + finite(right, 0);
				case "-":
					return finite(left, 0) - finite(right, 0);
				case "*":
					return finite(left, 0) * finite(right, 0);
				case "/":
					return finite(left, 0) / finite(right, 0);
				case "%":
					return finite(left, 0) % finite(right, 0);
				case "**":
					return finite(left, 0) ** finite(right, 0);
				case "===":
					return left === right;
				case "!==":
					return left !== right;
				case "<":
					return finite(left, 0) < finite(right, 0);
				case "<=":
					return finite(left, 0) <= finite(right, 0);
				case ">":
					return finite(left, 0) > finite(right, 0);
				case ">=":
					return finite(left, 0) >= finite(right, 0);
				default:
					throw new Error(`Safe binary operator is unavailable: ${expression.operator}`);
			}
		}
		case "logical": {
			const left = evaluateExpression(expression.left, environment, state, depth + 1);
			if (expression.operator === "&&") {
				return left
					? evaluateExpression(expression.right, environment, state, depth + 1)
					: left;
			}
			if (expression.operator === "||") {
				return left
					? left
					: evaluateExpression(expression.right, environment, state, depth + 1);
			}
			if (expression.operator === "??") {
				return left !== null && left !== undefined
					? left
					: evaluateExpression(expression.right, environment, state, depth + 1);
			}
			throw new Error(`Safe logical operator is unavailable: ${expression.operator}`);
		}
		case "conditional":
			return evaluateExpression(
				evaluateExpression(expression.test, environment, state, depth + 1)
					? expression.consequent
					: expression.alternate,
				environment,
				state,
				depth + 1,
			);
		case "member": {
			if (FORBIDDEN_PROPERTIES.has(expression.property)) {
				throw new Error("Safe member access contains a forbidden property");
			}
			const object = evaluateExpression(expression.object, environment, state, depth + 1);
			if (Array.isArray(object) && expression.property === "length") return object.length;
			const record = safeObject(object);
			if (!record || !Object.prototype.hasOwnProperty.call(record, expression.property)) {
				throw new Error(`Safe member is unavailable: ${expression.property}`);
			}
			return record[expression.property];
		}
		case "call":
			return callSafeRuntime(
				expression.callee,
				expression.arguments.map((argument) =>
					evaluateExpression(argument, environment, state, depth + 1),
				),
				state,
			);
		case "template": {
			if (
				!Array.isArray(expression.quasis) ||
				expression.quasis.length !== expression.expressions.length + 1
			) {
				throw new Error("Safe template literal is invalid");
			}
			return expression.expressions.reduce(
				(result, item, index) =>
					`${result}${String(evaluateExpression(item, environment, state, depth + 1))}${expression.quasis[index + 1]}`,
				expression.quasis[0] ?? "",
			);
		}
		case "element":
		case "fragment":
			return evaluateNode(expression, environment, state, depth + 1);
		default:
			throw new Error("Safe IR expression kind is unavailable");
	}
}

function flattenNodeValue(value: unknown): EvaluatedMotionGraphicNode[] {
	if (value === null || value === undefined || value === false || value === true) return [];
	if (Array.isArray(value)) return value.flatMap(flattenNodeValue);
	if (typeof value === "string" || typeof value === "number") return [value];
	const record = safeObject(value);
	if (record && typeof record.tag === "string" && Array.isArray(record.children)) {
		return [value as EvaluatedMotionGraphicNode];
	}
	throw new Error("Safe JSX expression did not produce a renderable value");
}

function evaluateNode(
	node: MotionGraphicJsxNode,
	environment: Map<string, unknown>,
	state: EvaluationState,
	depth: number,
): EvaluatedMotionGraphicNode | EvaluatedMotionGraphicNode[] {
	operation(state, depth);
	switch (node.kind) {
		case "text":
			return node.value;
		case "expression":
			return flattenNodeValue(
				evaluateExpression(node.expression, environment, state, depth + 1),
			);
		case "fragment":
			return node.children.flatMap((child) =>
				flattenNodeValue(evaluateNode(child, environment, state, depth + 1)),
			);
		case "element": {
			if (!SAFE_TAGS.has(node.tag)) throw new Error(`Safe JSX tag is unavailable: ${node.tag}`);
			const props: SafeRecord = Object.create(null);
			for (const attribute of node.attributes) {
				if (
					!Array.isArray(attribute) ||
					attribute.length !== 2 ||
					typeof attribute[0] !== "string" ||
					FORBIDDEN_PROPERTIES.has(attribute[0]) ||
					/^on/i.test(attribute[0])
				) {
					throw new Error("Safe JSX attribute is invalid");
				}
				props[attribute[0]] = evaluateExpression(
					attribute[1],
					environment,
					state,
					depth + 1,
				);
			}
			return {
				tag: node.tag,
				props,
				children: node.children.flatMap((child) =>
					flattenNodeValue(evaluateNode(child, environment, state, depth + 1)),
				),
			};
		}
		default:
			throw new Error("Safe JSX node kind is unavailable");
	}
}

export function evaluateMotionGraphicJsxIr(
	ir: MotionGraphicJsxIr,
	localTime: number,
): EvaluatedMotionGraphicNode[] {
	if (
		ir.version !== 1 ||
		ir.kind !== "jsxSafeIr" ||
		!Number.isFinite(ir.width) ||
		!Number.isFinite(ir.height) ||
		!Number.isFinite(ir.fps) ||
		ir.width < 1 ||
		ir.height < 1 ||
		ir.fps <= 0 ||
		!ir.program ||
		!Array.isArray(ir.program.bindings)
	) {
		throw new Error("Advanced motion graphic safe IR is invalid");
	}
	const state: EvaluationState = {
		operations: 0,
		frame: Math.max(0, Math.floor(localTime * ir.fps)),
		config: Object.freeze({
			width: ir.width,
			height: ir.height,
			fps: ir.fps,
			durationInFrames: Math.ceil(ir.durationSeconds * ir.fps),
		}),
	};
	const environment = new Map<string, unknown>();
	for (const binding of ir.program.bindings) {
		if (
			!binding ||
			typeof binding.name !== "string" ||
			!/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(binding.name) ||
			environment.has(binding.name)
		) {
			throw new Error("Advanced motion graphic contains an invalid binding");
		}
		environment.set(
			binding.name,
			evaluateExpression(binding.expression, environment, state, 1),
		);
	}
	return flattenNodeValue(evaluateNode(ir.program.root, environment, state, 1));
}

function styleFor(node: Exclude<EvaluatedMotionGraphicNode, string | number>): SafeRecord {
	return safeObject(node.props.style) ?? Object.create(null);
}

function styledBounds(style: SafeRecord, parent: Bounds): Bounds {
	const width = Math.max(0, finite(style.width, parent.width));
	const height = Math.max(0, finite(style.height, parent.height));
	return {
		x: parent.x + finite(style.left, 0),
		y: parent.y + finite(style.top, 0),
		width,
		height,
	};
}

function paintBackground(
	context: OffscreenCanvasRenderingContext2D,
	style: SafeRecord,
	bounds: Bounds,
): void {
	const background =
		typeof style.backgroundColor === "string"
			? style.backgroundColor
			: typeof style.background === "string" && !style.background.includes("url(")
				? style.background
				: null;
	if (!background) return;
	context.fillStyle = background;
	context.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
}

function applyTransform(
	context: OffscreenCanvasRenderingContext2D,
	style: SafeRecord,
	bounds: Bounds,
): void {
	if (typeof style.transform !== "string" || style.transform.length > 512) return;
	const centerX = bounds.x + bounds.width / 2;
	const centerY = bounds.y + bounds.height / 2;
	context.translate(centerX, centerY);
	for (const match of style.transform.matchAll(
		/(translateX|translateY|translate|scale|scaleX|scaleY|rotate)\(([-+0-9.eE]+)(?:px|deg)?(?:,\s*([-+0-9.eE]+)(?:px)?)?\)/g,
	)) {
		const first = Number(match[2]);
		const second = Number(match[3]);
		if (!Number.isFinite(first)) continue;
		switch (match[1]) {
			case "translateX":
				context.translate(first, 0);
				break;
			case "translateY":
				context.translate(0, first);
				break;
			case "translate":
				context.translate(first, Number.isFinite(second) ? second : 0);
				break;
			case "scale":
				context.scale(first, Number.isFinite(second) ? second : first);
				break;
			case "scaleX":
				context.scale(first, 1);
				break;
			case "scaleY":
				context.scale(1, first);
				break;
			case "rotate":
				context.rotate((first * Math.PI) / 180);
				break;
		}
	}
	context.translate(-centerX, -centerY);
}

function drawText(
	context: OffscreenCanvasRenderingContext2D,
	text: string,
	style: SafeRecord,
	bounds: Bounds,
): void {
	if (!text) return;
	const size = Math.max(1, finite(style.fontSize, 48));
	const weight =
		typeof style.fontWeight === "string" || typeof style.fontWeight === "number"
			? style.fontWeight
			: 400;
	const family = typeof style.fontFamily === "string" ? style.fontFamily : "Inter, sans-serif";
	const fontStyle = typeof style.fontStyle === "string" ? style.fontStyle : "normal";
	context.font = `${fontStyle} ${weight} ${size}px ${family}`;
	context.textBaseline = "middle";
	context.fillStyle = typeof style.color === "string" ? style.color : "#ffffff";
	const align = style.textAlign;
	context.textAlign = align === "left" || align === "start" ? "left" : align === "right" || align === "end" ? "right" : "center";
	const x = context.textAlign === "left" ? bounds.x : context.textAlign === "right" ? bounds.x + bounds.width : bounds.x + bounds.width / 2;
	context.fillText(text, x, bounds.y + bounds.height / 2, bounds.width);
}

function sourceDimensions(source: CanvasImageSource): { width: number; height: number } {
	const record = source as unknown as Record<string, unknown>;
	return {
		width: finite(record.videoWidth, finite(record.naturalWidth, finite(record.width, 0))),
		height: finite(record.videoHeight, finite(record.naturalHeight, finite(record.height, 0))),
	};
}

async function drawMedia(
	context: OffscreenCanvasRenderingContext2D,
	node: Exclude<EvaluatedMotionGraphicNode, string | number>,
	style: SafeRecord,
	bounds: Bounds,
	localTime: number,
	media: MotionGraphicMediaResolver,
): Promise<void> {
	const sourceId = typeof node.props.src === "string" ? node.props.src : "";
	if (!sourceId.startsWith("asset:")) return;
	const source = await media.resolve(sourceId, localTime);
	if (!source) return;
	const dimensions = sourceDimensions(source);
	if (dimensions.width <= 0 || dimensions.height <= 0) return;
	const fit = style.objectFit === "contain" ? "contain" : style.objectFit === "fill" ? "fill" : "cover";
	if (fit === "fill") {
		context.drawImage(source, bounds.x, bounds.y, bounds.width, bounds.height);
		return;
	}
	const scale = fit === "contain"
		? Math.min(bounds.width / dimensions.width, bounds.height / dimensions.height)
		: Math.max(bounds.width / dimensions.width, bounds.height / dimensions.height);
	const width = dimensions.width * scale;
	const height = dimensions.height * scale;
	context.save();
	context.beginPath();
	context.rect(bounds.x, bounds.y, bounds.width, bounds.height);
	context.clip();
	context.drawImage(
		source,
		bounds.x + (bounds.width - width) / 2,
		bounds.y + (bounds.height - height) / 2,
		width,
		height,
	);
	context.restore();
}

function svgNumber(props: SafeRecord, name: string, fallback: number): number {
	return finite(props[name], fallback);
}

function drawSvgPrimitive(
	context: OffscreenCanvasRenderingContext2D,
	node: Exclude<EvaluatedMotionGraphicNode, string | number>,
	style: SafeRecord,
	bounds: Bounds,
): boolean {
	const props = node.props;
	const fill = typeof props.fill === "string" ? props.fill : typeof style.fill === "string" ? style.fill : "#ffffff";
	const stroke = typeof props.stroke === "string" ? props.stroke : typeof style.stroke === "string" ? style.stroke : "transparent";
	context.fillStyle = fill;
	context.strokeStyle = stroke;
	context.lineWidth = svgNumber(props, "strokeWidth", finite(style.strokeWidth, 1));
	context.beginPath();
	switch (node.tag) {
		case "rect":
			context.rect(
				bounds.x + svgNumber(props, "x", 0),
				bounds.y + svgNumber(props, "y", 0),
				svgNumber(props, "width", bounds.width),
				svgNumber(props, "height", bounds.height),
			);
			break;
		case "circle":
			context.arc(
				bounds.x + svgNumber(props, "cx", bounds.width / 2),
				bounds.y + svgNumber(props, "cy", bounds.height / 2),
				Math.abs(svgNumber(props, "r", Math.min(bounds.width, bounds.height) / 2)),
				0,
				Math.PI * 2,
			);
			break;
		case "ellipse":
			context.ellipse(
				bounds.x + svgNumber(props, "cx", bounds.width / 2),
				bounds.y + svgNumber(props, "cy", bounds.height / 2),
				Math.abs(svgNumber(props, "rx", bounds.width / 2)),
				Math.abs(svgNumber(props, "ry", bounds.height / 2)),
				0,
				0,
				Math.PI * 2,
			);
			break;
		case "line":
			context.moveTo(bounds.x + svgNumber(props, "x1", 0), bounds.y + svgNumber(props, "y1", 0));
			context.lineTo(bounds.x + svgNumber(props, "x2", bounds.width), bounds.y + svgNumber(props, "y2", bounds.height));
			break;
		case "path": {
			if (typeof props.d !== "string" || typeof Path2D === "undefined") return true;
			const path = new Path2D(props.d);
			if (fill !== "none") context.fill(path);
			if (stroke !== "none" && stroke !== "transparent") context.stroke(path);
			return true;
		}
		default:
			return false;
	}
	if (fill !== "none" && node.tag !== "line") context.fill();
	if (stroke !== "none" && stroke !== "transparent") context.stroke();
	return true;
}

function childBounds(
	style: SafeRecord,
	bounds: Bounds,
	index: number,
	count: number,
): Bounds {
	const padding = Math.max(0, finite(style.padding, 0));
	const gap = Math.max(0, finite(style.gap, 0));
	const content: Bounds = {
		x: bounds.x + padding,
		y: bounds.y + padding,
		width: Math.max(0, bounds.width - padding * 2),
		height: Math.max(0, bounds.height - padding * 2),
	};
	if (style.display !== "flex" || count <= 1) return content;
	if (style.flexDirection === "row") {
		const width = Math.max(0, (content.width - gap * (count - 1)) / count);
		return { ...content, x: content.x + index * (width + gap), width };
	}
	const height = Math.max(0, (content.height - gap * (count - 1)) / count);
	return { ...content, y: content.y + index * (height + gap), height };
}

async function renderEvaluatedNode(
	context: OffscreenCanvasRenderingContext2D,
	node: EvaluatedMotionGraphicNode,
	parent: Bounds,
	localTime: number,
	frame: number,
	media: MotionGraphicMediaResolver,
	depth: number,
): Promise<void> {
	if (depth > MAX_DEPTH) throw new Error("Advanced motion graphic exceeded its render depth");
	if (typeof node === "string" || typeof node === "number") {
		drawText(context, String(node), Object.create(null), parent);
		return;
	}
	if (node.tag === "Sequence") {
		const from = Math.max(0, finite(node.props.from, 0));
		const duration = Math.max(0, finite(node.props.durationInFrames, Number.POSITIVE_INFINITY));
		if (frame < from || frame >= from + duration) return;
	}
	if (node.tag === "Audio" || node.tag === "audio") return;
	const style = styleFor(node);
	const bounds = node.tag === "AbsoluteFill" ? parent : styledBounds(style, parent);
	context.save();
	context.globalAlpha *= clamp(finite(style.opacity, 1), 0, 1);
	applyTransform(context, style, bounds);
	paintBackground(context, style, bounds);
	if (["Img", "Video", "img", "video"].includes(node.tag)) {
		await drawMedia(context, node, style, bounds, localTime, media);
	}
	const svgDrawn = drawSvgPrimitive(context, node, style, bounds);
	const text = node.children
		.filter((child): child is string | number => typeof child === "string" || typeof child === "number")
		.map(String)
		.join("");
	if (text && !svgDrawn) drawText(context, text, style, bounds);
	const childElements = node.children.filter(
		(child): child is Exclude<EvaluatedMotionGraphicNode, string | number> =>
			typeof child !== "string" && typeof child !== "number",
	);
	for (const [index, child] of childElements.entries()) {
		await renderEvaluatedNode(
			context,
			child,
			childBounds(style, bounds, index, childElements.length),
			localTime,
			frame,
			media,
			depth + 1,
		);
	}
	context.restore();
}

export async function renderMotionGraphicJsxIr({
	context,
	ir,
	localTime,
	media,
}: {
	context: OffscreenCanvasRenderingContext2D;
	ir: MotionGraphicJsxIr;
	localTime: number;
	media: MotionGraphicMediaResolver;
}): Promise<void> {
	context.clearRect(0, 0, ir.width, ir.height);
	const nodes = evaluateMotionGraphicJsxIr(ir, localTime);
	const bounds = { x: 0, y: 0, width: ir.width, height: ir.height };
	const frame = Math.max(0, Math.floor(localTime * ir.fps));
	for (const node of nodes) {
		await renderEvaluatedNode(context, node, bounds, localTime, frame, media, 0);
	}
}
