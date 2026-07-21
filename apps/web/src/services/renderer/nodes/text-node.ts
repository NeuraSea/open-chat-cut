import { BaseNode } from "./base-node";
import type { TextElement } from "@/timeline";
import type { EffectPass } from "@/effects/types";
import type { BlendMode, Transform } from "@/rendering";
import { drawMeasuredTextLayout } from "@/text/primitives";
import type { MeasuredTextElement } from "@/text/measure-element";
import { setCanvasLetterSpacing } from "@/text/layout";

export type TextNodeParams = TextElement & {
	transform: Transform;
	opacity: number;
	blendMode?: BlendMode;
	canvasCenter: { x: number; y: number };
	canvasHeight: number;
	textBaseline?: CanvasTextBaseline;
};

export interface ResolvedTextNodeState {
	transform: Transform;
	opacity: number;
	textColor: string;
	backgroundColor: string;
	effectPasses: EffectPass[][];
	measuredText: MeasuredTextElement;
	captionHighlight?: {
		lineIndex: number;
		prefix: string;
		text: string;
		color: string;
	};
}

export class TextNode extends BaseNode<TextNodeParams, ResolvedTextNodeState> {}

export function renderTextToContext({
	node,
	ctx,
}: {
	node: TextNode;
	ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
}): void {
	const resolved = node.resolved;
	if (!resolved) {
		return;
	}

	const x = resolved.transform.position.x + node.params.canvasCenter.x;
	const y = resolved.transform.position.y + node.params.canvasCenter.y;
	const baseline = node.params.textBaseline ?? "middle";

	ctx.save();
	ctx.translate(x, y);
	ctx.scale(resolved.transform.scaleX, resolved.transform.scaleY);
	if (resolved.transform.rotate) {
		ctx.rotate((resolved.transform.rotate * Math.PI) / 180);
	}

	drawMeasuredTextLayout({
		ctx,
		layout: resolved.measuredText,
		textColor: resolved.textColor,
		background: resolved.measuredText.resolvedBackground,
		backgroundColor: resolved.backgroundColor,
		textBaseline: baseline,
	});

	if (resolved.captionHighlight) {
		const { lineIndex, prefix, text, color } = resolved.captionHighlight;
		const lineMetric = resolved.measuredText.lineMetrics[lineIndex];
		if (lineMetric) {
			ctx.font = resolved.measuredText.fontString;
			ctx.textBaseline = baseline;
			setCanvasLetterSpacing({
				ctx,
				letterSpacingPx: resolved.measuredText.letterSpacing,
			});
			const lineStart =
				resolved.measuredText.textAlign === "center"
					? -lineMetric.width / 2
					: resolved.measuredText.textAlign === "right"
						? -lineMetric.width
						: 0;
			const prefixWidth = ctx.measureText(prefix).width;
			const lineY =
				lineIndex * resolved.measuredText.lineHeightPx -
				resolved.measuredText.block.visualCenterOffset;
			ctx.fillStyle = color;
			ctx.textAlign = "left";
			ctx.fillText(text, lineStart + prefixWidth, lineY);
		}
	}

	ctx.restore();
}
