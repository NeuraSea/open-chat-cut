"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TransitionTopIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import {
	AlertTriangle,
	CheckCircle2,
	Clock3,
	Copy,
	Download,
	FileArchive,
	LoaderCircle,
	RotateCcw,
	ShieldCheck,
	Square,
	XCircle,
} from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
	Dialog,
	DialogBody,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import {
	Select,
	SelectContent,
	SelectGroup,
	SelectItem,
	SelectLabel,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { useEditor } from "@/editor/use-editor";
import {
	buildProfessionalExportArguments,
	defaultProfessionalExportFileName,
	exportFileNameFromJob,
	exportFormatFromJob,
	getProfessionalExportDescriptor,
	isExportJob,
	isPortableExportFileName,
	type ProfessionalExportFormat,
	withProfessionalExportExtension,
} from "@/export/professional";
import {
	LocalCoreError,
	localCoreClient,
	type JobRecord,
	type LocalCoreEvent,
} from "@/services/local-core";
import type { DomainProjectDocument } from "@/services/local-core/project-adapter";
import { cn } from "@/utils/ui";
import { TICKS_PER_SECOND } from "@/wasm";

const ACTIVE_JOB_STATES = new Set([
	"queued",
	"running",
	"waitingForProvider",
	"paused",
]);

const DELIVERY_FORMATS = [
	{ value: "mp4", label: "MP4 · H.264/AAC", group: "Video", extension: "mp4" },
	{ value: "webm", label: "WebM", group: "Video", extension: "webm" },
	{
		value: "prores-4444",
		label: "ProRes 4444 · alpha",
		group: "Video",
		extension: "mov",
	},
	{ value: "wav", label: "WAV", group: "Audio", extension: "wav" },
	{ value: "mp3", label: "MP3", group: "Audio", extension: "mp3" },
	{ value: "png", label: "PNG frame", group: "Frames", extension: "png" },
	{
		value: "png-sequence",
		label: "PNG sequence · ZIP",
		group: "Frames",
		extension: "zip",
	},
	{ value: "srt", label: "SRT", group: "Captions", extension: "srt" },
	{ value: "vtt", label: "WebVTT", group: "Captions", extension: "vtt" },
	{ value: "ass", label: "ASS", group: "Captions", extension: "ass" },
	{
		value: "txt",
		label: "Plain transcript",
		group: "Captions",
		extension: "txt",
	},
	{
		value: "premiere-xml",
		label: "Premiere XML",
		group: "Interchange",
		extension: "xml",
	},
	{
		value: "resolve-xml",
		label: "Resolve XML",
		group: "Interchange",
		extension: "xml",
	},
	{
		value: "project-package",
		label: "Portable project",
		group: "Interchange",
		extension: "occproj",
	},
] as const;

type DeliveryFormat = ProfessionalExportFormat;
type DeliveryGroup = (typeof DELIVERY_FORMATS)[number]["group"];

const GROUPS: DeliveryGroup[] = [
	"Video",
	"Audio",
	"Frames",
	"Captions",
	"Interchange",
];

interface StartExportData {
	job: JobRecord;
	pinnedRevision: number;
	documentHash: string;
	renderer: string;
	outputPath: string;
	warnings?: Array<{ code?: string; message?: string }>;
}

function formatDefinition(format: DeliveryFormat) {
	return DELIVERY_FORMATS.find((candidate) => candidate.value === format)!;
}

function supportsRange(format: DeliveryFormat): boolean {
	return [
		"mp4",
		"webm",
		"prores-4444",
		"wav",
		"mp3",
		"png",
		"png-sequence",
	].includes(format);
}

function supportsVideoSettings(format: DeliveryFormat): boolean {
	return ["mp4", "webm", "prores-4444", "png", "png-sequence"].includes(format);
}

function outputNameFor({
	name,
	format,
	revision = 0,
}: {
	name: string;
	format: DeliveryFormat;
	revision?: number;
}): string {
	return defaultProfessionalExportFileName({
		projectName: name,
		revision,
		format,
	});
}

function replaceOutputExtension({
	name,
	format,
}: {
	name: string;
	format: DeliveryFormat;
}): string {
	return withProfessionalExportExtension({ fileName: name, format });
}

function isActiveJob(job: JobRecord | null): boolean {
	return !!job && ACTIVE_JOB_STATES.has(job.state);
}

function jobOutputName(job: JobRecord): string {
	return exportFileNameFromJob(job);
}

function jobFormat(job: JobRecord): string {
	return exportFormatFromJob(job) ?? job.kind;
}

function formatBytes(value: unknown): string | null {
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0)
		return null;
	if (value < 1024) return `${value} B`;
	if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
	if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
	return `${(value / 1024 ** 3).toFixed(2)} GB`;
}

function jobErrorMessage(job: JobRecord): string {
	return job.error?.message ?? job.message ?? "The export worker failed";
}

function preferLatestJob({
	current,
	incoming,
}: {
	current: JobRecord | undefined;
	incoming: JobRecord;
}): JobRecord {
	if (!current || current.id !== incoming.id) return incoming;
	const currentTime = Date.parse(current.updatedAt);
	const incomingTime = Date.parse(incoming.updatedAt);
	if (Number.isFinite(currentTime) && Number.isFinite(incomingTime)) {
		if (incomingTime > currentTime) return incoming;
		if (incomingTime < currentTime) return current;
	}
	const terminal = new Set(["succeeded", "failed", "cancelled"]);
	if (terminal.has(current.state) && !terminal.has(incoming.state))
		return current;
	return incoming;
}

export function ExportButton() {
	const [open, setOpen] = useState(false);
	const activeProject = useEditor((editor) => editor.project.getActiveOrNull());

	return (
		<>
			<button
				type="button"
				className={cn(
					"flex items-center gap-1.5 rounded-md bg-[#38BDF8] px-[0.12rem] py-[0.12rem] text-white",
					activeProject ? "cursor-pointer" : "cursor-not-allowed opacity-50",
				)}
				disabled={!activeProject}
				onClick={() => setOpen(true)}
			>
				<div className="relative flex items-center gap-1.5 rounded-[0.6rem] bg-linear-270 from-[#2567EC] to-[#37B6F7] px-4 py-1 shadow-[0_1px_3px_0px_rgba(0,0,0,0.65)]">
					<HugeiconsIcon icon={TransitionTopIcon} className="z-50 size-3.5" />
					<span className="z-50 text-[0.875rem]">Export</span>
					<div className="absolute top-0 left-0 z-10 flex size-full items-center justify-center rounded-[0.6rem] bg-linear-to-t from-white/0 to-white/50">
						<div className="absolute top-[0.08rem] z-50 h-[calc(100%-2px)] w-[calc(100%-2px)] rounded-[0.6rem] bg-linear-270 from-[#2567EC] to-[#37B6F7]" />
					</div>
				</div>
			</button>
			{activeProject ? (
				<ProfessionalExportDialog
					key={activeProject.metadata.id}
					open={open}
					onOpenChange={setOpen}
				/>
			) : null}
		</>
	);
}

function ProfessionalExportDialog({
	open,
	onOpenChange,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const editor = useEditor();
	const activeProject = useEditor((value) => value.project.getActive());
	const timelineDuration = useEditor((value) =>
		value.timeline.getTotalDuration(),
	);
	const projectId = activeProject.metadata.id;
	const projectFps =
		activeProject.settings.fps.numerator /
		activeProject.settings.fps.denominator;
	const durationSeconds = Math.max(0, timelineDuration / TICKS_PER_SECOND);
	const [format, setFormat] = useState<DeliveryFormat>("mp4");
	const [outputName, setOutputName] = useState(() =>
		outputNameFor({
			name: activeProject.metadata.name,
			format: "mp4",
		}),
	);
	const [width, setWidth] = useState(activeProject.settings.canvasSize.width);
	const [height, setHeight] = useState(
		activeProject.settings.canvasSize.height,
	);
	const [fps, setFps] = useState(Number(projectFps.toFixed(3)));
	const [customRange, setCustomRange] = useState(false);
	const [rangeStart, setRangeStart] = useState(0);
	const [rangeEnd, setRangeEnd] = useState(Number(durationSeconds.toFixed(3)));
	const [allowOverwrite, setAllowOverwrite] = useState(false);
	const [overwriteConfirmationOpen, setOverwriteConfirmationOpen] =
		useState(false);
	const [latestRevision, setLatestRevision] = useState<number | null>(null);
	const [latestDocumentHash, setLatestDocumentHash] = useState<string | null>(
		null,
	);
	const [captionTrackId, setCaptionTrackId] = useState("auto");
	const [projectDocument, setProjectDocument] =
		useState<DomainProjectDocument | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);
	const [isRefreshing, setIsRefreshing] = useState(false);
	const [downloadingJobId, setDownloadingJobId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [warning, setWarning] = useState<string | null>(null);
	const [currentJob, setCurrentJob] = useState<JobRecord | null>(null);
	const [recentJobs, setRecentJobs] = useState<JobRecord[]>([]);
	const [startMetadata, setStartMetadata] = useState<StartExportData | null>(
		null,
	);
	const pendingSubmission = useRef<{ fingerprint: string; key: string } | null>(
		null,
	);
	const outputNameEdited = useRef(false);

	const mergeJob = useCallback((job: JobRecord) => {
		setRecentJobs((current) => {
			const accepted = preferLatestJob({
				current: current.find((candidate) => candidate.id === job.id),
				incoming: job,
			});
			const next = [
				accepted,
				...current.filter((candidate) => candidate.id !== job.id),
			];
			return next
				.sort(
					(left, right) =>
						new Date(right.createdAt).getTime() -
						new Date(left.createdAt).getTime(),
				)
				.slice(0, 8);
		});
		setCurrentJob((current) => {
			if (current?.id === job.id)
				return preferLatestJob({ current, incoming: job });
			if (isActiveJob(job) && !isActiveJob(current)) return job;
			return current;
		});
	}, []);

	const refreshJobs = useCallback(async () => {
		setIsRefreshing(true);
		try {
			const jobs = (
				await localCoreClient.listJobs({ projectId, limit: 30 })
			).filter(isExportJob);
			setRecentJobs(jobs.slice(0, 8));
			setCurrentJob((current) => {
				const refreshed = current
					? jobs.find((candidate) => candidate.id === current.id)
					: undefined;
				return (
					refreshed ??
					jobs.find((candidate) => isActiveJob(candidate)) ??
					jobs[0] ??
					null
				);
			});
		} catch (nextError) {
			setError(
				nextError instanceof Error
					? nextError.message
					: "Could not read persistent export jobs",
			);
		} finally {
			setIsRefreshing(false);
		}
	}, [projectId]);

	const refreshProjectPin = useCallback(async () => {
		try {
			const envelope = await localCoreClient.readProject<DomainProjectDocument>(
				{
					projectId,
				},
			);
			setLatestRevision(envelope.revision);
			setLatestDocumentHash(envelope.documentHash);
			setProjectDocument(envelope.document);
			if (!outputNameEdited.current) {
				setOutputName(
					outputNameFor({
						name: envelope.document.name,
						format,
						revision: envelope.revision,
					}),
				);
			}
		} catch (nextError) {
			setError(
				nextError instanceof Error
					? nextError.message
					: "Could not read the current project revision",
			);
		}
	}, [format, projectId]);

	useEffect(() => {
		if (!open) return;
		const timer = window.setTimeout(() => {
			void refreshJobs();
			void refreshProjectPin();
		}, 0);
		return () => window.clearTimeout(timer);
	}, [open, refreshJobs, refreshProjectPin]);

	useEffect(() => {
		return localCoreClient.connectEvents({
			onEvent: (event: LocalCoreEvent) => {
				if (event.type === "connected" && open) {
					void refreshJobs();
					void refreshProjectPin();
					return;
				}
				if (
					event.type === "revision.changed" &&
					event.projectId === projectId
				) {
					setLatestRevision(event.revision);
					return;
				}
				if (
					event.type === "job.changed" &&
					event.job.projectId === projectId &&
					isExportJob(event.job)
				) {
					mergeJob(event.job);
				}
			},
		});
	}, [mergeJob, open, projectId, refreshJobs, refreshProjectPin]);

	const captionTracks = useMemo(() => {
		if (!projectDocument) return [];
		return projectDocument.scenes.flatMap((scene) =>
			scene.tracks
				.filter((track) =>
					track.items.some((item) => item.content.type === "caption"),
				)
				.map((track) => ({ id: track.id, name: track.name })),
		);
	}, [projectDocument]);

	const settingsVisible =
		supportsRange(format) || supportsVideoSettings(format);
	const activeExport = isActiveJob(currentJob);
	const selectedDefinition = formatDefinition(format);
	const groups = useMemo(
		() =>
			GROUPS.map((group) => ({
				group,
				formats: DELIVERY_FORMATS.filter(
					(candidate) => candidate.group === group,
				),
			})),
		[],
	);

	const handleFormatChange = (next: string) => {
		const candidate = DELIVERY_FORMATS.find((item) => item.value === next);
		if (!candidate) return;
		const nextFormat = candidate.value;
		setFormat(nextFormat);
		setOutputName((current) =>
			replaceOutputExtension({ name: current, format: nextFormat }),
		);
		setError(null);
		setWarning(null);
	};

	const startExport = async () => {
		if (isSubmitting || activeExport) return;
		setIsSubmitting(true);
		setError(null);
		setWarning(null);
		try {
			if (!isPortableExportFileName(outputName)) {
				throw new Error(
					"Output must be one portable file name without folders",
				);
			}
			const expectedExtension = `.${selectedDefinition.extension}`;
			if (!outputName.toLowerCase().endsWith(expectedExtension.toLowerCase())) {
				throw new Error(
					`This format requires the ${expectedExtension} extension`,
				);
			}
			if (supportsVideoSettings(format)) {
				if (
					!Number.isInteger(width) ||
					!Number.isInteger(height) ||
					width < 16 ||
					height < 16
				) {
					throw new Error(
						"Width and height must be whole numbers of at least 16 pixels",
					);
				}
				if (!Number.isFinite(fps) || fps < 1 || fps > 240) {
					throw new Error("Frame rate must be between 1 and 240 fps");
				}
			}
			if (customRange && supportsRange(format)) {
				if (
					!Number.isFinite(rangeStart) ||
					!Number.isFinite(rangeEnd) ||
					rangeStart < 0 ||
					rangeEnd <= rangeStart
				) {
					throw new Error(
						"The selected range must have a non-negative start and a later end",
					);
				}
			}

			// Flush the browser scene before reading the authoritative envelope. The
			// resulting revision is then pinned for the complete background job.
			await editor.save.flush();
			const envelope = await localCoreClient.readProject<DomainProjectDocument>(
				{ projectId },
			);
			setLatestRevision(envelope.revision);
			setLatestDocumentHash(envelope.documentHash);
			setProjectDocument(envelope.document);
			const effectiveOutputName = outputNameEdited.current
				? outputName
				: outputNameFor({
						name: envelope.document.name,
						format,
						revision: envelope.revision,
					});
			if (!outputNameEdited.current) setOutputName(effectiveOutputName);
			const args = buildProfessionalExportArguments({
				projectId,
				expectedRevision: envelope.revision,
				format,
				outputPath: effectiveOutputName,
				allowOverwrite,
				...(supportsVideoSettings(format)
					? { resolution: { width, height }, fps }
					: {}),
				...(customRange && supportsRange(format)
					? { range: { startSeconds: rangeStart, endSeconds: rangeEnd } }
					: {}),
				...(getProfessionalExportDescriptor(format).category === "subtitle" &&
				captionTrackId !== "auto"
					? { captionTrackId }
					: {}),
			});
			const fingerprint = JSON.stringify(args);
			const idempotencyKey =
				pendingSubmission.current?.fingerprint === fingerprint
					? pendingSubmission.current.key
					: crypto.randomUUID();
			pendingSubmission.current = { fingerprint, key: idempotencyKey };
			const result = await localCoreClient.invokeTool<StartExportData>({
				name: "start_export",
				arguments: args,
				idempotencyKey,
			});
			if (!result.ok || !result.data?.job) {
				throw new Error(
					result.error?.message ?? "The daemon did not start the export",
				);
			}
			pendingSubmission.current = null;
			setStartMetadata(result.data);
			mergeJob(result.data.job);
			const firstWarning = result.data.warnings?.find(
				(item) => item.message,
			)?.message;
			if (firstWarning) setWarning(firstWarning);
			toast.success(`Export pinned to revision ${result.data.pinnedRevision}`, {
				description: "The durable daemon job continues if this browser closes.",
			});
		} catch (nextError) {
			// Structured daemon errors are definitive. Only preserve the key after a
			// transport ambiguity so an exact retry cannot enqueue duplicate work.
			if (nextError instanceof LocalCoreError) pendingSubmission.current = null;
			const message =
				nextError instanceof Error
					? nextError.message
					: "Could not start export";
			setError(message);
		} finally {
			setIsSubmitting(false);
		}
	};

	const requestStartExport = () => {
		if (allowOverwrite) {
			setOverwriteConfirmationOpen(true);
			return;
		}
		void startExport();
	};

	const cancelExport = async () => {
		if (!currentJob || !isActiveJob(currentJob)) return;
		try {
			const job = await localCoreClient.cancelJob({ jobId: currentJob.id });
			mergeJob(job);
		} catch (nextError) {
			setError(
				nextError instanceof Error
					? nextError.message
					: "Could not cancel export",
			);
		}
	};

	const downloadArtifact = async (job: JobRecord) => {
		setDownloadingJobId(job.id);
		setError(null);
		try {
			const url = await localCoreClient.getJobArtifactUrl({ jobId: job.id });
			const anchor = document.createElement("a");
			anchor.href = url;
			anchor.download = jobOutputName(job);
			document.body.appendChild(anchor);
			anchor.click();
			anchor.remove();
		} catch (nextError) {
			setError(
				nextError instanceof Error
					? nextError.message
					: "Could not download export",
			);
		} finally {
			setDownloadingJobId(null);
		}
	};

	return (
		<>
			<Dialog open={open} onOpenChange={onOpenChange}>
				<DialogContent className="max-h-[88vh] max-w-4xl grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden p-0">
					<DialogHeader>
						<div className="flex items-center gap-2 pr-8">
							<div className="bg-primary/10 text-primary flex size-9 items-center justify-center rounded-lg">
								<FileArchive className="size-4" />
							</div>
							<div>
								<DialogTitle>Professional export</DialogTitle>
								<DialogDescription className="mt-1">
									Revision-pinned rendering through the local daemon and FFmpeg
								</DialogDescription>
							</div>
						</div>
					</DialogHeader>

					<DialogBody className="min-h-0 overflow-y-auto p-0">
						<div className="grid min-h-0 lg:grid-cols-[minmax(0,1fr)_22rem]">
							<div className="space-y-5 p-6 lg:border-r">
								<div className="border-primary/20 bg-primary/5 flex items-center justify-between gap-3 rounded-lg border px-3 py-2.5">
									<div className="flex min-w-0 items-center gap-2.5">
										<ShieldCheck className="text-primary size-4 shrink-0" />
										<div className="min-w-0">
											<p className="text-xs font-medium">
												Latest durable revision{" "}
												{latestRevision === null ? "…" : `r${latestRevision}`}
											</p>
											<p className="text-muted-foreground truncate font-mono text-[10px]">
												{latestDocumentHash
													? `${latestDocumentHash.slice(0, 16)}…`
													: "Reading document hash…"}
											</p>
										</div>
									</div>
									<span className="text-muted-foreground text-right text-[10px]">
										Autosave is flushed before pinning
									</span>
								</div>
								<div className="grid gap-4 sm:grid-cols-2">
									<div className="space-y-2">
										<Label htmlFor="delivery-format">Delivery format</Label>
										<Select
											value={format}
											onValueChange={handleFormatChange}
											disabled={activeExport}
										>
											<SelectTrigger
												id="delivery-format"
												variant="outline"
												className="h-9 w-full"
											>
												<SelectValue />
											</SelectTrigger>
											<SelectContent className="max-h-80">
												{groups.map(({ group, formats }) => (
													<SelectGroup key={group}>
														<SelectLabel>{group}</SelectLabel>
														{formats.map((candidate) => (
															<SelectItem
																key={candidate.value}
																value={candidate.value}
															>
																{candidate.label}
															</SelectItem>
														))}
													</SelectGroup>
												))}
											</SelectContent>
										</Select>
									</div>
									<div className="space-y-2">
										<Label htmlFor="export-output-name">Output file</Label>
										<Input
											id="export-output-name"
											value={outputName}
											onChange={(event) => {
												outputNameEdited.current = true;
												setOutputName(event.target.value);
											}}
											disabled={activeExport}
											className="font-mono text-xs"
										/>
									</div>
								</div>

								{supportsVideoSettings(format) ? (
									<div className="space-y-3 rounded-lg border p-4">
										<div>
											<p className="text-sm font-medium">Picture</p>
											<p className="text-muted-foreground text-xs">
												The headless renderer and encoder share these exact
												dimensions.
											</p>
										</div>
										<div className="grid grid-cols-3 gap-3">
											<div className="space-y-1.5">
												<Label htmlFor="export-width" className="text-xs">
													Width
												</Label>
												<Input
													id="export-width"
													type="number"
													min={16}
													max={16384}
													value={width}
													disabled={activeExport}
													onChange={(event) =>
														setWidth(Number(event.target.value))
													}
												/>
											</div>
											<div className="space-y-1.5">
												<Label htmlFor="export-height" className="text-xs">
													Height
												</Label>
												<Input
													id="export-height"
													type="number"
													min={16}
													max={16384}
													value={height}
													disabled={activeExport}
													onChange={(event) =>
														setHeight(Number(event.target.value))
													}
												/>
											</div>
											<div className="space-y-1.5">
												<Label htmlFor="export-fps" className="text-xs">
													FPS
												</Label>
												<Input
													id="export-fps"
													type="number"
													min={1}
													max={240}
													step="0.001"
													value={fps}
													disabled={activeExport}
													onChange={(event) =>
														setFps(Number(event.target.value))
													}
												/>
											</div>
										</div>
									</div>
								) : null}

								{supportsRange(format) ? (
									<div className="space-y-3 rounded-lg border p-4">
										<div className="flex items-center justify-between gap-3">
											<div>
												<p className="text-sm font-medium">Timeline range</p>
												<p className="text-muted-foreground text-xs">
													Full timeline is {durationSeconds.toFixed(2)} seconds
												</p>
											</div>
											<Label
												htmlFor="custom-export-range"
												className="flex items-center gap-2 text-xs font-normal"
											>
												<Checkbox
													id="custom-export-range"
													checked={customRange}
													onCheckedChange={(checked) =>
														setCustomRange(checked === true)
													}
													disabled={activeExport}
												/>
												Custom range
											</Label>
										</div>
										{customRange ? (
											<div className="grid grid-cols-2 gap-3">
												<div className="space-y-1.5">
													<Label htmlFor="range-start" className="text-xs">
														Start (seconds)
													</Label>
													<Input
														id="range-start"
														type="number"
														min={0}
														step="0.001"
														value={rangeStart}
														disabled={activeExport}
														onChange={(event) =>
															setRangeStart(Number(event.target.value))
														}
													/>
												</div>
												<div className="space-y-1.5">
													<Label htmlFor="range-end" className="text-xs">
														End (seconds)
													</Label>
													<Input
														id="range-end"
														type="number"
														min={0}
														step="0.001"
														value={rangeEnd}
														disabled={activeExport}
														onChange={(event) =>
															setRangeEnd(Number(event.target.value))
														}
													/>
												</div>
											</div>
										) : null}
									</div>
								) : settingsVisible ? null : (
									<div className="bg-muted/35 text-muted-foreground rounded-lg border p-4 text-xs">
										This delivery is generated directly from the pinned Rust
										project document. Video rendering settings do not apply.
									</div>
								)}

								{getProfessionalExportDescriptor(format).category ===
								"subtitle" ? (
									<div className="space-y-2 rounded-lg border p-4">
										<div>
											<p className="text-sm font-medium">Caption track</p>
											<p className="text-muted-foreground text-xs">
												Export one semantic CaptionElement track from the pinned
												revision.
											</p>
										</div>
										<Select
											value={captionTrackId}
											onValueChange={setCaptionTrackId}
											disabled={activeExport}
										>
											<SelectTrigger variant="outline" className="h-9 w-full">
												<SelectValue />
											</SelectTrigger>
											<SelectContent>
												<SelectItem value="auto">
													First visible caption track
												</SelectItem>
												{captionTracks.map((track) => (
													<SelectItem key={track.id} value={track.id}>
														{track.name}
													</SelectItem>
												))}
											</SelectContent>
										</Select>
									</div>
								) : supportsVideoSettings(format) ? (
									<div className="bg-muted/35 text-muted-foreground rounded-lg border px-4 py-3 text-xs">
										Visible semantic caption tracks are burned in exactly as
										shown in the preview.
									</div>
								) : null}

								<Label
									htmlFor="allow-export-overwrite"
									className="flex items-start gap-3 rounded-lg border p-4 font-normal"
								>
									<Checkbox
										id="allow-export-overwrite"
										checked={allowOverwrite}
										onCheckedChange={(checked) => {
											setAllowOverwrite(checked === true);
											if (checked !== true) setOverwriteConfirmationOpen(false);
										}}
										disabled={activeExport}
									/>
									<span>
										<span className="block text-sm font-medium">
											Allow replacing an existing output
										</span>
										<span className="text-muted-foreground block text-xs">
											Off by default. The daemon otherwise refuses same-name
											files atomically.
										</span>
									</span>
								</Label>

								{error ? (
									<div className="border-destructive/30 bg-destructive/5 text-destructive flex gap-3 rounded-lg border p-3 text-sm">
										<AlertTriangle className="mt-0.5 size-4 shrink-0" />
										<div>
											<p className="font-medium">Export could not continue</p>
											<p className="mt-0.5 text-xs opacity-90">{error}</p>
										</div>
									</div>
								) : null}
								{warning ? (
									<div className="border-amber-500/30 bg-amber-500/5 text-amber-700 dark:text-amber-300 flex gap-3 rounded-lg border p-3 text-xs">
										<AlertTriangle className="size-4 shrink-0" />
										{warning}
									</div>
								) : null}
							</div>

							<div className="bg-muted/15 space-y-5 p-6">
								<div className="flex items-center justify-between">
									<div>
										<p className="text-sm font-medium">
											Persistent delivery job
										</p>
										<p className="text-muted-foreground text-xs">
											Safe to close this browser
										</p>
									</div>
									<Button
										variant="ghost"
										size="icon"
										className="size-8"
										onClick={() => void refreshJobs()}
										disabled={isRefreshing}
										aria-label="Refresh export jobs"
									>
										<RotateCcw
											className={cn("size-3.5", isRefreshing && "animate-spin")}
										/>
									</Button>
								</div>

								{currentJob ? (
									<JobStatusCard
										job={currentJob}
										metadata={
											startMetadata?.job.id === currentJob.id
												? startMetadata
												: null
										}
										downloading={downloadingJobId === currentJob.id}
										onCancel={cancelExport}
										onDownload={downloadArtifact}
									/>
								) : (
									<div className="text-muted-foreground flex min-h-40 flex-col items-center justify-center rounded-lg border border-dashed p-5 text-center">
										<Clock3 className="mb-3 size-5" />
										<p className="text-sm font-medium text-foreground">
											No delivery selected
										</p>
										<p className="mt-1 max-w-52 text-xs">
											Configure an export; it will remain visible here after
											reconnect or daemon restart.
										</p>
									</div>
								)}

								<div className="space-y-2">
									<p className="text-muted-foreground text-[11px] font-medium uppercase tracking-wider">
										Recent exports
									</p>
									<div className="space-y-1.5">
										{recentJobs.slice(0, 5).map((job) => (
											<button
												key={job.id}
												type="button"
												onClick={() => setCurrentJob(job)}
												className={cn(
													"hover:bg-accent/60 flex w-full items-center gap-2 rounded-md border px-2.5 py-2 text-left",
													currentJob?.id === job.id &&
														"border-primary/35 bg-primary/5",
												)}
											>
												<JobStateIcon job={job} />
												<span className="min-w-0 flex-1">
													<span className="block truncate text-xs font-medium">
														{jobOutputName(job)}
													</span>
													<span className="text-muted-foreground block truncate text-[10px]">
														r{job.revision ?? "?"} · {jobFormat(job)}
													</span>
												</span>
												<span className="text-muted-foreground text-[10px] capitalize">
													{job.state}
												</span>
											</button>
										))}
										{recentJobs.length === 0 ? (
											<p className="text-muted-foreground py-2 text-xs">
												No previous exports for this project.
											</p>
										) : null}
									</div>
								</div>
							</div>
						</div>
					</DialogBody>

					<DialogFooter className="items-center justify-between sm:justify-between">
						<div className="text-muted-foreground flex items-center gap-2 text-xs">
							<ShieldCheck className="size-3.5" />
							CAS revision · verified artifact ·{" "}
							{allowOverwrite ? "confirmed replace" : "no overwrite"}
						</div>
						<div className="flex gap-2">
							<Button variant="outline" onClick={() => onOpenChange(false)}>
								Close
							</Button>
							<Button
								onClick={requestStartExport}
								disabled={isSubmitting || activeExport}
								className="min-w-32 gap-2"
							>
								{isSubmitting ? (
									<LoaderCircle className="size-4 animate-spin" />
								) : (
									<TransitionExportIcon />
								)}
								{activeExport
									? "Export running"
									: isSubmitting
										? "Starting…"
										: "Start export"}
							</Button>
						</div>
					</DialogFooter>
				</DialogContent>
			</Dialog>
			<AlertDialog
				open={overwriteConfirmationOpen}
				onOpenChange={setOverwriteConfirmationOpen}
			>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>Replace an existing export?</AlertDialogTitle>
						<AlertDialogDescription>
							If <strong>{outputName}</strong> already exists in the private
							daemon export directory, it will be atomically replaced. The
							project and source assets are not modified.
						</AlertDialogDescription>
					</AlertDialogHeader>
					<div className="border-amber-500/30 bg-amber-500/5 rounded-md border p-3 text-xs">
						Pinned source:{" "}
						{latestRevision === null
							? "latest revision"
							: `revision r${latestRevision}`}
						<br />
						Effect: overwrite one same-name delivery artifact only
					</div>
					<AlertDialogFooter>
						<AlertDialogCancel>Keep existing file</AlertDialogCancel>
						<AlertDialogAction
							className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
							onClick={() => void startExport()}
						>
							Replace and export
						</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</>
	);
}

function TransitionExportIcon() {
	return <HugeiconsIcon icon={TransitionTopIcon} className="size-4" />;
}

function JobStateIcon({ job }: { job: JobRecord }) {
	if (isActiveJob(job))
		return (
			<LoaderCircle className="text-primary size-3.5 shrink-0 animate-spin" />
		);
	if (job.state === "succeeded")
		return <CheckCircle2 className="text-constructive size-3.5 shrink-0" />;
	if (job.state === "cancelled")
		return <Square className="text-muted-foreground size-3.5 shrink-0" />;
	return <XCircle className="text-destructive size-3.5 shrink-0" />;
}

function JobStatusCard({
	job,
	metadata,
	downloading,
	onCancel,
	onDownload,
}: {
	job: JobRecord;
	metadata: StartExportData | null;
	downloading: boolean;
	onCancel: () => void;
	onDownload: (job: JobRecord) => void;
}) {
	const outputPath =
		(typeof job.output?.outputPath === "string"
			? job.output.outputPath
			: null) ??
		metadata?.outputPath ??
		null;
	const byteSize = formatBytes(job.output?.byteSize);
	const sha256 =
		typeof job.output?.sha256 === "string" ? job.output.sha256 : null;

	return (
		<div className="space-y-3 rounded-lg border bg-background p-4 shadow-xs">
			<div className="flex items-start gap-3">
				<div
					className={cn(
						"flex size-8 shrink-0 items-center justify-center rounded-full",
						job.state === "succeeded"
							? "bg-constructive/10 text-constructive"
							: job.state === "failed"
								? "bg-destructive/10 text-destructive"
								: "bg-primary/10 text-primary",
					)}
				>
					<JobStateIcon job={job} />
				</div>
				<div className="min-w-0 flex-1">
					<p className="truncate text-sm font-medium">{jobOutputName(job)}</p>
					<p className="text-muted-foreground mt-0.5 text-xs">
						r{job.revision ?? metadata?.pinnedRevision ?? "?"} ·{" "}
						{jobFormat(job)}
					</p>
				</div>
				<span className="bg-muted rounded-full px-2 py-1 text-[10px] font-medium capitalize">
					{job.state}
				</span>
			</div>

			{isActiveJob(job) ? (
				<>
					<div className="space-y-1.5">
						<div className="text-muted-foreground flex justify-between text-[11px]">
							<span>
								{job.message ??
									(job.state === "queued" ? "Waiting for worker" : "Rendering")}
							</span>
							<span>{Math.round(job.progress * 100)}%</span>
						</div>
						<Progress value={job.progress * 100} />
					</div>
					<Button
						variant="outline"
						size="sm"
						className="w-full gap-2"
						onClick={onCancel}
					>
						<Square className="size-3" /> Cancel export
					</Button>
				</>
			) : null}

			{job.state === "succeeded" ? (
				<>
					<div className="bg-constructive/5 border-constructive/20 flex gap-2 rounded-md border p-2.5 text-xs">
						<ShieldCheck className="text-constructive size-4 shrink-0" />
						<span>
							<span className="block font-medium">Daemon verified</span>
							<span className="text-muted-foreground">
								{byteSize ?? "Completed artifact"}
								{sha256 ? ` · SHA-256 ${sha256.slice(0, 12)}…` : ""}
							</span>
						</span>
					</div>
					{outputPath ? (
						<div className="flex items-center gap-2">
							<code className="bg-muted min-w-0 flex-1 truncate rounded px-2 py-1.5 text-[10px]">
								{outputPath}
							</code>
							<Button
								variant="ghost"
								size="icon"
								className="size-7"
								aria-label="Copy output path"
								onClick={() => void navigator.clipboard.writeText(outputPath)}
							>
								<Copy className="size-3.5" />
							</Button>
						</div>
					) : null}
					<Button
						size="sm"
						className="w-full gap-2"
						disabled={downloading}
						onClick={() => onDownload(job)}
					>
						{downloading ? (
							<LoaderCircle className="size-3.5 animate-spin" />
						) : (
							<Download className="size-3.5" />
						)}
						Download verified artifact
					</Button>
				</>
			) : null}

			{job.state === "failed" ? (
				<div className="border-destructive/30 bg-destructive/5 text-destructive flex gap-2 rounded-md border p-2.5 text-xs">
					<XCircle className="size-4 shrink-0" />
					<span>{jobErrorMessage(job)}</span>
				</div>
			) : null}
			{job.state === "cancelled" ? (
				<p className="text-muted-foreground text-xs">
					This persistent job was cancelled. Configure a new delivery to try
					again.
				</p>
			) : null}
		</div>
	);
}
