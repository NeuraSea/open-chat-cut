import type {
	AgentSession,
	AgentSessionSummary,
	BrowserSession,
	LocalCommitResponse,
	LocalCoreEvent,
	LocalCoreStatus,
	LocalEditTransaction,
	HistoryNavigationResponse,
	JobRecord,
	LocalProjectEnvelope,
	LocalProjectSummary,
	ManagedMediaUploadResponse,
	NamedProjectVersion,
	RestoreProjectVersionResponse,
	ToolResult,
} from "./types";

const DEFAULT_API_BASE_URL = "http://127.0.0.1:3210/api/v1";
const SESSION_STORAGE_KEY = "openchatcut.browser-session";

export class LocalCoreError extends Error {
	readonly code: string;
	readonly status: number;
	readonly details?: unknown;

	constructor({
		message,
		code,
		status,
		details,
	}: {
		message: string;
		code: string;
		status: number;
		details?: unknown;
	}) {
		super(message);
		this.name = "LocalCoreError";
		this.code = code;
		this.status = status;
		this.details = details;
	}
}

function normalizeBaseUrl({ value }: { value: string }): string {
	return value.replace(/\/+$/, "");
}

export function resolveLocalCoreBaseUrl({
	value,
	browserOrigin,
}: {
	value: string;
	browserOrigin?: string;
}): string {
	if (value === "same-origin") {
		return browserOrigin
			? `${browserOrigin.replace(/\/+$/, "")}/api/v1`
			: DEFAULT_API_BASE_URL;
	}
	if (!browserOrigin) return value;
	try {
		const url = new URL(value);
		const editorHost = new URL(browserOrigin).hostname;
		const configuredIsDefaultLoopback =
			url.port === "3210" &&
			(url.hostname === "127.0.0.1" || url.hostname === "localhost");
		const editorIsLoopback =
			editorHost === "127.0.0.1" || editorHost === "localhost";
		if (configuredIsDefaultLoopback && editorIsLoopback) {
			url.hostname = editorHost;
			return url.toString();
		}
	} catch {
		// Invalid URLs are reported by fetch with their original value.
	}
	return value;
}

function browserAwareBaseUrl({ value }: { value: string }): string {
	return resolveLocalCoreBaseUrl({
		value,
		browserOrigin:
			typeof window === "undefined" ? undefined : window.location.origin,
	});
}

function readStoredSession(): BrowserSession | null {
	if (typeof window === "undefined") return null;
	const value = window.sessionStorage.getItem(SESSION_STORAGE_KEY);
	if (!value) return null;
	try {
		const parsed = JSON.parse(value) as Partial<BrowserSession>;
		if (typeof parsed.csrfToken !== "string") return null;
		if (
			parsed.expiresAt &&
			new Date(parsed.expiresAt).getTime() <= Date.now()
		) {
			window.sessionStorage.removeItem(SESSION_STORAGE_KEY);
			return null;
		}
		return { csrfToken: parsed.csrfToken, expiresAt: parsed.expiresAt };
	} catch {
		return null;
	}
}

function storeSession({ session }: { session: BrowserSession }): void {
	if (typeof window === "undefined") return;
	window.sessionStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
}

function clearStoredSession(): void {
	if (typeof window === "undefined") return;
	window.sessionStorage.removeItem(SESSION_STORAGE_KEY);
}

export class LocalCoreClient {
	readonly baseUrl: string;
	private session: BrowserSession | null = null;
	private bootstrapPromise: Promise<BrowserSession> | null = null;

	constructor({ baseUrl }: { baseUrl?: string } = {}) {
		this.baseUrl = normalizeBaseUrl({
			value: browserAwareBaseUrl({
				value:
					baseUrl ??
					process.env.NEXT_PUBLIC_OPENCHATCUT_API_URL ??
					DEFAULT_API_BASE_URL,
			}),
		});
	}

	async getStatus(): Promise<LocalCoreStatus> {
		return this.request<LocalCoreStatus>({ path: "/status", method: "GET" });
	}

	async listProjects(): Promise<LocalProjectSummary[]> {
		const response = await this.request<{ projects: LocalProjectSummary[] }>({
			path: "/projects",
			method: "GET",
		});
		return response.projects;
	}

	async setProjectAutoApply({
		projectId,
		expectedRevision,
		enabled,
		idempotencyKey,
	}: {
		projectId: string;
		expectedRevision: number;
		enabled: boolean;
		idempotencyKey: string;
	}): Promise<LocalProjectSummary> {
		const response = await this.request<{ project: LocalProjectSummary }>({
			path: `/projects/${encodeURIComponent(projectId)}/settings/auto-apply`,
			method: "POST",
			headers: { "Idempotency-Key": idempotencyKey },
			body: { expectedRevision, enabled, idempotencyKey },
		});
		return response.project;
	}

	async createProject<TDocument>({
		name,
		projectId,
		idempotencyKey,
	}: {
		name: string;
		projectId: string;
		idempotencyKey: string;
	}): Promise<LocalCommitResponse<TDocument>> {
		return this.request<LocalCommitResponse<TDocument>>({
			path: "/projects",
			method: "POST",
			headers: { "Idempotency-Key": idempotencyKey },
			body: { name, projectId, idempotencyKey },
		});
	}

	async readProject<TDocument>({
		projectId,
	}: {
		projectId: string;
	}): Promise<LocalProjectEnvelope<TDocument>> {
		const response = await this.request<{
			envelope: LocalProjectEnvelope<TDocument>;
		}>({
			path: `/projects/${encodeURIComponent(projectId)}`,
			method: "GET",
		});
		return response.envelope;
	}

	async readProjectRevision<TDocument>({
		projectId,
		revision,
	}: {
		projectId: string;
		revision: number;
	}): Promise<LocalProjectEnvelope<TDocument>> {
		if (!Number.isSafeInteger(revision) || revision < 0) {
			throw new TypeError("revision must be a non-negative safe integer");
		}
		const response = await this.request<{
			envelope: LocalProjectEnvelope<TDocument>;
		}>({
			path: `/projects/${encodeURIComponent(projectId)}/revisions/${revision}`,
			method: "GET",
		});
		return response.envelope;
	}

	async listProjectVersions({
		projectId,
	}: {
		projectId: string;
	}): Promise<NamedProjectVersion[]> {
		const response = await this.request<{
			versions: NamedProjectVersion[];
		}>({
			path: `/projects/${encodeURIComponent(projectId)}/versions`,
			method: "GET",
		});
		return response.versions;
	}

	async createProjectVersion({
		projectId,
		name,
		expectedRevision,
		idempotencyKey,
	}: {
		projectId: string;
		name: string;
		expectedRevision: number;
		idempotencyKey: string;
	}): Promise<{ replayed: boolean; version: NamedProjectVersion }> {
		if (!Number.isSafeInteger(expectedRevision) || expectedRevision < 0) {
			throw new TypeError(
				"expectedRevision must be a non-negative safe integer",
			);
		}
		return this.request({
			path: `/projects/${encodeURIComponent(projectId)}/versions`,
			method: "POST",
			headers: { "Idempotency-Key": idempotencyKey },
			body: { name, expectedRevision, idempotencyKey },
		});
	}

	async restoreProjectVersion<TDocument>({
		projectId,
		versionId,
		expectedRevision,
		idempotencyKey,
	}: {
		projectId: string;
		versionId: string;
		expectedRevision: number;
		idempotencyKey: string;
	}): Promise<RestoreProjectVersionResponse<TDocument>> {
		if (!Number.isSafeInteger(expectedRevision) || expectedRevision < 0) {
			throw new TypeError(
				"expectedRevision must be a non-negative safe integer",
			);
		}
		return this.request<RestoreProjectVersionResponse<TDocument>>({
			path: `/projects/${encodeURIComponent(projectId)}/restore`,
			method: "POST",
			headers: { "Idempotency-Key": idempotencyKey },
			body: { versionId, expectedRevision, idempotencyKey },
		});
	}

	async listAgentSessions({
		projectId,
	}: {
		projectId: string;
	}): Promise<AgentSessionSummary[]> {
		const response = await this.request<{ sessions: AgentSessionSummary[] }>({
			path: `/projects/${encodeURIComponent(projectId)}/agent/sessions`,
			method: "GET",
		});
		return response.sessions;
	}

	async createAgentSession({
		projectId,
		provider,
	}: {
		projectId: string;
		provider: string;
	}): Promise<AgentSession> {
		const response = await this.request<{ session: AgentSession }>({
			path: `/projects/${encodeURIComponent(projectId)}/agent/sessions`,
			method: "POST",
			body: { provider },
		});
		return response.session;
	}

	async readAgentSession({
		sessionId,
	}: {
		sessionId: string;
	}): Promise<AgentSession> {
		const response = await this.request<{ session: AgentSession }>({
			path: `/agent/sessions/${encodeURIComponent(sessionId)}`,
			method: "GET",
		});
		return response.session;
	}

	async commitTransaction<TDocument, TOperation>({
		projectId,
		transaction,
	}: {
		projectId: string;
		transaction: LocalEditTransaction<TOperation>;
	}): Promise<LocalCommitResponse<TDocument>> {
		return this.request<LocalCommitResponse<TDocument>>({
			path: `/projects/${encodeURIComponent(projectId)}/transactions`,
			method: "POST",
			body: transaction,
		});
	}

	async undoProject<TDocument>({
		projectId,
		expectedRevision,
		idempotencyKey,
		agentSessionId,
		agentMessageId,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
		agentSessionId?: string;
		agentMessageId?: string;
	}): Promise<HistoryNavigationResponse<TDocument>> {
		return this.navigateProjectHistory<TDocument>({
			projectId,
			expectedRevision,
			idempotencyKey,
			agentSessionId,
			agentMessageId,
			action: "undo",
		});
	}

	async redoProject<TDocument>({
		projectId,
		expectedRevision,
		idempotencyKey,
		agentSessionId,
		agentMessageId,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
		agentSessionId?: string;
		agentMessageId?: string;
	}): Promise<HistoryNavigationResponse<TDocument>> {
		return this.navigateProjectHistory<TDocument>({
			projectId,
			expectedRevision,
			idempotencyKey,
			agentSessionId,
			agentMessageId,
			action: "redo",
		});
	}

	private async navigateProjectHistory<TDocument>({
		projectId,
		expectedRevision,
		idempotencyKey,
		agentSessionId,
		agentMessageId,
		action,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
		agentSessionId?: string;
		agentMessageId?: string;
		action: "undo" | "redo";
	}): Promise<HistoryNavigationResponse<TDocument>> {
		if (!Number.isSafeInteger(expectedRevision) || expectedRevision < 0) {
			throw new TypeError(
				"expectedRevision must be a non-negative safe integer",
			);
		}
		return this.request<HistoryNavigationResponse<TDocument>>({
			path: `/projects/${encodeURIComponent(projectId)}/${action}`,
			method: "POST",
			headers: { "Idempotency-Key": idempotencyKey },
			body: {
				expectedRevision,
				idempotencyKey,
				...(agentSessionId && agentMessageId
					? { agentSessionId, agentMessageId }
					: {}),
			},
		});
	}

	async uploadManagedMedia<TDocument, TAsset>({
		projectId,
		expectedRevision,
		idempotencyKey,
		assetId,
		file,
		durationTicks,
		width,
		height,
		hasAudio,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
		assetId: string;
		file: File;
		durationTicks?: number;
		width?: number;
		height?: number;
		hasAudio?: boolean;
	}): Promise<ManagedMediaUploadResponse<TDocument, TAsset>> {
		const query = new URLSearchParams({ assetId, name: file.name });
		if (durationTicks !== undefined)
			query.set("durationTicks", String(durationTicks));
		if (width !== undefined) query.set("width", String(width));
		if (height !== undefined) query.set("height", String(height));
		if (hasAudio !== undefined) query.set("hasAudio", String(hasAudio));
		query.set("lastModified", String(file.lastModified));
		return this.requestBinaryJson<
			ManagedMediaUploadResponse<TDocument, TAsset>
		>({
			path: `/projects/${encodeURIComponent(projectId)}/media?${query.toString()}`,
			method: "POST",
			body: file,
			headers: {
				"Content-Type": file.type || "application/octet-stream",
				"Idempotency-Key": idempotencyKey,
				"X-OpenChatCut-Expected-Revision": String(expectedRevision),
			},
		});
	}

	async downloadManagedMedia({
		projectId,
		assetId,
	}: {
		projectId: string;
		assetId: string;
	}): Promise<Blob> {
		return this.requestBlob({
			path: `/projects/${encodeURIComponent(projectId)}/assets/${encodeURIComponent(assetId)}/content`,
		});
	}

	async downloadMediaDerivative({
		projectId,
		assetId,
		kind,
	}: {
		projectId: string;
		assetId: string;
		kind: "thumbnail" | "contactSheet" | "waveform" | "proxy" | "audio";
	}): Promise<Blob> {
		return this.requestBlob({
			path: `/projects/${encodeURIComponent(projectId)}/assets/${encodeURIComponent(assetId)}/derivatives/${encodeURIComponent(kind)}`,
		});
	}

	async deleteProject({
		projectId,
		expectedRevision,
		idempotencyKey,
	}: {
		projectId: string;
		expectedRevision: number;
		idempotencyKey: string;
	}): Promise<{
		replayed: boolean;
		projectId: string;
		deletedRevision: number;
	}> {
		return this.request({
			path: `/projects/${encodeURIComponent(projectId)}`,
			method: "DELETE",
			headers: { "Idempotency-Key": idempotencyKey },
			body: { expectedRevision, idempotencyKey },
		});
	}

	async bootstrap(): Promise<BrowserSession> {
		if (this.session) return this.session;
		this.session = readStoredSession();
		if (this.session) return this.session;
		if (this.bootstrapPromise) return this.bootstrapPromise;

		this.bootstrapPromise = this.request<BrowserSession>({
			path: "/session/bootstrap",
			method: "POST",
			skipBootstrap: true,
		}).then((session) => {
			this.session = session;
			storeSession({ session });
			return session;
		});

		try {
			return await this.bootstrapPromise;
		} finally {
			this.bootstrapPromise = null;
		}
	}

	async invokeTool<T>({
		name,
		arguments: args,
		idempotencyKey,
	}: {
		name: string;
		arguments?: Record<string, unknown>;
		idempotencyKey?: string;
	}): Promise<ToolResult<T>> {
		return this.request<ToolResult<T>>({
			path: `/tools/${encodeURIComponent(name)}`,
			method: "POST",
			body: {
				arguments: args ?? {},
				idempotencyKey: idempotencyKey ?? crypto.randomUUID(),
			},
		});
	}

	async listJobs({
		projectId,
		limit = 100,
	}: {
		projectId?: string;
		limit?: number;
	} = {}): Promise<JobRecord[]> {
		const query = new URLSearchParams({ limit: String(limit) });
		if (projectId) query.set("projectId", projectId);
		const response = await this.request<{ jobs: JobRecord[] }>({
			path: `/jobs?${query.toString()}`,
			method: "GET",
		});
		return response.jobs;
	}

	async readJob({ jobId }: { jobId: string }): Promise<JobRecord> {
		const response = await this.request<{ job: JobRecord }>({
			path: `/jobs/${encodeURIComponent(jobId)}`,
			method: "GET",
		});
		return response.job;
	}

	async cancelJob({ jobId }: { jobId: string }): Promise<JobRecord> {
		const response = await this.request<{ job: JobRecord }>({
			path: `/jobs/${encodeURIComponent(jobId)}/cancel`,
			method: "POST",
			body: {},
		});
		return response.job;
	}

	async getJobArtifactUrl({ jobId }: { jobId: string }): Promise<string> {
		await this.bootstrap();
		return `${this.baseUrl}/jobs/${encodeURIComponent(jobId)}/artifact`;
	}

	connectEvents({
		onEvent,
		onError,
	}: {
		onEvent: (event: LocalCoreEvent) => void;
		onError?: (error: Event) => void;
	}): () => void {
		let socket: WebSocket | null = null;
		let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
		let disposed = false;
		const connect = async () => {
			try {
				await this.bootstrap();
				if (disposed) return;
				const url = new URL(`${this.baseUrl}/events/ws`);
				url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
				socket = new WebSocket(url);
				socket.onopen = () => onEvent({ type: "connected" });
				socket.onerror = (error) => {
					onError?.(error);
					socket?.close();
				};
				socket.onclose = () => {
					socket = null;
					this.session = null;
					clearStoredSession();
					onEvent({ type: "disconnected" });
					if (!disposed) {
						reconnectTimer = setTimeout(() => void connect(), 1_000);
					}
				};
				socket.onmessage = (message) => {
					try {
						if (typeof message.data === "string") {
							onEvent(JSON.parse(message.data) as LocalCoreEvent);
						}
					} catch {
						// A malformed event cannot mutate editor state; ignore and wait for the next event.
					}
				};
			} catch {
				onEvent({ type: "disconnected" });
				if (!disposed) {
					reconnectTimer = setTimeout(() => void connect(), 1_000);
				}
			}
		};
		void connect();
		return () => {
			disposed = true;
			if (reconnectTimer) clearTimeout(reconnectTimer);
			if (socket) {
				socket.onclose = null;
				socket.onerror = null;
				socket.close();
			}
		};
	}

	private async request<T>({
		path,
		method,
		body,
		headers,
		skipBootstrap = false,
		retryAuthentication = true,
	}: {
		path: string;
		method: "GET" | "POST" | "PUT" | "DELETE";
		body?: unknown;
		headers?: Record<string, string>;
		skipBootstrap?: boolean;
		retryAuthentication?: boolean;
	}): Promise<T> {
		const isMutation = method !== "GET";
		if (!skipBootstrap) await this.bootstrap();

		const response = await fetch(`${this.baseUrl}${path}`, {
			method,
			credentials: "include",
			headers: {
				Accept: "application/json",
				...(body === undefined ? {} : { "Content-Type": "application/json" }),
				...headers,
				...(isMutation && this.session
					? { "X-OpenChatCut-CSRF": this.session.csrfToken }
					: {}),
			},
			body: body === undefined ? undefined : JSON.stringify(body),
		});

		const payload = (await response.json().catch(() => null)) as {
			error?: { code?: string; message?: string; details?: unknown };
		} | null;
		if (response.status === 401 && !skipBootstrap && retryAuthentication) {
			this.session = null;
			clearStoredSession();
			await this.bootstrap();
			return this.request<T>({
				path,
				method,
				body,
				headers,
				retryAuthentication: false,
			});
		}
		if (!response.ok) {
			throw new LocalCoreError({
				message:
					payload?.error?.message ??
					`Local core request failed (${response.status})`,
				code: payload?.error?.code ?? "LOCAL_CORE_REQUEST_FAILED",
				status: response.status,
				details: payload?.error?.details,
			});
		}

		return payload as T;
	}

	private async requestBinaryJson<T>({
		path,
		method,
		body,
		headers,
		retryAuthentication = true,
	}: {
		path: string;
		method: "POST";
		body: Blob;
		headers: Record<string, string>;
		retryAuthentication?: boolean;
	}): Promise<T> {
		await this.bootstrap();
		const response = await fetch(`${this.baseUrl}${path}`, {
			method,
			credentials: "include",
			headers: {
				Accept: "application/json",
				...headers,
				...(this.session
					? { "X-OpenChatCut-CSRF": this.session.csrfToken }
					: {}),
			},
			body,
		});
		if (response.status === 401 && retryAuthentication) {
			this.session = null;
			clearStoredSession();
			await this.bootstrap();
			return this.requestBinaryJson<T>({
				path,
				method,
				body,
				headers,
				retryAuthentication: false,
			});
		}
		const payload = await response.json().catch(() => null);
		if (!response.ok) throw localCoreResponseError({ response, payload });
		return payload as T;
	}

	private async requestBlob({
		path,
		retryAuthentication = true,
	}: {
		path: string;
		retryAuthentication?: boolean;
	}): Promise<Blob> {
		await this.bootstrap();
		const response = await fetch(`${this.baseUrl}${path}`, {
			method: "GET",
			credentials: "include",
			headers: { Accept: "application/octet-stream" },
		});
		if (response.status === 401 && retryAuthentication) {
			this.session = null;
			clearStoredSession();
			await this.bootstrap();
			return this.requestBlob({ path, retryAuthentication: false });
		}
		if (!response.ok) {
			const payload = await response.json().catch(() => null);
			throw localCoreResponseError({ response, payload });
		}
		return response.blob();
	}
}

function localCoreResponseError({
	response,
	payload,
}: {
	response: Response;
	payload: unknown;
}): LocalCoreError {
	const error =
		typeof payload === "object" && payload !== null && "error" in payload
			? payload.error
			: null;
	const fields = typeof error === "object" && error !== null ? error : null;
	return new LocalCoreError({
		message:
			fields && "message" in fields && typeof fields.message === "string"
				? fields.message
				: `Local core request failed (${response.status})`,
		code:
			fields && "code" in fields && typeof fields.code === "string"
				? fields.code
				: "LOCAL_CORE_REQUEST_FAILED",
		status: response.status,
		details: fields && "details" in fields ? fields.details : undefined,
	});
}

export const localCoreClient = new LocalCoreClient();
