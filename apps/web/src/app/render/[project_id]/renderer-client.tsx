"use client";

import { useEffect, useRef, useState } from "react";

import type { MediaAsset } from "@/media/types";
import { getProjectDurationFromScenes } from "@/timeline/scenes";
import {
	fromDomainProjectEnvelope,
	localCoreClient,
} from "@/services/local-core";
import type {
	DomainAsset,
	DomainProjectDocument,
	DomainProjectEnvelope,
} from "@/services/local-core/project-adapter";
import { CanvasRenderer } from "@/services/renderer/canvas-renderer";
import { initializeGpuRenderer } from "@/services/renderer/gpu-renderer";
import { buildScene } from "@/services/renderer/scene-builder";

type RendererStatus = "loading" | "ready" | "rendering" | "error";

type RenderResult = {
	projectId: string;
	revision: number;
	documentHash: string;
	timeTicks: number;
	width: number;
	height: number;
};

type HeadlessRendererApi = {
	readonly protocolVersion: 1;
	readonly projectId: string;
	readonly revision: number;
	readonly documentHash: string;
	renderAt(timeTicks: number): Promise<RenderResult>;
};

declare global {
	interface Window {
		__OPENCHATCUT_RENDERER__?: HeadlessRendererApi;
	}
}

function rendererState(status: RendererStatus, message?: string): void {
	document.documentElement.dataset.openchatcutRendererState = status;
	if (message)
		document.documentElement.dataset.openchatcutRendererMessage = message;
	else delete document.documentElement.dataset.openchatcutRendererMessage;
}

function safeMessage(error: unknown): string {
	const message = error instanceof Error ? error.message : String(error);
	return message.replace(/[\r\n\t]+/gu, " ").slice(0, 500);
}

function isRenderableAsset(
	asset: DomainAsset,
): asset is DomainAsset & { kind: MediaAsset["type"] } {
	return (
		(asset.kind === "video" ||
			asset.kind === "image" ||
			asset.kind === "audio") &&
		(typeof asset.contentHash === "string" ||
			(typeof asset.linkedFile?.fingerprintSha256 === "string" &&
				asset.linkedFile.portable === false))
	);
}

async function hydrateManagedAssets({
	projectId,
	document,
	signal,
}: {
	projectId: string;
	document: DomainProjectDocument;
	signal: AbortSignal;
}): Promise<{ assets: MediaAsset[]; revoke: () => void }> {
	const urls: string[] = [];
	try {
		const assets = await Promise.all(
			document.assets.filter(isRenderableAsset).map(async (asset) => {
				if (signal.aborted)
					throw new DOMException("Render cancelled", "AbortError");
				const blob = await localCoreClient.downloadManagedMedia({
					projectId,
					assetId: asset.id,
				});
				if (signal.aborted)
					throw new DOMException("Render cancelled", "AbortError");
				const file = new File([blob], asset.name, {
					type:
						blob.type ||
						asset.managedMedia?.mimeType ||
						asset.linkedFile?.mimeType ||
						"application/octet-stream",
					lastModified: asset.managedMedia?.lastModified ?? 0,
				});
				const url = URL.createObjectURL(file);
				urls.push(url);
				return {
					id: asset.id,
					name: asset.name,
					type: asset.kind,
					file,
					url,
					contentHash:
						asset.contentHash ??
						`linked:${asset.linkedFile?.fingerprintSha256 ?? "invalid"}`,
					...(asset.width !== undefined ? { width: asset.width } : {}),
					...(asset.height !== undefined ? { height: asset.height } : {}),
					...(asset.durationTicks !== undefined
						? { duration: asset.durationTicks / 120_000 }
						: {}),
					hasAudio: asset.hasAudio,
				} satisfies MediaAsset;
			}),
		);
		return {
			assets,
			revoke: () => urls.forEach((url) => URL.revokeObjectURL(url)),
		};
	} catch (error) {
		urls.forEach((url) => URL.revokeObjectURL(url));
		throw error;
	}
}

export function HeadlessRendererClient({
	projectId,
	revision,
	previewWidth,
}: {
	projectId: string;
	revision: number;
	previewWidth: number;
}) {
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const [status, setStatus] = useState<RendererStatus>("loading");
	const [message, setMessage] = useState("Loading pinned project revision");
	const [displayHeight, setDisplayHeight] = useState(720);

	useEffect(() => {
		const controller = new AbortController();
		let revokeAssets: (() => void) | undefined;
		let renderChain: Promise<unknown> = Promise.resolve();
		let disposed = false;
		rendererState("loading");
		delete window.__OPENCHATCUT_RENDERER__;

		void (async () => {
			const envelope =
				await localCoreClient.readProjectRevision<DomainProjectDocument>({
					projectId,
					revision,
				});
			if (controller.signal.aborted) return;
			if (
				envelope.revision !== revision ||
				envelope.document.id !== projectId
			) {
				throw new Error("Daemon returned a different project revision");
			}

			const project = fromDomainProjectEnvelope({
				envelope: envelope as DomainProjectEnvelope,
			});
			const scene =
				project.scenes.find(
					(candidate) => candidate.id === project.currentSceneId,
				) ??
				project.scenes.find((candidate) => candidate.isMain) ??
				project.scenes[0];
			if (!scene) throw new Error("Pinned project has no scene to render");

			const hydrated = await hydrateManagedAssets({
				projectId,
				document: envelope.document,
				signal: controller.signal,
			});
			revokeAssets = hydrated.revoke;
			if (controller.signal.aborted) return;

			const duration = getProjectDurationFromScenes({ scenes: [scene] });
			if (duration <= 0)
				throw new Error("Pinned scene has no renderable duration");
			const canvasSize = project.settings.canvasSize;
			const targetCanvas = canvasRef.current;
			if (!targetCanvas) throw new Error("Renderer canvas is unavailable");
			targetCanvas.width = canvasSize.width;
			targetCanvas.height = canvasSize.height;
			setDisplayHeight(
				Math.max(
					1,
					Math.round((previewWidth * canvasSize.height) / canvasSize.width),
				),
			);

			await initializeGpuRenderer();
			const renderTree = buildScene({
				tracks: scene.tracks,
				mediaAssets: hydrated.assets,
				duration,
				canvasSize,
				background: project.settings.background,
			});
			const renderer = new CanvasRenderer({
				width: canvasSize.width,
				height: canvasSize.height,
				fps: project.settings.fps,
			});

			const api: HeadlessRendererApi = {
				protocolVersion: 1,
				projectId,
				revision,
				documentHash: envelope.documentHash,
				renderAt(timeTicks) {
					if (
						!Number.isSafeInteger(timeTicks) ||
						timeTicks < 0 ||
						timeTicks >= duration
					) {
						return Promise.reject(
							new RangeError(
								`timeTicks must be an integer in [0, ${duration})`,
							),
						);
					}
					const task = renderChain.then(async () => {
						if (disposed || controller.signal.aborted) {
							throw new DOMException("Renderer disposed", "AbortError");
						}
						setStatus("rendering");
						rendererState("rendering");
						await renderer.renderToCanvas({
							node: renderTree,
							time: timeTicks,
							targetCanvas,
						});
						setStatus("ready");
						rendererState("ready");
						return {
							projectId,
							revision,
							documentHash: envelope.documentHash,
							timeTicks,
							width: canvasSize.width,
							height: canvasSize.height,
						};
					});
					renderChain = task.catch(() => undefined);
					return task;
				},
			};
			window.__OPENCHATCUT_RENDERER__ = api;
			setStatus("ready");
			setMessage("Pinned renderer ready");
			rendererState("ready");
		})().catch((error: unknown) => {
			if (controller.signal.aborted) return;
			const nextMessage = safeMessage(error);
			setStatus("error");
			setMessage(nextMessage);
			rendererState("error", nextMessage);
		});

		return () => {
			disposed = true;
			controller.abort();
			revokeAssets?.();
			delete window.__OPENCHATCUT_RENDERER__;
		};
	}, [previewWidth, projectId, revision]);

	return (
		<main
			data-openchatcut-renderer-state={status}
			aria-label="OpenChatCut headless renderer"
			style={{
				alignItems: "flex-start",
				background: "#000",
				display: "flex",
				height: `${displayHeight}px`,
				justifyContent: "flex-start",
				overflow: "hidden",
				width: `${previewWidth}px`,
			}}
		>
			<canvas
				ref={canvasRef}
				data-openchatcut-render-canvas
				aria-label="Pinned project frame"
				style={{
					display: "block",
					height: `${displayHeight}px`,
					width: `${previewWidth}px`,
				}}
			/>
			{status === "error" ? (
				<p
					role="alert"
					style={{ color: "white", left: 16, position: "absolute", top: 16 }}
				>
					{message}
				</p>
			) : null}
		</main>
	);
}
