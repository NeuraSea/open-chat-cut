import type { EditorCore } from "@/core";

type SaveManagerOptions = {
	debounceMs?: number;
};

export class SaveManager {
	private debounceMs: number;
	private isPaused = false;
	private isSaving = false;
	private savePromise: Promise<void> | null = null;
	private hasPendingSave = false;
	private saveTimer: ReturnType<typeof setTimeout> | null = null;
	private unsubscribeHandlers: Array<() => void> = [];

	constructor({
		editor,
		debounceMs = 800,
	}: {
		editor: EditorCore;
	} & SaveManagerOptions) {
		this.editor = editor;
		this.debounceMs = debounceMs;
	}

	private editor: EditorCore;

	start(): void {
		if (this.unsubscribeHandlers.length > 0) return;

		this.unsubscribeHandlers = [
			this.editor.scenes.subscribe(() => {
				this.markDirty();
			}),
			this.editor.timeline.subscribe(() => {
				this.markDirty();
			}),
		];
	}

	stop(): void {
		for (const unsubscribe of this.unsubscribeHandlers) {
			unsubscribe();
		}
		this.unsubscribeHandlers = [];
		this.clearTimer();
	}

	pause(): void {
		this.isPaused = true;
	}

	resume(): void {
		this.isPaused = false;
		if (this.hasPendingSave) {
			this.queueSave();
		}
	}

	markDirty({ force = false }: { force?: boolean } = {}): void {
		if (this.isPaused && !force) return;
		this.hasPendingSave = true;
		this.queueSave();
	}

	async flush(): Promise<void> {
		this.hasPendingSave = true;
		this.clearTimer();
		// A flush is a durability barrier, not merely a request to start saving.
		// If autosave is already in flight, wait for it and then persist any edits
		// that arrived while that snapshot was being written.
		do {
			if (!(await this.saveNow())) break;
		} while (this.hasPendingSave && !this.isPaused);
	}

	getIsDirty(): boolean {
		return this.hasPendingSave || this.isSaving;
	}

	/**
	 * Drop a queued browser snapshot before replacing the editor with an
	 * authoritative daemon revision. Loading is used after Agent undo/redo and
	 * on a fresh page boot; replaying the old local snapshot at that boundary
	 * would create a phantom revision and make the restored history action fail
	 * its CAS check.
	 */
	discardPending(): void {
		if (this.isSaving) return;
		this.hasPendingSave = false;
		this.clearTimer();
	}

	private queueSave(): void {
		if (this.isPaused) return;
		if (this.isSaving) return;
		if (this.saveTimer) {
			clearTimeout(this.saveTimer);
		}
		this.saveTimer = setTimeout(() => {
			void this.saveNow().catch((error: unknown) => {
				console.error("Autosave failed:", error);
			});
		}, this.debounceMs);
	}

	private async saveNow(): Promise<boolean> {
		if (this.savePromise) {
			await this.savePromise;
			return true;
		}
		if (!this.hasPendingSave) return false;

		const activeProject = this.editor.project.getActive();
		if (!activeProject) return false;
		if (this.editor.project.getIsLoading()) return false;
		if (this.editor.project.getMigrationState().isMigrating) return false;

		const save = async () => {
			this.isSaving = true;
			this.hasPendingSave = false;
			this.clearTimer();

			try {
				await this.editor.project.saveCurrentProject();
			} catch (error) {
				this.hasPendingSave = true;
				throw error;
			} finally {
				this.isSaving = false;
				this.savePromise = null;
				if (this.hasPendingSave && !this.isPaused) {
					this.queueSave();
				}
			}
		};
		this.savePromise = save();
		await this.savePromise;
		return true;
	}

	private clearTimer(): void {
		if (!this.saveTimer) return;
		clearTimeout(this.saveTimer);
		this.saveTimer = null;
	}
}
