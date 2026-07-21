"use client";

import { useCallback, useEffect, useState } from "react";
import { Button } from "../ui/button";
import {
	Dialog,
	DialogBody,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { ScrollArea } from "../ui/scroll-area";
import { Spinner } from "../ui/spinner";
import { Badge } from "../ui/badge";
import {
	LocalCoreError,
	localCoreClient,
	type NamedProjectVersion,
} from "@/services/local-core";
import { useEditor } from "@/editor/use-editor";
import { toast } from "sonner";

interface VersionHistoryDialogProps {
	isOpen: boolean;
	onOpenChange: (open: boolean) => void;
	projectId: string | null;
}

type Action = "load" | "create" | "restore" | null;

function formatVersionDate(value: string): string {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: "medium",
		timeStyle: "short",
	}).format(date);
}

function errorMessage(error: unknown): string {
	return error instanceof Error ? error.message : "Local daemon unavailable";
}

export function VersionHistoryDialog({
	isOpen,
	onOpenChange,
	projectId,
}: VersionHistoryDialogProps) {
	const editor = useEditor();
	const [versions, setVersions] = useState<NamedProjectVersion[]>([]);
	const [currentRevision, setCurrentRevision] = useState<number | null>(null);
	const [versionName, setVersionName] = useState("");
	const [restoreTarget, setRestoreTarget] =
		useState<NamedProjectVersion | null>(null);
	const [action, setAction] = useState<Action>(null);
	const [error, setError] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		if (!projectId) return;
		setAction("load");
		setError(null);
		try {
			const [envelope, nextVersions] = await Promise.all([
				localCoreClient.readProject({ projectId }),
				localCoreClient.listProjectVersions({ projectId }),
			]);
			setCurrentRevision(envelope.revision);
			setVersions(nextVersions);
		} catch (caught) {
			setError(errorMessage(caught));
		} finally {
			setAction(null);
		}
	}, [projectId]);

	useEffect(() => {
		if (!isOpen || !projectId) return;
		setRestoreTarget(null);
		setVersionName("");
		void refresh();
	}, [isOpen, projectId, refresh]);

	const handleCreate = async () => {
		const name = versionName.trim();
		if (!projectId || !name) return;
		setAction("create");
		setError(null);
		try {
			// A named version must include the editor's latest manual edits. Flush
			// before reading the CAS revision, then use the daemon envelope as the
			// only authority for the version request.
			if (editor.save.getIsDirty()) await editor.save.flush();
			const envelope = await localCoreClient.readProject({ projectId });
			const result = await localCoreClient.createProjectVersion({
				projectId,
				name,
				expectedRevision: envelope.revision,
				idempotencyKey: crypto.randomUUID(),
			});
			setCurrentRevision(envelope.revision);
			setVersionName("");
			toast.success(`Saved version “${result.version.name}”`, {
				description: `Revision ${result.version.revision} is now available for restore.`,
			});
			await refresh();
		} catch (caught) {
			setError(errorMessage(caught));
			toast.error("Version was not saved", { description: errorMessage(caught) });
			setAction(null);
		}
	};

	const handleRestore = async () => {
		if (!projectId || !restoreTarget) return;
		setAction("restore");
		setError(null);
		try {
			if (editor.save.getIsDirty()) await editor.save.flush();
			const current = await localCoreClient.readProject({ projectId });
			const result = await localCoreClient.restoreProjectVersion({
				projectId,
				versionId: restoreTarget.id,
				expectedRevision: current.revision,
				idempotencyKey: crypto.randomUUID(),
			});
			// Do not let an old browser snapshot race the restored daemon document.
			editor.save.discardPending();
			await editor.project.loadProject({ id: projectId });
			setCurrentRevision(result.envelope.revision);
			setRestoreTarget(null);
			toast.success(`Restored “${restoreTarget.name}”`, {
				description: `Created revision ${result.envelope.revision}; earlier revisions remain in history.`,
			});
			await refresh();
		} catch (caught) {
			setError(errorMessage(caught));
			if (
				caught instanceof LocalCoreError &&
				(caught.code === "revision_conflict" ||
					caught.code === "revisionConflict")
			) {
				toast.error("Project changed before restore", {
					description: "The version list was refreshed. Review and try again.",
				});
			} else {
				toast.error("Version was not restored", {
					description: errorMessage(caught),
				});
			}
			setAction(null);
			await refresh();
		}
	};

	return (
		<Dialog open={isOpen} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-xl">
				<DialogHeader>
					<DialogTitle>Project versions</DialogTitle>
					<DialogDescription>
						Named checkpoints are stored by the local daemon. Restoring never
						overwrites history; it creates a new revision.
					</DialogDescription>
				</DialogHeader>
				<DialogBody className="min-h-0">
					<div className="flex items-end gap-2">
						<div className="min-w-0 flex-1">
							<label
								className="text-muted-foreground mb-1 block text-xs"
								htmlFor="version-name"
							>
								Save current revision as
							</label>
							<Input
								id="version-name"
								value={versionName}
								maxLength={120}
								onChange={(event) => setVersionName(event.target.value)}
								onKeyDown={(event) => {
									if (event.key === "Enter") void handleCreate();
								}}
								placeholder="e.g. Before captions"
								disabled={action !== null}
							/>
						</div>
						<Button
							onClick={() => void handleCreate()}
							disabled={!versionName.trim() || action !== null}
						>
							{action === "create" && <Spinner />}
							Save version
						</Button>
					</div>

					<div className="flex items-center justify-between text-xs text-muted-foreground">
						<span>
							Current daemon revision: {currentRevision === null ? "—" : currentRevision}
						</span>
						<Button
							variant="link"
							size="text"
							onClick={() => void refresh()}
							disabled={action !== null}
						>
							Refresh
						</Button>
					</div>

					{error && (
						<div
							role="alert"
							className="border-destructive/30 bg-destructive/10 text-destructive rounded-md border px-3 py-2 text-sm"
						>
							{error}
						</div>
					)}

					<ScrollArea className="max-h-64 min-h-24 rounded-md border">
						{action === "load" && versions.length === 0 ? (
							<div className="text-muted-foreground flex items-center justify-center gap-2 p-8 text-sm">
								<Spinner /> Loading versions…
							</div>
						) : versions.length === 0 ? (
							<p className="text-muted-foreground p-8 text-center text-sm">
								No named versions yet. Save one before a major edit or export.
							</p>
						) : (
							<div className="divide-y">
								{versions.map((version) => (
									<div
										key={version.id}
										className="flex items-center justify-between gap-3 px-3 py-3"
									>
										<div className="min-w-0">
											<div className="flex items-center gap-2">
												<p className="truncate text-sm font-medium">{version.name}</p>
												{version.revision === currentRevision && (
													<Badge variant="secondary">current</Badge>
												)}
											</div>
											<p className="text-muted-foreground text-xs">
												Revision {version.revision} · {formatVersionDate(version.createdAt)}
											</p>
										</div>
										<Button
											variant="outline"
											size="sm"
											onClick={() => setRestoreTarget(version)}
											disabled={action !== null}
										>
											Restore
										</Button>
									</div>
								))}
							</div>
						)}
					</ScrollArea>

					{restoreTarget && (
						<div className="border-caution/40 bg-caution/10 rounded-md border p-3">
							<p className="text-sm font-medium">
								Restore “{restoreTarget.name}” from revision {restoreTarget.revision}?
							</p>
							<p className="text-muted-foreground mt-1 text-xs">
								The current document will become a new revision. This does not
								delete newer history, but unsaved browser edits will be saved first.
							</p>
							<div className="mt-3 flex justify-end gap-2">
								<Button
									variant="ghost"
									onClick={() => setRestoreTarget(null)}
									disabled={action === "restore"}
								>
									Cancel
								</Button>
								<Button
									variant="caution"
									onClick={() => void handleRestore()}
									disabled={action !== null}
								>
									{action === "restore" && <Spinner />}
									Restore version
								</Button>
							</div>
						</div>
					)}
				</DialogBody>
				<DialogFooter>
					<Button variant="outline" onClick={() => onOpenChange(false)}>
						Done
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
