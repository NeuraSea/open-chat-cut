export type LocalCoreHealth = "ready" | "degraded" | "offline";

export interface LocalCoreStatus {
	status: LocalCoreHealth;
	protocolVersion: string;
	daemonVersion: string;
	dataDirectory?: string;
	capabilities: Record<string, boolean>;
	agentProviders?: AgentProviderStatus[];
	mediaWorker?: MediaWorkerStatus;
}

export interface MediaWorkerStatus {
	available: boolean;
	capabilities?: {
		ffmpegAvailable: boolean;
		videoEncoding?: {
			selected?: string;
			accelerated?: boolean;
			fallbackReason?: string;
		};
	};
}

export interface AgentProviderStatus {
	id: "codex" | "openai-compatible" | "ollama";
	name: string;
	available: boolean;
	authentication: string;
	external: boolean;
	supportsVisualContext: boolean;
	model?: string;
	baseUrl?: string;
}

export interface BrowserSession {
	csrfToken: string;
	expiresAt?: string;
}

export interface LocalProjectSummary {
	id: string;
	name: string;
	currentRevision: number;
	documentHash: string;
	autoApply: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface LocalProjectEnvelope<TDocument = unknown> {
	document: TDocument;
	revision: number;
	documentHash: string;
}

export interface NamedProjectVersion {
	id: string;
	projectId: string;
	name: string;
	revision: number;
	documentHash: string;
	createdAt: string;
}

export interface RestoreProjectVersionResponse<TDocument = unknown> {
	replayed: boolean;
	envelope: LocalProjectEnvelope<TDocument>;
	restoredVersionId: string;
}

export interface LocalEditTransaction<TOperation = unknown> {
	transactionId: string;
	projectId: string;
	baseRevision: number;
	idempotencyKey: string;
	actor: {
		kind: "user" | "agent" | "system";
		id?: string;
		displayName?: string;
	};
	operations: TOperation[];
}

export interface LocalCommitResponse<TDocument = unknown> {
	replayed: boolean;
	envelope: LocalProjectEnvelope<TDocument>;
	inverseOperations?: unknown[];
	changes?: unknown[];
}

export interface HistoryNavigationResponse<TDocument = unknown> {
	replayed: boolean;
	action: "undo" | "redo";
	sourceRevision: number;
	restoredFromRevision: number;
	canUndo: boolean;
	canRedo: boolean;
	envelope: LocalProjectEnvelope<TDocument>;
}

export interface ManagedMediaUploadResponse<
	TDocument = unknown,
	TAsset = unknown,
> {
	asset: TAsset;
	revision: number;
	replayed: boolean;
	commit: LocalCommitResponse<TDocument>;
}

export interface ToolWarning {
	code: string;
	message: string;
	severity: "info" | "warning" | "danger";
}

export interface CostEstimate {
	currency: string;
	minimum?: number;
	maximum?: number;
	display: string;
}

export interface OperationDiff {
	operationId: string;
	kind: string;
	summary: string;
	targetIds: string[];
	before?: unknown;
	after?: unknown;
}

export interface ToolProposal<T = unknown> {
	kind?: "timelineEdit" | "capabilityWorkflow" | string;
	applyTool?: "apply_timeline_edit" | "apply_agent_workflow" | string;
	proposalId: string;
	projectId: string;
	baseRevision: number;
	summary: string;
	diffs: OperationDiff[];
	dependencyImpact: string[];
	warnings: ToolWarning[];
	cost?: CostEstimate;
	payload: T;
	expiresAt?: string;
}

export interface AgentSessionSummary {
	id: string;
	projectId: string;
	title: string;
	provider: string;
	createdAt: string;
	updatedAt: string;
}

export interface AgentSessionMessage {
	id: string;
	role: "user" | "agent" | "error";
	status: "streaming" | "completed" | "failed";
	text: string;
	proposal?: ToolProposal;
	historyAction?: {
		projectId: string;
		expectedRevision: number;
		action: "undo" | "redo";
	};
	workflow?: {
		proposalId: string;
		pinnedRevision: number;
		jobIds: string[];
	};
	error?: { code?: string; message?: string };
	createdAt: string;
	updatedAt: string;
}

export interface AgentSession extends AgentSessionSummary {
	messages: AgentSessionMessage[];
}

export interface ToolResult<T = unknown> {
	ok: boolean;
	data?: T;
	message?: string;
	proposal?: ToolProposal;
	jobId?: string;
	revision?: number;
	error?: {
		code: string;
		message: string;
		details?: unknown;
	};
}

export interface TranscriptWord {
	id: string;
	spokenText: string;
	displayText: string;
	startMs: number;
	endMs: number;
	confidence?: number;
	speakerId?: string;
	deleted?: boolean;
}

export interface TranscriptUtterance {
	id: string;
	speakerId?: string;
	words: TranscriptWord[];
}

export interface TranscriptDocument {
	id: string;
	projectId: string;
	sourceAssetId: string;
	language?: string;
	utterances: TranscriptUtterance[];
	revision: number;
}

export type TranscriptCleanupSuggestionKind =
	| "filler"
	| "repeatedTake"
	| "longPause"
	| "highlight";

export interface TranscriptCleanupSuggestion {
	id: string;
	kind: TranscriptCleanupSuggestionKind;
	startTicks: number;
	endTicks: number;
	confidenceBps: number;
	reason: string;
	wordIds: string[];
	segmentIds: string[];
	action: { type: string; [key: string]: unknown };
	recommended: boolean;
	estimatedRemovedTicks: number;
}

export interface TranscriptCleanupAnalysis {
	transcriptId: string;
	options: {
		pauseThresholdTicks: number;
		targetPauseTicks: number;
		minimumApplyConfidenceBps: number;
		minimumRepeatedTakeWords: number;
		repeatedTakeSimilarityBps: number;
		highlightLimit: number;
	};
	summary: {
		fillerCount: number;
		repeatedTakeCount: number;
		longPauseCount: number;
		highlightCount: number;
		recommendedRemovedTicks: number;
	};
	suggestions: TranscriptCleanupSuggestion[];
}

export interface JobRecord {
	id: string;
	projectId?: string;
	kind: string;
	state:
		| "queued"
		| "running"
		| "waitingForProvider"
		| "paused"
		| "succeeded"
		| "failed"
		| "cancelled";
	progress: number;
	input?: Record<string, unknown>;
	output?: Record<string, unknown>;
	message?: string;
	revision?: number;
	cancelRequested?: boolean;
	createdAt: string;
	updatedAt: string;
	startedAt?: string;
	finishedAt?: string;
	error?: { code: string; message: string; retryable?: boolean };
}

export type LocalCoreEvent =
	| { type: "revision.changed"; projectId: string; revision: number }
	| { type: "job.changed"; job: JobRecord }
	| {
			type: "asset.changed";
			projectId: string;
			assetId: string;
			status: string;
	  }
	| { type: "daemon.ready"; instanceId: string }
	| {
			type: "worker.capabilities.changed";
			available: boolean;
			recovered?: boolean;
			capabilities: MediaWorkerStatus["capabilities"];
		  }
	| {
			type: "agent.turn.started";
			projectId: string;
			sessionId?: string;
			messageId: string;
			provider: string;
			revision: number;
			phase: string;
	  }
	| {
			type: "agent.turn.progress";
			projectId: string;
			sessionId?: string;
			messageId: string;
			phase: string;
			label: string;
	  }
	| {
			type: "agent.message.streaming";
			projectId: string;
			sessionId?: string;
			messageId: string;
			text: string;
	  }
	| {
			type: "agent.plan.ready";
			projectId: string;
			sessionId?: string;
			messageId: string;
			text: string;
			proposal?: ToolProposal;
			hasChanges?: boolean;
			autoApplied?: boolean;
			revision?: number;
	  }
	| {
			type: "agent.turn.failed";
			projectId: string;
			sessionId?: string;
			messageId: string;
			message: string;
			code: string;
			details?: unknown;
	  }
	| {
			type: "agent.workflow.progress";
			projectId: string;
			proposalId: string;
			callIndex: number;
			callCount: number;
			tool: string;
			label: string;
			jobIds: string[];
		  }
	| {
			type: "agent.workflow.started";
			projectId: string;
			proposalId: string;
			pinnedRevision: number;
			callCount: number;
			jobIds: string[];
		  }
	| {
			type: "agent.workflow.completed";
			projectId: string;
			proposalId: string;
			pinnedRevision: number;
			callCount: number;
			jobIds: string[];
		  }
	| { type: "stream.lagged"; skipped: number }
	| { type: "connected" }
	| { type: "disconnected" };
