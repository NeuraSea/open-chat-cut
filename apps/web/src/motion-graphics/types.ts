export type MotionGraphicNodeType =
	| "text"
	| "shape"
	| "svg"
	| "path"
	| "chart"
	| "media"
	| "group";

export type MotionGraphicEasing =
	| "linear"
	| "ease-in"
	| "ease-out"
	| "ease-in-out"
	| "back-in"
	| "back-out"
	| "elastic-out"
	| "bounce-out";

export interface MotionGraphicKeyframe {
	time: number;
	value: unknown;
	easing?: MotionGraphicEasing;
}

export interface MotionGraphicNode {
	id: string;
	type: MotionGraphicNodeType;
	name?: string;
	x?: number;
	y?: number;
	width?: number;
	height?: number;
	opacity?: number;
	rotation?: number;
	scale?: number;
	scaleX?: number;
	scaleY?: number;
	anchorX?: number;
	anchorY?: number;
	visible?: boolean;
	blendMode?: GlobalCompositeOperation;
	stagger?: number;
	animations?: Record<string, MotionGraphicKeyframe[]>;
	children?: MotionGraphicNode[];
	[key: string]: unknown;
}

export interface MotionGraphicDsl {
	version: 1;
	width: number;
	height: number;
	durationSeconds: number;
	nodes: MotionGraphicNode[];
	designStyle?: string;
	background?: string;
}

export interface MotionGraphicDefinition {
	dslVersion: number;
	definition: unknown;
	templateId?: string;
}

export type MotionGraphicJsxExpression =
	| { kind: "literal"; value: unknown }
	| { kind: "identifier"; name: string }
	| { kind: "array"; items: MotionGraphicJsxExpression[] }
	| { kind: "object"; entries: [string, MotionGraphicJsxExpression][] }
	| {
			kind: "unary";
			operator: "+" | "-" | "!";
			argument: MotionGraphicJsxExpression;
	  }
	| {
			kind: "binary" | "logical";
			operator: string;
			left: MotionGraphicJsxExpression;
			right: MotionGraphicJsxExpression;
	  }
	| {
			kind: "conditional";
			test: MotionGraphicJsxExpression;
			consequent: MotionGraphicJsxExpression;
			alternate: MotionGraphicJsxExpression;
	  }
	| {
			kind: "member";
			object: MotionGraphicJsxExpression;
			property: string;
	  }
	| {
			kind: "call";
			callee: string;
			arguments: MotionGraphicJsxExpression[];
	  }
	| {
			kind: "template";
			quasis: string[];
			expressions: MotionGraphicJsxExpression[];
	  }
	| MotionGraphicJsxNode;

export type MotionGraphicJsxNode =
	| { kind: "text"; value: string }
	| { kind: "expression"; expression: MotionGraphicJsxExpression }
	| { kind: "fragment"; children: MotionGraphicJsxNode[] }
	| {
			kind: "element";
			tag: string;
			attributes: [string, MotionGraphicJsxExpression][];
			children: MotionGraphicJsxNode[];
	  };

export interface MotionGraphicJsxIr {
	version: 1;
	kind: "jsxSafeIr";
	width: number;
	height: number;
	durationSeconds: number;
	fps: number;
	program: {
		bindings: { name: string; expression: MotionGraphicJsxExpression }[];
		root: MotionGraphicJsxNode;
	};
}

export interface MotionGraphicJsxDefinition {
	version: 1;
	mode: "jsx";
	source: string;
	ir: MotionGraphicJsxIr;
	validation: unknown;
	security: {
		sourceExecuted: false;
		interpreter: "deterministic-allowlisted-ir-v1";
		networkAccess: "disabled";
		fileAccess: "disabled";
		sandboxOrigin: "opaque";
	};
}

export function isMotionGraphicDsl(value: unknown): value is MotionGraphicDsl {
	if (!value || typeof value !== "object" || Array.isArray(value)) return false;
	const candidate = value as Record<string, unknown>;
	return (
		candidate.version === 1 &&
		typeof candidate.width === "number" &&
		Number.isFinite(candidate.width) &&
		typeof candidate.height === "number" &&
		Number.isFinite(candidate.height) &&
		typeof candidate.durationSeconds === "number" &&
		Number.isFinite(candidate.durationSeconds) &&
		Array.isArray(candidate.nodes)
	);
}

export function isMotionGraphicJsxDefinition(
	value: unknown,
): value is MotionGraphicJsxDefinition {
	if (!value || typeof value !== "object" || Array.isArray(value)) return false;
	const candidate = value as Record<string, unknown>;
	if (
		candidate.version !== 1 ||
		candidate.mode !== "jsx" ||
		typeof candidate.source !== "string" ||
		candidate.source.length === 0 ||
		candidate.source.length > 256 * 1024 ||
		!candidate.ir ||
		typeof candidate.ir !== "object" ||
		Array.isArray(candidate.ir) ||
		!candidate.security ||
		typeof candidate.security !== "object" ||
		Array.isArray(candidate.security)
	) {
		return false;
	}
	const ir = candidate.ir as Record<string, unknown>;
	const security = candidate.security as Record<string, unknown>;
	return (
		ir.version === 1 &&
		ir.kind === "jsxSafeIr" &&
		typeof ir.width === "number" &&
		Number.isFinite(ir.width) &&
		ir.width >= 1 &&
		ir.width <= 16_384 &&
		typeof ir.height === "number" &&
		Number.isFinite(ir.height) &&
		ir.height >= 1 &&
		ir.height <= 16_384 &&
		typeof ir.durationSeconds === "number" &&
		Number.isFinite(ir.durationSeconds) &&
		ir.durationSeconds > 0 &&
		ir.durationSeconds <= 3_600 &&
		typeof ir.fps === "number" &&
		Number.isFinite(ir.fps) &&
		ir.fps > 0 &&
		ir.fps <= 240 &&
		security.sourceExecuted === false &&
		security.interpreter === "deterministic-allowlisted-ir-v1" &&
		security.networkAccess === "disabled" &&
		security.fileAccess === "disabled" &&
		security.sandboxOrigin === "opaque"
	);
}
