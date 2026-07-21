"use client";

import {
	FormEvent,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import {
	Bot,
	ArrowUp,
	Circle,
	CircleCheck,
	CircleOff,
	Clock3,
	History,
	LoaderCircle,
	Mic,
	MessageSquarePlus,
	Plus,
	RefreshCw,
	RotateCcw,
	RotateCw,
	ShieldCheck,
} from "lucide-react";
import { ProposalCard } from "@/agent/components/proposal-card";
import {
	workflowWithJobs,
	type PersistedWorkflow,
	type WorkflowProgress,
} from "@/agent/workflow-progress";
import { PanelView } from "@/components/editor/panels/assets/views/base-panel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useEditor } from "@/editor/use-editor";
import {
	LocalCoreError,
	localCoreClient,
	type AgentSession,
	type AgentSessionMessage,
	type AgentSessionSummary,
	type JobRecord,
	type LocalCoreEvent,
	type LocalCoreStatus,
	type ToolProposal,
} from "@/services/local-core";

type ViewMessage = AgentSessionMessage & {
	retryInstruction?: string;
};

type TurnProgress = {
	messageId: string;
	phase: string;
	label: string;
	provider: string;
	revision: number;
	startedAt: number;
	updatedAt: number;
	steps: TurnStep[];
};

type TurnStep = {
	phase: string;
	label: string;
	status: "active" | "done" | "pending";
};

type ProgressEvent = Pick<TurnProgress, "phase" | "label">;

const NEXT_PHASE: Record<string, ProgressEvent | undefined> = {
	preparingContext: {
		phase: "startingAppServer",
		label: "Starting the signed-in Codex app-server",
	},
	startingAppServer: {
		phase: "handshake",
		label: "Waiting for the Codex protocol handshake",
	},
	handshake: {
		phase: "connected",
		label: "Connecting to the signed-in Codex session",
	},
	connected: {
		phase: "turnQueued",
		label: "Submitting the pinned context to Codex",
	},
	turnQueued: {
		phase: "reasoning",
		label: "Planning a reversible edit",
	},
	reasoning: {
		phase: "responding",
		label: "Receiving the structured plan",
	},
	waitingForModel: {
		phase: "responding",
		label: "Receiving the structured plan",
	},
	responding: {
		phase: "validating",
		label: "Validating the proposed operations",
	},
	requestingProvider: {
		phase: "responding",
		label: "Receiving the provider response",
	},
	validating: {
		phase: "buildingProposal",
		label: "Building the approval diff",
	},
	compilingMotionGraphic: {
		phase: "buildingProposal",
		label: "Building the approval diff",
	},
};

function createTurnProgress({
	messageId,
	provider,
	revision,
}: {
	messageId: string;
	provider: string;
	revision: number;
}): TurnProgress {
	const now = Date.now();
	return {
		messageId,
		provider,
		revision,
		phase: "preparingContext",
		label: "Pinning the project context",
		startedAt: now,
		updatedAt: now,
		steps: [
			{
				phase: "preparingContext",
				label: "Pinning the project context",
				status: "active",
			},
		],
	};
}

function advanceTurnProgress({
	current,
	event,
}: {
	current: TurnProgress | null;
	event: ProgressEvent;
}): TurnProgress | null {
	if (!current) return current;
	const steps = current.steps.map((step) =>
		step.phase === current.phase && step.status === "active"
			? { ...step, status: "done" as const }
			: step,
	);
	const existing = steps.find((step) => step.phase === event.phase);
	if (existing) {
		existing.label = event.label;
		existing.status = "active";
	} else {
		steps.push({ ...event, status: "active" });
	}
	return {
		...current,
		phase: event.phase,
		label: event.label,
		updatedAt: Date.now(),
		steps,
	};
}

function formatElapsed(milliseconds: number): string {
	const seconds = Math.max(0, Math.floor(milliseconds / 1000));
	if (seconds < 60) return `${seconds}s`;
	return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
}

function AgentProgressPanel({
	progress,
	now,
}: {
	progress: TurnProgress;
	now: number;
}) {
	const next = NEXT_PHASE[progress.phase];
	const steps =
		next && !progress.steps.some((step) => step.phase === next.phase)
			? [...progress.steps, { ...next, status: "pending" as const }]
			: progress.steps;
	const elapsed = now - progress.startedAt;
	return (
		<div className="border-border bg-muted/30 mt-2 rounded-xl border p-3 text-xs">
			<div className="flex items-center justify-between gap-3">
				<div className="text-foreground flex min-w-0 items-center gap-2 font-medium">
					<LoaderCircle className="text-primary size-3.5 shrink-0 animate-spin" />
					<span className="truncate">{progress.label}</span>
				</div>
				<span className="text-muted-foreground shrink-0 tabular-nums">
					{formatElapsed(elapsed)}
				</span>
			</div>
			<div className="mt-3 space-y-2">
				{steps.map((step) => (
					<div
						key={step.phase}
						className="text-muted-foreground flex items-center gap-2"
					>
						{step.status === "active" ? (
							<LoaderCircle className="text-primary size-3.5 shrink-0 animate-spin" />
						) : step.status === "done" ? (
							<CircleCheck className="text-primary size-3.5 shrink-0" />
						) : (
							<Circle className="text-muted-foreground/50 size-3.5 shrink-0" />
						)}
						<span className={step.status === "active" ? "text-foreground" : ""}>
							{step.label}
						</span>
						{step.status === "active" && (
							<span className="text-muted-foreground/70 ml-auto text-[10px]">
								running
							</span>
						)}
					</div>
				))}
			</div>
			<div className="text-muted-foreground/80 mt-3 flex items-center gap-2 border-t pt-2 text-[10px]">
				<span>Pinned revision r{progress.revision}</span>
				<span>·</span>
				<span>
					{progress.provider === "codex"
						? "Codex app-server"
						: progress.provider}
				</span>
				<span>·</span>
				<span>live events</span>
			</div>
			{elapsed >= 15_000 && (
				<div className="text-caution mt-2 flex items-start gap-1.5 text-[10px] leading-relaxed">
					<Clock3 className="mt-0.5 size-3 shrink-0" />
					<span>
						Still working. Codex may be warming up or waiting on the local
						app-server; the pinned revision is safe and no edit has been applied
						yet.
					</span>
				</div>
			)}
		</div>
	);
}

function AgentWorkflowProgressPanel({
	progress,
}: {
	progress: WorkflowProgress;
}) {
	const completed =
		progress.status === "completed"
			? progress.callCount
			: Math.min(progress.callIndex, progress.callCount);
	const isRunning = progress.status === "running";
	return (
		<div className="border-border bg-muted/30 mt-2 rounded-xl border p-3 text-xs">
			<div className="flex items-center gap-2 font-medium">
				{isRunning ? (
					<LoaderCircle className="text-primary size-3.5 shrink-0 animate-spin" />
				) : progress.status === "completed" ? (
					<CircleCheck className="text-primary size-3.5 shrink-0" />
				) : (
					<CircleOff className="text-destructive size-3.5 shrink-0" />
				)}
				<span className="truncate">{progress.label}</span>
			</div>
			<div className="text-muted-foreground mt-2 flex items-center justify-between gap-3">
				<span className="truncate">{progress.tool}</span>
				<span className="shrink-0 tabular-nums">
					{progress.status === "completed"
						? `${progress.callCount} steps complete`
						: `Step ${Math.min(progress.callIndex + 1, progress.callCount)} of ${progress.callCount}`}
				</span>
			</div>
			<div
				className="bg-border mt-2 h-1 overflow-hidden rounded-full"
				role="progressbar"
				aria-valuemin={0}
				aria-valuemax={progress.callCount}
				aria-valuenow={completed}
			>
				<div
					className="bg-primary h-full transition-[width]"
					style={{
						width: `${progress.callCount > 0 ? (completed / progress.callCount) * 100 : 0}%`,
					}}
				/>
			</div>
			<p className="text-muted-foreground/80 mt-2 text-[10px]">
				{progress.status === "failed"
					? progress.error ?? "The workflow failed before all steps completed."
					: progress.status === "completed"
						? "Workflow complete. Jobs and generated assets remain durable."
						: "The approved workflow is running in the daemon. You can close this browser; jobs and generated assets remain durable."}
			</p>
			{progress.jobIds.length > 0 && (
				<div className="mt-3 space-y-2 border-t pt-2">
					{progress.jobIds.map((jobId) => {
						const job = progress.jobs.find((candidate) => candidate.id === jobId);
						const value = job?.state === "succeeded" ? 1 : (job?.progress ?? 0);
						return (
							<div key={jobId} className="space-y-1">
								<div className="text-muted-foreground flex items-center gap-2 text-[10px]">
									<span className="text-foreground min-w-0 flex-1 truncate">
										{job?.kind ?? jobId}
									</span>
									<span className="shrink-0 capitalize">
										{job?.state ?? "queued"} · {Math.round(value * 100)}%
									</span>
								</div>
								<div className="bg-border h-1 overflow-hidden rounded-full">
									<div
										className="bg-primary h-full transition-[width]"
										style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }}
									/>
								</div>
								{job?.message && (
									<p className="text-muted-foreground/80 truncate text-[10px]">
										{job.message}
									</p>
								)}
							</div>
						);
					})}
				</div>
			)}
		</div>
	);
}

function now(): string {
	return new Date().toISOString();
}

function getErrorMessage({ error }: { error: unknown }): string {
	if (
		error instanceof LocalCoreError &&
		(error.code === "CAPABILITY_UNAVAILABLE" ||
			error.code === "capability_not_available")
	) {
		return "The local Codex capability is unavailable. Run `codex login`, then restart openchatcutd.";
	}
	return error instanceof Error
		? error.message
		: "The local Agent request failed";
}

function currentRevisionFromDetails(details: unknown): number | null {
	if (!details || typeof details !== "object") return null;
	const value = (details as { currentRevision?: unknown }).currentRevision;
	return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
		? value
		: null;
}

function optimisticMessage({
	id,
	role,
	text,
	status,
}: {
	id: string;
	role: AgentSessionMessage["role"];
	text: string;
	status: AgentSessionMessage["status"];
}): ViewMessage {
	const timestamp = now();
	return {
		id,
		role,
		status,
		text,
		createdAt: timestamp,
		updatedAt: timestamp,
	};
}

function messagesWithRetryActions(
	messages: AgentSessionMessage[],
): ViewMessage[] {
	let latestInstruction: string | undefined;
	return messages.map((message) => {
		if (message.role === "user") latestInstruction = message.text;
		return message.status === "failed" && latestInstruction
			? { ...message, retryInstruction: latestInstruction }
			: message;
	});
}

export function AgentView() {
	const editor = useEditor();
	const activeProject = useEditor((editor) => editor.project.getActive());
	const projectId = activeProject?.metadata.id;
	const [status, setStatus] = useState<LocalCoreStatus | null>(null);
	const [statusError, setStatusError] = useState<string | null>(null);
	const [sessions, setSessions] = useState<AgentSessionSummary[]>([]);
	const [selectedSessionId, setSelectedSessionId] = useState<string | null>(
		null,
	);
	const [messages, setMessages] = useState<ViewMessage[]>([]);
	const [prompt, setPrompt] = useState("");
	const [provider, setProvider] = useState("codex");
	const [externalConfirmed, setExternalConfirmed] = useState(false);
	const [currentRevision, setCurrentRevision] = useState<number | null>(null);
	const [autoApply, setAutoApply] = useState(false);
	const [isUpdatingAutoApply, setIsUpdatingAutoApply] = useState(false);
	const [isLoadingSessions, setIsLoadingSessions] = useState(false);
	const [isPlanning, setIsPlanning] = useState(false);
	const [composerMenuOpen, setComposerMenuOpen] = useState(false);
	const [approvalNoticeOpen, setApprovalNoticeOpen] = useState(false);
	const [turnProgress, setTurnProgress] = useState<TurnProgress | null>(null);
	const [workflowProgress, setWorkflowProgress] =
		useState<WorkflowProgress | null>(null);
	const [progressNow, setProgressNow] = useState(() => Date.now());
	const [applyingProposalId, setApplyingProposalId] = useState<string | null>(
		null,
	);
	const [historyEntryId, setHistoryEntryId] = useState<string | null>(null);
	const selectedSessionIdRef = useRef<string | null>(null);
	const projectIdRef = useRef<string | undefined>(projectId);
	const conversationRef = useRef<HTMLDivElement | null>(null);
	const isProgressing = turnProgress !== null;

	useEffect(() => {
		if (!isProgressing) return;
		setProgressNow(Date.now());
		const timer = window.setInterval(() => setProgressNow(Date.now()), 1_000);
		return () => window.clearInterval(timer);
	}, [isProgressing]);

	useEffect(() => {
		selectedSessionIdRef.current = selectedSessionId;
	}, [selectedSessionId]);
	useEffect(() => {
		projectIdRef.current = projectId;
	}, [projectId]);

	const refreshStatus = useCallback(async () => {
		try {
			const next = await localCoreClient.getStatus();
			setStatus(next);
			setStatusError(null);
		} catch (error) {
			setStatus(null);
			setStatusError(getErrorMessage({ error }));
		}
	}, []);

	const loadSession = useCallback(
		async ({ sessionId }: { sessionId: string }) => {
			const session = await localCoreClient.readAgentSession({ sessionId });
			if (selectedSessionIdRef.current === sessionId) {
				setMessages(messagesWithRetryActions(session.messages));
				setProvider(session.provider);
				const persistedWorkflow = [...session.messages]
					.reverse()
					.find((message) => message.workflow)?.workflow as
					PersistedWorkflow | undefined;
				if (persistedWorkflow) {
					const jobs = (
						await Promise.all(
							persistedWorkflow.jobIds.map((jobId) =>
								localCoreClient.readJob({ jobId }).catch(() => null),
							),
						)
					).filter((job): job is JobRecord => Boolean(job));
					if (selectedSessionIdRef.current === sessionId) {
						const restored: WorkflowProgress = {
							proposalId: persistedWorkflow.proposalId,
							callIndex: persistedWorkflow.jobIds.length,
							callCount: Math.max(1, persistedWorkflow.jobIds.length),
							tool: "approved creative workflow",
							label:
								persistedWorkflow.jobIds.length > 0
									? "Resuming persistent workflow jobs"
									: "Workflow completed",
							status: persistedWorkflow.jobIds.length > 0 ? "running" : "completed",
							jobIds: persistedWorkflow.jobIds,
							jobs,
						};
						setWorkflowProgress(workflowWithJobs({ progress: restored, jobs }));
					}
				} else {
					setWorkflowProgress(null);
				}
				const running = session.messages.some(
					(message) =>
						message.role === "agent" && message.status === "streaming",
				);
				setIsPlanning(running);
				if (!running) setTurnProgress(null);
			}
			return session;
		},
		[],
	);

	const reconcileAcceptedTurn = useCallback(
		async ({
			sessionId,
			messageId,
		}: {
			sessionId: string;
			messageId: string;
		}) => {
			// WebSocket events are the low-latency path, but a very fast local reply
			// can finish between session creation and the event listener observing the
			// selected session. Re-read the durable conversation until that exact turn
			// reaches a terminal state so the composer never stays stuck on Planning.
			const deadline = Date.now() + 270_000;
			let delay = 200;
			let lastError: unknown = null;
			while (Date.now() < deadline) {
				if (selectedSessionIdRef.current !== sessionId) return;
				try {
					const session = await loadSession({ sessionId });
					const message = session.messages.find(
						(candidate) => candidate.id === messageId,
					);
					if (message && message.status !== "streaming") return;
					lastError = null;
				} catch (error) {
					lastError = error;
				}
				await new Promise<void>((resolve) => window.setTimeout(resolve, delay));
				delay = Math.min(2_000, Math.round(delay * 1.5));
			}

			if (selectedSessionIdRef.current !== sessionId) return;
			setIsPlanning(false);
			setTurnProgress(null);
			setStatusError(
				lastError
					? getErrorMessage({ error: lastError })
					: "The Agent turn did not reach a terminal state. Reconnect to load its durable session.",
			);
		},
		[loadSession],
	);

	const refreshSessions = useCallback(
		async ({ preferredId }: { preferredId?: string } = {}) => {
			if (!projectId) return;
			setIsLoadingSessions(true);
			try {
				const next = await localCoreClient.listAgentSessions({ projectId });
				setSessions(next);
				const nextId =
					preferredId ??
					(selectedSessionIdRef.current &&
					next.some((session) => session.id === selectedSessionIdRef.current)
						? selectedSessionIdRef.current
						: next[0]?.id);
				setSelectedSessionId(nextId ?? null);
				selectedSessionIdRef.current = nextId ?? null;
				if (nextId) await loadSession({ sessionId: nextId });
				else setMessages([]);
			} catch (error) {
				setStatusError(getErrorMessage({ error }));
			} finally {
				setIsLoadingSessions(false);
			}
		},
		[loadSession, projectId],
	);

	useEffect(() => {
		setSessions([]);
		setMessages([]);
		setSelectedSessionId(null);
		selectedSessionIdRef.current = null;
		setCurrentRevision(null);
		setAutoApply(false);
		if (!projectId) return;
		void Promise.all([
			refreshStatus(),
			refreshSessions(),
			localCoreClient
				.readProject({ projectId })
				.then((project) => setCurrentRevision(project.revision)),
			localCoreClient
				.listProjects()
				.then((projects) => {
					const project = projects.find(
						(candidate) => candidate.id === projectId,
					);
					if (project) {
						setAutoApply(project.autoApply);
						setCurrentRevision((current) => current ?? project.currentRevision);
					}
				})
				.catch((error: unknown) => setStatusError(getErrorMessage({ error }))),
		]);
	}, [projectId, refreshSessions, refreshStatus]);

	const toggleAutoApply = useCallback(async () => {
		if (!projectId || currentRevision === null || isUpdatingAutoApply) return;
		setIsUpdatingAutoApply(true);
		try {
			const project = await localCoreClient.setProjectAutoApply({
				projectId,
				expectedRevision: currentRevision,
				enabled: !autoApply,
				idempotencyKey: crypto.randomUUID(),
			});
			setAutoApply(project.autoApply);
			setCurrentRevision(project.currentRevision);
		} catch (error) {
			setStatusError(getErrorMessage({ error }));
		} finally {
			setIsUpdatingAutoApply(false);
		}
	}, [autoApply, currentRevision, isUpdatingAutoApply, projectId]);

	const updateStreamingMessage = useCallback(
		({ messageId, text }: { messageId: string; text: string }) => {
			setMessages((current) =>
				current.map((message) =>
					message.id === messageId
						? { ...message, role: "agent", status: "streaming", text }
						: message,
				),
			);
		},
		[],
	);

	const handleAgentEvent = useCallback(
		(event: LocalCoreEvent) => {
			if (event.type === "connected") {
				void refreshStatus();
				const sessionId = selectedSessionIdRef.current;
				if (sessionId) void loadSession({ sessionId });
				return;
			}
			if (event.type === "worker.capabilities.changed") {
				// Capability probing can finish after the editor has already loaded.
				// Refresh the daemon snapshot so provider pickers and availability
				// hints do not require a manual reconnect.
				void refreshStatus();
				return;
			}
			if (
				"projectId" in event &&
				event.projectId === projectIdRef.current &&
				event.type === "revision.changed"
			) {
				setCurrentRevision(event.revision);
				return;
			}
			if (
				event.type === "agent.workflow.progress" &&
				event.projectId === projectIdRef.current
			) {
				setWorkflowProgress((current) => ({
					...(current?.proposalId === event.proposalId ? current : {}),
					proposalId: event.proposalId,
					callIndex: event.callIndex,
					callCount: event.callCount,
					tool: event.tool,
					label: event.label,
					status: "running",
					jobIds: event.jobIds,
					jobs: current?.jobs ?? [],
				}));
				return;
			}
			if (
				event.type === "agent.workflow.started" &&
				event.projectId === projectIdRef.current
			) {
				setWorkflowProgress((current) => ({
					...(current?.proposalId === event.proposalId ? current : {}),
					proposalId: event.proposalId,
					callIndex: 0,
					callCount: Math.max(1, event.callCount),
					tool: "approved creative workflow",
					label: "Running the approved creative workflow",
					status: "running",
					jobIds: [],
					jobs: [],
				}));
				setCurrentRevision(event.pinnedRevision);
				return;
			}
			if (
				event.type === "agent.workflow.completed" &&
				event.projectId === projectIdRef.current
			) {
				setWorkflowProgress((current) => {
					const next: WorkflowProgress = {
						...(current?.proposalId === event.proposalId ? current : {}),
						proposalId: event.proposalId,
						callIndex: event.callCount,
						callCount: Math.max(1, event.callCount),
						tool: "approved creative workflow",
						label:
							event.jobIds.length > 0
								? "Workflow dispatched to persistent jobs"
								: "Workflow completed",
						status: event.jobIds.length > 0 ? "running" : "completed",
						jobIds: event.jobIds,
						jobs: current?.jobs ?? [],
					};
					return workflowWithJobs({ progress: next, jobs: next.jobs });
				});
				setCurrentRevision(event.pinnedRevision);
				return;
			}
			if (event.type === "job.changed") {
				setWorkflowProgress((current) => {
					if (!current?.jobIds.includes(event.job.id)) return current;
					const jobs = [
						...current.jobs.filter((job) => job.id !== event.job.id),
						event.job,
					];
					return workflowWithJobs({ progress: current, jobs });
				});
				return;
			}
			if (
				!("sessionId" in event) ||
				event.sessionId !== selectedSessionIdRef.current
			) {
				return;
			}
			if (event.type === "agent.turn.started") {
				setTurnProgress((current) => {
					const next =
						current ??
						createTurnProgress({
							messageId: event.messageId,
							provider: event.provider,
							revision: event.revision,
						});
					return {
						...next,
						messageId: event.messageId,
						provider: event.provider,
						revision: event.revision,
					};
				});
				setTurnProgress((current) =>
					advanceTurnProgress({
						current,
						event: {
							phase: event.phase,
							label: "Pinning the project context",
						},
					}),
				);
			} else if (event.type === "agent.turn.progress") {
				setTurnProgress((current) =>
					advanceTurnProgress({
						current,
						event: { phase: event.phase, label: event.label },
					}),
				);
			} else if (event.type === "agent.message.streaming") {
				updateStreamingMessage({
					messageId: event.messageId,
					text: event.text,
				});
				setTurnProgress((current) =>
					advanceTurnProgress({
						current,
						event: {
							phase: "responding",
							label: "Receiving the structured plan",
						},
					}),
				);
			} else if (event.type === "agent.plan.ready") {
				if (event.autoApplied && typeof event.revision === "number") {
					setCurrentRevision(event.revision);
					void editor.project.loadProject({ id: event.projectId });
				}
				setMessages((current) =>
					current.map((message) =>
						message.id === event.messageId
							? {
									...message,
									status: "completed",
									text: event.text,
									proposal: event.proposal,
									...(event.autoApplied && typeof event.revision === "number"
										? {
												historyAction: {
													projectId: event.projectId,
													expectedRevision: event.revision,
													action: "undo" as const,
												},
											}
										: {}),
								}
							: message,
					),
				);
				setIsPlanning(false);
				setTurnProgress(null);
			} else if (event.type === "agent.turn.failed") {
				const revision = currentRevisionFromDetails(event.details);
				if (revision !== null) setCurrentRevision(revision);
				setMessages((current) => {
					const failedIndex = current.findIndex(
						(message) => message.id === event.messageId,
					);
					const retryInstruction =
						failedIndex > 0
							? [...current.slice(0, failedIndex)]
									.reverse()
									.find((message) => message.role === "user")?.text
							: undefined;
					return current.map((message) =>
						message.id === event.messageId
							? {
									...message,
									role: "error",
									status: "failed",
									text: event.message,
									retryInstruction,
								}
							: message,
					);
				});
				setIsPlanning(false);
				setTurnProgress(null);
			}
		},
		[editor, loadSession, refreshStatus, updateStreamingMessage],
	);

	useEffect(() => {
		const dispose = localCoreClient.connectEvents({
			onEvent: handleAgentEvent,
		});
		return dispose;
	}, [handleAgentEvent]);

	useEffect(() => {
		const element = conversationRef.current;
		if (element) element.scrollTop = element.scrollHeight;
	}, [messages, turnProgress]);

	const availableProviders = useMemo(
		() =>
			status?.agentProviders?.filter((candidate) => candidate.available) ?? [],
		[status],
	);
	useEffect(() => {
		if (
			availableProviders.length > 0 &&
			!availableProviders.some((candidate) => candidate.id === provider)
		) {
			setProvider(availableProviders[0]!.id);
			setExternalConfirmed(false);
		}
	}, [availableProviders, provider]);
	const selectedProvider = availableProviders.find(
		(candidate) => candidate.id === provider,
	);

	const createSession = useCallback(async (): Promise<AgentSession> => {
		if (!projectId)
			throw new Error("Open a project before starting an Agent chat");
		const session = await localCoreClient.createAgentSession({
			projectId,
			provider,
		});
		setSessions((current) => [session, ...current]);
		setSelectedSessionId(session.id);
		selectedSessionIdRef.current = session.id;
		setMessages([]);
		return session;
	}, [projectId, provider]);

	const canSubmit = Boolean(
		projectId &&
		prompt.trim() &&
		!isPlanning &&
		status &&
		selectedProvider &&
		(provider === "codex" || externalConfirmed),
	);

	const submitInstruction = async ({
		instruction,
	}: {
		instruction: string;
	}) => {
		if (
			!instruction ||
			!projectId ||
			isPlanning ||
			!status ||
			!selectedProvider
		)
			return;
		if (provider !== "codex" && !externalConfirmed) return;
			setPrompt("");
			setIsPlanning(true);
			setWorkflowProgress(null);
		let assistantMessageId: string | null = null;
		let continuesInBackground = false;
		try {
			const sessionId = selectedSessionId ?? (await createSession()).id;
			if (editor.save.getIsDirty()) await editor.save.flush();
			const pinned = await localCoreClient.readProject({ projectId });
			setCurrentRevision(pinned.revision);
			const userMessageId = `agent-message:${crypto.randomUUID()}`;
			const nextAssistantMessageId = `agent-message:${crypto.randomUUID()}`;
			assistantMessageId = nextAssistantMessageId;
			setMessages((current) => [
				...current,
				optimisticMessage({
					id: userMessageId,
					role: "user",
					text: instruction,
					status: "completed",
				}),
				optimisticMessage({
					id: nextAssistantMessageId,
					role: "agent",
					text: "",
					status: "streaming",
				}),
			]);
			setTurnProgress(
				createTurnProgress({
					messageId: nextAssistantMessageId,
					provider,
					revision: pinned.revision,
				}),
			);
			const result = await localCoreClient.invokeTool<{
				accepted?: boolean;
				autoApplied?: boolean;
				revision?: number;
			}>({
				name: "agent_plan",
				arguments: {
					projectId,
					expectedRevision: pinned.revision,
					instruction,
					provider,
					confirmExternal: provider === "codex" ? undefined : externalConfirmed,
					mode: "dry-run",
					includeVisualContext: true,
					sessionId,
					userMessageId,
					assistantMessageId: nextAssistantMessageId,
				},
			});
			if (!result.ok) {
				throw new LocalCoreError({
					message: result.error?.message ?? "Agent planning failed",
					code: result.error?.code ?? "AGENT_PLAN_FAILED",
					status: 400,
					details: result.error?.details,
				});
			}
			if (result.data?.accepted) {
				continuesInBackground = true;
				void reconcileAcceptedTurn({
					sessionId,
					messageId: nextAssistantMessageId,
				});
				return;
			}
			if (result.data?.autoApplied) {
				const revision = result.revision ?? result.data.revision;
				if (typeof revision === "number") setCurrentRevision(revision);
				setMessages((current) => [
					...current.map((message) =>
						message.id === nextAssistantMessageId
							? {
									...message,
									status: "completed" as const,
									text:
										result.message ??
										"Auto-applied the reversible mechanical edit.",
									historyAction:
										typeof revision === "number"
											? {
													projectId,
													expectedRevision: revision,
													action: "undo" as const,
												}
											: undefined,
								}
							: message,
					),
				]);
				void refreshSessions({ preferredId: sessionId });
				return;
			}
			setMessages((current) =>
				current.map((message) =>
					message.id === nextAssistantMessageId
						? {
								...message,
								status: "completed",
								text:
									result.message ??
									result.proposal?.summary ??
									"The request completed without timeline changes.",
								proposal: result.proposal,
							}
						: message,
				),
			);
			void refreshSessions({ preferredId: sessionId });
		} catch (error) {
			const message = getErrorMessage({ error });
			const revisionConflict =
				error instanceof LocalCoreError && error.code === "revisionConflict";
			if (revisionConflict && projectId) {
				void localCoreClient
					.readProject({ projectId })
					.then((project) => setCurrentRevision(project.revision));
			}
			setMessages((current) => {
				const target = assistantMessageId
					? current.find((candidate) => candidate.id === assistantMessageId)
					: undefined;
				if (!target) {
					return [
						...current,
						optimisticMessage({
							id: `agent-error:${crypto.randomUUID()}`,
							role: "error",
							status: "failed",
							text: message,
						}),
					];
				}
				return current.map((candidate) =>
					candidate.id === target.id
						? {
								...candidate,
								role: "error",
								status: "failed",
								text: message,
								retryInstruction: instruction,
							}
						: candidate,
				);
			});
		} finally {
			if (!continuesInBackground) {
				setIsPlanning(false);
				setTurnProgress(null);
			}
		}
	};

	const handleSubmit = (event: FormEvent) => {
		event.preventDefault();
		const instruction = prompt.trim();
		if (!instruction || !canSubmit) return;
		void submitInstruction({ instruction });
	};

	const applyProposal = async ({
		proposal,
		messageId,
	}: {
		proposal: ToolProposal;
		messageId: string;
	}) => {
		setApplyingProposalId(proposal.proposalId);
		const workflow = proposal.kind === "capabilityWorkflow";
		const workflowCallCount =
			workflow &&
			proposal.payload &&
			typeof proposal.payload === "object" &&
			Array.isArray(
				(proposal.payload as { calls?: unknown[] }).calls,
			)
				? Math.max(1, (proposal.payload as { calls: unknown[] }).calls.length)
				: 1;
		try {
			if (editor.save.getIsDirty()) await editor.save.flush();
			const pinned = await localCoreClient.readProject({
				projectId: proposal.projectId,
			});
			setCurrentRevision(pinned.revision);
			if (pinned.revision !== proposal.baseRevision) {
				throw new LocalCoreError({
					message: `Project is now at revision ${pinned.revision}; plan again before applying.`,
					code: "revisionConflict",
					status: 409,
					details: {
						expectedRevision: proposal.baseRevision,
						currentRevision: pinned.revision,
					},
				});
			}
			if (workflow) {
				setWorkflowProgress({
					proposalId: proposal.proposalId,
					callIndex: 0,
					callCount: workflowCallCount,
					tool: "approved creative workflow",
					label: "Starting the approved creative workflow",
					status: "running",
					jobIds: [],
					jobs: [],
				});
			}
			const result = await localCoreClient.invokeTool<{
				jobIds?: string[];
			}>({
				name:
					proposal.applyTool ??
					(workflow ? "apply_agent_workflow" : "apply_timeline_edit"),
				arguments: {
					projectId: proposal.projectId,
					expectedRevision: proposal.baseRevision,
					proposalId: proposal.proposalId,
					confirm: true,
					agentSessionId: selectedSessionId ?? undefined,
					agentMessageId: messageId,
				},
			});
			if (!result.ok) throw new Error(result.error?.message ?? "Apply failed");
			if (workflow) {
				const jobIds = result.data?.jobIds ?? [];
				const jobs = (
					await Promise.all(
						jobIds.map((jobId) =>
							localCoreClient.readJob({ jobId }).catch(() => null),
						),
					)
				).filter((job): job is JobRecord => Boolean(job));
				setWorkflowProgress((current) => {
					const next: WorkflowProgress = {
						...(current?.proposalId === proposal.proposalId ? current : {}),
						proposalId: proposal.proposalId,
						callIndex: workflowCallCount,
						callCount: workflowCallCount,
						tool: "approved creative workflow",
						label:
							jobIds.length > 0
								? "Workflow dispatched to persistent jobs"
								: "Workflow completed",
						status: jobIds.length > 0 ? "running" : "completed",
						jobIds,
						jobs,
					};
					return workflowWithJobs({ progress: next, jobs });
				});
				setMessages((current) => [
					...current,
					optimisticMessage({
						id: `agent-workflow:${crypto.randomUUID()}`,
						role: "agent",
						status: "completed",
						text:
							jobIds.length > 0
								? `Started the approved workflow. Persistent job${jobIds.length === 1 ? "" : "s"}: ${jobIds.join(", ")}. You can close the browser and resume here later.`
								: "Completed the approved workflow. No persistent media job was required.",
					}),
				]);
				return;
			}
			const revision = result.revision;
			if (typeof revision === "number") {
				await editor.project.loadProject({ id: proposal.projectId });
				setCurrentRevision(revision);
				setMessages((current) => [
					...current.map((message) =>
						message.id === messageId
							? {
									...message,
									text: `${message.text}\n\nApplied the approved plan as revision ${revision}.`,
									historyAction: {
										projectId: proposal.projectId,
										expectedRevision: revision,
										action: "undo" as const,
									},
								}
							: message,
					),
				]);
			}
		} catch (error) {
			if (workflow) {
				setWorkflowProgress((current) =>
					current?.proposalId === proposal.proposalId
						? {
								...current,
								status: "failed",
								label: "Workflow failed",
								error: getErrorMessage({ error }),
							}
						: current,
				);
			}
			setMessages((current) => [
				...current,
				optimisticMessage({
					id: `agent-error:${crypto.randomUUID()}`,
					role: "error",
					status: "failed",
					text: getErrorMessage({ error }),
				}),
			]);
		} finally {
			setApplyingProposalId(null);
		}
	};

	const navigateHistory = async ({ message }: { message: ViewMessage }) => {
		if (!message.historyAction) return;
		setHistoryEntryId(message.id);
		try {
			// History navigation is already a daemon-side CAS operation. Do not
			// flush a browser snapshot here: after a reload, editor initialization
			// can leave a queued Classic save that would create a phantom revision
			// immediately before the undo/redo request. The authoritative revision
			// is loaded below after the navigation succeeds.
			editor.save.discardPending();
			const request = {
				projectId: message.historyAction.projectId,
				expectedRevision: message.historyAction.expectedRevision,
				idempotencyKey: crypto.randomUUID(),
				agentSessionId: selectedSessionId ?? undefined,
				agentMessageId: message.id,
			};
			const result =
				message.historyAction.action === "undo"
					? await localCoreClient.undoProject(request)
					: await localCoreClient.redoProject(request);
			await editor.project.loadProject({
				id: message.historyAction.projectId,
			});
			setCurrentRevision(result.envelope.revision);
			setMessages((current) =>
				current.map((candidate) =>
					candidate.id === message.id
						? {
								...candidate,
								text: `${message.historyAction?.action === "undo" ? "Undid" : "Redid"} the Agent edit as revision ${result.envelope.revision}.`,
								historyAction: {
									projectId: message.historyAction!.projectId,
									expectedRevision: result.envelope.revision,
									action:
										message.historyAction!.action === "undo" ? "redo" : "undo",
								},
							}
						: candidate,
				),
			);
		} catch (error) {
			setMessages((current) => [
				...current,
				optimisticMessage({
					id: `agent-error:${crypto.randomUUID()}`,
					role: "error",
					status: "failed",
					text: getErrorMessage({ error }),
				}),
			]);
		} finally {
			setHistoryEntryId(null);
		}
	};

	return (
		<PanelView
			title="Agent"
			scrollClassName="overflow-hidden pt-0"
			contentClassName="flex h-full min-h-0 flex-col px-0"
			actions={
				<div className="flex items-center gap-1">
					<Badge variant={status ? "secondary" : "outline"} className="gap-1">
						{status ? (
							<CircleCheck className="size-3" />
						) : (
							<CircleOff className="size-3" />
						)}
						{status ? "Local" : "Offline"}
					</Badge>
					<Button
						size="sm"
						variant={autoApply ? "secondary" : "ghost"}
						disabled={
							!projectId ||
							currentRevision === null ||
							isPlanning ||
							isUpdatingAutoApply
						}
						onClick={() => void toggleAutoApply()}
						title={
							autoApply
								? "Auto-Apply is on for reversible mechanical edits"
								: "Turn on Auto-Apply for reversible mechanical edits"
						}
					>
						<ShieldCheck className="size-3" />
						{autoApply ? "Auto" : "Review"}
					</Button>
					<Button
						size="icon"
						variant="ghost"
						disabled={!projectId || isPlanning}
						onClick={() => void createSession()}
						title="New Agent conversation"
					>
						<MessageSquarePlus />
						<span className="sr-only">New conversation</span>
					</Button>
					<Button
						size="icon"
						variant="ghost"
						onClick={() => {
							void refreshStatus();
							void refreshSessions();
						}}
						title="Reconnect and reload conversations"
					>
						<RefreshCw />
						<span className="sr-only">Reconnect</span>
					</Button>
				</div>
			}
		>
			<div className="border-border flex shrink-0 items-center gap-2 border-b px-3 py-2">
				<History className="text-muted-foreground size-3.5 shrink-0" />
				<Select
					value={selectedSessionId ?? undefined}
					onValueChange={(value) => {
						setSelectedSessionId(value);
						selectedSessionIdRef.current = value;
						setMessages([]);
						void loadSession({ sessionId: value });
					}}
					disabled={isPlanning || isLoadingSessions || sessions.length === 0}
				>
					<SelectTrigger className="h-8 min-w-0 flex-1 border-0 bg-transparent px-1 shadow-none">
						<SelectValue
							placeholder={
								isLoadingSessions
									? "Loading conversations…"
									: "New conversation"
							}
						/>
					</SelectTrigger>
					<SelectContent>
						{sessions.map((session) => (
							<SelectItem key={session.id} value={session.id}>
								{session.title}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</div>

			<div
				ref={conversationRef}
				className="scrollbar-hidden min-h-0 flex-1 space-y-4 overflow-y-auto px-3 py-3"
			>
				{messages.length === 0 && (
					<div className="text-muted-foreground flex min-h-56 flex-col items-center justify-center px-5 text-center">
						<div className="bg-muted mb-3 rounded-xl p-3">
							<Bot className="text-foreground size-5" />
						</div>
						<p className="text-foreground text-sm font-medium">
							Codex for this edit
						</p>
						<p className="mt-1 max-w-64 text-xs leading-relaxed">
							Ask for an edit, review the semantic diff, then approve it as one
							reversible project revision. Conversations survive browser
							reloads.
						</p>
					</div>
				)}

				{statusError && messages.length === 0 && (
					<div className="border-destructive/20 bg-destructive/5 text-destructive rounded-lg border p-3 text-xs">
						{statusError}
					</div>
				)}

				{messages.map((message) => {
					const displayText =
						message.status === "failed" && !message.text
							? (message.error?.message ?? "Agent turn failed. Please retry.")
							: message.text;
					const proposalOutdated = Boolean(
						message.proposal &&
						currentRevision !== null &&
						message.proposal.baseRevision !== currentRevision,
					);
					return (
						<div key={message.id} className="space-y-2">
							<div
								className={
									message.role === "user"
										? "bg-foreground text-background ml-8 rounded-xl px-3 py-2.5 text-sm"
										: message.role === "error" || message.status === "failed"
											? "border-destructive/20 bg-destructive/5 text-destructive rounded-lg border px-3 py-2.5 text-xs"
											: "text-foreground mr-3 px-1 text-sm leading-relaxed"
								}
							>
								{message.status === "streaming" && !message.text ? (
									turnProgress?.messageId === message.id ? (
										<AgentProgressPanel
											progress={turnProgress}
											now={progressNow}
										/>
									) : (
										<span className="text-muted-foreground flex items-center gap-2 text-xs">
											<LoaderCircle className="size-3 animate-spin" />
											Preparing a structured edit plan…
										</span>
									)
								) : (
									<p className="whitespace-pre-wrap">{displayText}</p>
								)}
								{message.status === "streaming" && message.text && (
									<span className="bg-primary ml-1 inline-block h-3 w-0.5 animate-pulse align-middle" />
								)}
							</div>
							{message.role === "agent" && message.proposal && (
								<ProposalCard
									proposal={message.proposal}
									isApplying={
										applyingProposalId === message.proposal.proposalId
									}
									disabled={proposalOutdated}
									applyLabel={
										message.proposal.kind === "capabilityWorkflow"
											? "Start workflow"
											: "Apply changes"
									}
									disabledReason={
										proposalOutdated
											? `Project is now at revision ${currentRevision}; plan again.`
											: undefined
									}
									onApply={() =>
										void applyProposal({
											proposal: message.proposal!,
											messageId: message.id,
										})
									}
								/>
							)}
							{message.historyAction && (
								<Button
									size="sm"
									variant="outline"
									disabled={historyEntryId !== null}
									onClick={() => void navigateHistory({ message })}
								>
									{historyEntryId === message.id ? (
										<LoaderCircle className="animate-spin" />
									) : message.historyAction.action === "undo" ? (
										<RotateCcw />
									) : (
										<RotateCw />
									)}
									{message.historyAction.action === "undo"
										? "Undo Agent revision"
										: "Redo Agent revision"}
								</Button>
							)}
							{message.retryInstruction && (
								<Button
									size="sm"
									variant="outline"
									disabled={isPlanning}
									onClick={() =>
										void submitInstruction({
											instruction: message.retryInstruction!,
										})
									}
								>
									<RefreshCw />
									Retry on revision r{currentRevision ?? "latest"}
								</Button>
							)}
						</div>
					);
				})}

				{turnProgress &&
					!messages.some(
						(message) =>
							message.id === turnProgress.messageId &&
							message.status === "streaming" &&
							!message.text,
					) && <AgentProgressPanel progress={turnProgress} now={progressNow} />}
				{workflowProgress && (
					<AgentWorkflowProgressPanel progress={workflowProgress} />
				)}
			</div>

			<form
				onSubmit={(event) => void handleSubmit(event)}
				className="border-border bg-background relative shrink-0 border-t px-3 py-3"
			>
				{composerMenuOpen && (
					<div className="border-border bg-popover text-popover-foreground absolute bottom-14 left-3 z-20 min-w-56 rounded-xl border p-1.5 text-xs shadow-lg">
						<button
							type="button"
							className="hover:bg-accent flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left"
							onClick={() => {
								setComposerMenuOpen(false);
								setPrompt((value) =>
									value ? `${value}\n\n` : "Use the current project context: ",
								);
							}}
						>
							<History className="text-muted-foreground size-3.5" />
							Use current project context
						</button>
						<button
							type="button"
							className="hover:bg-accent flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left"
							onClick={() => {
								setComposerMenuOpen(false);
								setPrompt((value) =>
									value
										? `${value}\n\n`
										: "Review this project before editing: ",
								);
							}}
						>
							<Bot className="text-muted-foreground size-3.5" />
							Ask for a project review
						</button>
					</div>
				)}
				{approvalNoticeOpen && (
					<div className="border-primary/20 bg-primary/5 text-muted-foreground mb-2 flex items-start gap-2 rounded-xl border px-2.5 py-2 text-[10px] leading-relaxed">
						<ShieldCheck className="text-primary mt-0.5 size-3.5 shrink-0" />
						<span>
							Codex plans stay in dry-run until you approve the diff. Every
							applied edit becomes one reversible revision.
						</span>
					</div>
				)}
				{provider !== "codex" && (
					<label className="border-border bg-muted/40 flex items-start gap-2 rounded-md border p-2 text-[10px] leading-relaxed">
						<input
							type="checkbox"
							className="mt-0.5"
							checked={externalConfirmed}
							onChange={(event) => setExternalConfirmed(event.target.checked)}
						/>
						<span>
							Send pinned project context to{" "}
							{selectedProvider?.name ?? provider}. Visual frames remain local.
						</span>
					</label>
				)}
				<div className="border-border bg-background focus-within:border-primary/40 focus-within:ring-ring rounded-[1.35rem] border p-2.5 shadow-[0_8px_24px_-18px_hsl(var(--foreground)/0.45)] focus-within:ring-1">
					<Textarea
						value={prompt}
						onChange={(event) => setPrompt(event.target.value)}
						placeholder="Remove filler words, tighten pauses, add captions…"
						className="min-h-[4.5rem] max-h-36 resize-none border-0 bg-transparent px-1 py-0.5 text-[15px] leading-relaxed shadow-none focus-visible:ring-0"
						disabled={!status || isPlanning}
						onKeyDown={(event) => {
							if (event.key === "Enter" && !event.shiftKey) {
								event.preventDefault();
								event.currentTarget.form?.requestSubmit();
							}
						}}
					/>
					<div className="flex items-center justify-between gap-2 pt-2">
						<div className="flex min-w-0 items-center gap-0.5">
							<Button
								type="button"
								size="icon"
								variant="ghost"
								className="size-8 rounded-full"
								disabled={!status || isPlanning}
								onClick={() => setComposerMenuOpen((open) => !open)}
								title="Composer actions"
								aria-label="Composer actions"
							>
								<Plus />
							</Button>
							<Button
								type="button"
								size="icon"
								variant="ghost"
								className={
									approvalNoticeOpen
										? "text-primary size-8 rounded-full"
										: "text-muted-foreground size-8 rounded-full"
								}
								onClick={() => setApprovalNoticeOpen((open) => !open)}
								title="Approval policy"
								aria-label="Approval policy"
							>
								<ShieldCheck />
							</Button>
							<span className="text-muted-foreground ml-1 hidden text-[10px] sm:inline">
								{provider === "codex" ? "Codex login" : selectedProvider?.name}{" "}
								· dry-run
							</span>
						</div>
						<div className="flex min-w-0 items-center gap-1">
							<Select
								value={provider}
								onValueChange={(value) => {
									setProvider(value);
									setExternalConfirmed(false);
								}}
								disabled={isPlanning || availableProviders.length === 0}
							>
								<SelectTrigger className="h-8 max-w-[7.5rem] min-w-0 rounded-full border-0 bg-transparent px-2 text-xs shadow-none">
									<SelectValue placeholder="Codex" />
								</SelectTrigger>
								<SelectContent>
									{availableProviders.map((candidate) => (
										<SelectItem key={candidate.id} value={candidate.id}>
											{candidate.name}
											{candidate.model ? ` · ${candidate.model}` : ""}
										</SelectItem>
									))}
								</SelectContent>
							</Select>
							<Badge
								variant="outline"
								className="h-8 shrink-0 rounded-full px-2.5 text-[10px]"
							>
								r{currentRevision ?? "–"}
							</Badge>
							<Button
								type="button"
								size="icon"
								variant="ghost"
								className="text-muted-foreground size-8 rounded-full"
								disabled
								title="Voice input is not enabled in the local editor yet"
								aria-label="Voice input unavailable"
							>
								<Mic />
							</Button>
							<Button
								type="submit"
								size="icon"
								className="size-9 rounded-full"
								disabled={!canSubmit}
								title={isPlanning ? "Planning" : "Send"}
								aria-label={isPlanning ? "Planning" : "Send"}
							>
								{isPlanning ? (
									<LoaderCircle className="animate-spin" />
								) : (
									<ArrowUp />
								)}
							</Button>
						</div>
					</div>
				</div>
			</form>
		</PanelView>
	);
}
