import { createCanvasSurface } from "../canvas-utils";
import {
	DEFAULT_GRAPHIC_SOURCE_SIZE,
	getGraphicDefinition,
	registerDefaultGraphics,
} from "@/graphics";
import type { ParamValues } from "@/params";
import type { MediaAsset } from "@/media/types";
import {
	createMotionGraphicMediaResolver,
	renderMotionGraphicDsl,
} from "@/motion-graphics/render-dsl";
import { renderMotionGraphicJsxIr } from "@/motion-graphics/render-jsx-ir";
import {
	isMotionGraphicDsl,
	isMotionGraphicJsxDefinition,
	type MotionGraphicDefinition,
} from "@/motion-graphics/types";
import { motionGraphicTimeSeconds } from "@/motion-graphics/time";
import {
	VisualNode,
	type ResolvedVisualNodeState,
	type VisualNodeParams,
} from "./visual-node";

export interface GraphicNodeParams extends VisualNodeParams {
	definitionId: string;
	params: ParamValues;
	motionGraphic?: MotionGraphicDefinition;
	mediaMap: Map<string, MediaAsset>;
}

export interface ResolvedGraphicNodeState extends ResolvedVisualNodeState {
	resolvedParams: ParamValues;
}

export class GraphicNode extends VisualNode<
	GraphicNodeParams,
	ResolvedGraphicNodeState
> {
	private cachedKey: string | null = null;
	private cachedSource: OffscreenCanvas | null = null;
	private readonly mediaResolver;

	constructor(params: GraphicNodeParams) {
		super(params);
		registerDefaultGraphics();
		this.mediaResolver = createMotionGraphicMediaResolver(params.mediaMap);
	}

	get motionGraphicDsl() {
		const value = this.params.motionGraphic?.definition;
		return isMotionGraphicDsl(value) ? value : null;
	}

	get motionGraphicJsx() {
		const value = this.params.motionGraphic?.definition;
		return isMotionGraphicJsxDefinition(value) ? value : null;
	}

	get hasMotionGraphicDefinition(): boolean {
		return this.motionGraphicDsl !== null || this.motionGraphicJsx !== null;
	}

	get sourceSize(): { width: number; height: number } {
		const definition = this.motionGraphicDsl ?? this.motionGraphicJsx?.ir;
		return definition
			? { width: definition.width, height: definition.height }
			: {
					width: DEFAULT_GRAPHIC_SOURCE_SIZE,
					height: DEFAULT_GRAPHIC_SOURCE_SIZE,
				};
	}

	async getSource({
		resolvedParams,
		localTime,
	}: {
		resolvedParams: ParamValues;
		localTime: number;
	}): Promise<OffscreenCanvas> {
		const localTimeSeconds = motionGraphicTimeSeconds(localTime);
		const motionGraphic = this.motionGraphicDsl;
		if (motionGraphic) {
			const cacheKey = JSON.stringify({
				definition: motionGraphic,
				localTime: localTimeSeconds,
			});
			if (this.cachedSource && this.cachedKey === cacheKey) {
				return this.cachedSource;
			}
			const { canvas, context } = createCanvasSurface({
				width: motionGraphic.width,
				height: motionGraphic.height,
			});
			await renderMotionGraphicDsl({
				context,
				definition: motionGraphic,
				localTime: localTimeSeconds,
				media: this.mediaResolver,
			});
			this.cachedKey = cacheKey;
			this.cachedSource = canvas;
			return canvas;
		}
		const motionGraphicJsx = this.motionGraphicJsx;
		if (motionGraphicJsx) {
			const cacheKey = JSON.stringify({
				ir: motionGraphicJsx.ir,
				localTime: localTimeSeconds,
			});
			if (this.cachedSource && this.cachedKey === cacheKey) {
				return this.cachedSource;
			}
			const { canvas, context } = createCanvasSurface({
				width: motionGraphicJsx.ir.width,
				height: motionGraphicJsx.ir.height,
			});
			await renderMotionGraphicJsxIr({
				context,
				ir: motionGraphicJsx.ir,
				localTime: localTimeSeconds,
				media: this.mediaResolver,
			});
			this.cachedKey = cacheKey;
			this.cachedSource = canvas;
			return canvas;
		}
		const definition = getGraphicDefinition({
			definitionId: this.params.definitionId,
		});
		const cacheKey = JSON.stringify({
			definitionId: this.params.definitionId,
			params: resolvedParams,
		});
		if (this.cachedSource && this.cachedKey === cacheKey) {
			return this.cachedSource;
		}

		const { canvas, context } = createCanvasSurface({
			width: DEFAULT_GRAPHIC_SOURCE_SIZE,
			height: DEFAULT_GRAPHIC_SOURCE_SIZE,
		});

		definition.render({
			ctx: context,
			params: resolvedParams,
			width: DEFAULT_GRAPHIC_SOURCE_SIZE,
			height: DEFAULT_GRAPHIC_SOURCE_SIZE,
		});

		this.cachedKey = cacheKey;
		this.cachedSource = canvas;
		return canvas;
	}
}
