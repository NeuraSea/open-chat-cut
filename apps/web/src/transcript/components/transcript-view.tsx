"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
	AudioLines,
	Clock3,
	LoaderCircle,
	RefreshCw,
	Repeat2,
	Scissors,
	Sparkles,
} from "lucide-react";
import { ProposalCard } from "@/agent/components/proposal-card";
import { PanelView } from "@/components/editor/panels/assets/views/base-panel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { useEditor } from "@/editor/use-editor";
import {
	localCoreClient,
	type ToolProposal,
	type TranscriptCleanupAnalysis,
	type TranscriptCleanupSuggestionKind,
	type TranscriptDocument,
} from "@/services/local-core";
import type { DomainProjectDocument } from "@/services/local-core/project-adapter";
import { cn } from "@/utils/ui";

type ScriptEditKind =
	| "delete_words"
	| "delete_utterances"
	| "split_at_word"
	| "reorder_words"
	| "close_gaps"
	| "change_speaker"
	| "correct_display_text";

const EDIT_LABELS: Record<ScriptEditKind, string> = {
	delete_words: "Delete selected words",
	delete_utterances: "Delete selected paragraphs",
	split_at_word: "Split clip at first word",
	reorder_words: "Move selected paragraphs first",
	close_gaps: "Close gaps over 1.5s",
	change_speaker: "Change speaker",
	correct_display_text: "Correct display text",
};

const CLEANUP_LABELS: Record<TranscriptCleanupSuggestionKind, string> = {
	filler: "Filler",
	repeatedTake: "Repeated take",
	longPause: "Long pause",
	highlight: "Highlight",
};

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isTranscriptDocument(value: unknown): value is TranscriptDocument {
	return (
		isRecord(value) &&
		typeof value.id === "string" &&
		typeof value.projectId === "string" &&
		typeof value.sourceAssetId === "string" &&
		Array.isArray(value.utterances) &&
		typeof value.revision === "number"
	);
}

function isCleanupSuggestionKind(value: unknown): value is TranscriptCleanupSuggestionKind {
	return (
		value === "filler" ||
		value === "repeatedTake" ||
		value === "longPause" ||
		value === "highlight"
	);
}

function isCleanupAnalysis(value: unknown): value is TranscriptCleanupAnalysis {
	return (
		isRecord(value) &&
		typeof value.transcriptId === "string" &&
		isRecord(value.options) &&
		isRecord(value.summary) &&
		Array.isArray(value.suggestions) &&
		value.suggestions.every(
			(suggestion) =>
				isRecord(suggestion) &&
				isCleanupSuggestionKind(suggestion.kind) &&
				typeof suggestion.id === "string" &&
				typeof suggestion.reason === "string" &&
				typeof suggestion.confidenceBps === "number" &&
				typeof suggestion.recommended === "boolean" &&
				Array.isArray(suggestion.wordIds),
		)
	);
}

function isScriptEditKind(value: string): value is ScriptEditKind {
	return Object.hasOwn(EDIT_LABELS, value);
}

function unwrapScriptResult({ value }: { value: unknown }): {
	transcript: TranscriptDocument | null;
	cleanupAnalysis: TranscriptCleanupAnalysis | null;
} | null {
	if (!isRecord(value)) return null;
	const transcript = value.transcript ?? value;
	const cleanupAnalysis = value.cleanupAnalysis;
	return {
		transcript: isTranscriptDocument(transcript) ? transcript : null,
		cleanupAnalysis: isCleanupAnalysis(cleanupAnalysis) ? cleanupAnalysis : null,
	};
}

function formatTimestamp({ milliseconds }: { milliseconds: number }): string {
	const totalSeconds = Math.max(0, Math.floor(milliseconds / 1000));
	const minutes = Math.floor(totalSeconds / 60);
	const seconds = totalSeconds % 60;
	return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export function TranscriptView() {
	const activeProject = useEditor((editor) => editor.project.getActive());
	const projectId = activeProject?.metadata.id;
	const [transcript, setTranscript] = useState<TranscriptDocument | null>(null);
	const [cleanupAnalysis, setCleanupAnalysis] = useState<TranscriptCleanupAnalysis | null>(null);
	const [selectedWordIds, setSelectedWordIds] = useState<Set<string>>(new Set());
	const [editKind, setEditKind] = useState<ScriptEditKind>("delete_words");
	const [editValue, setEditValue] = useState("");
	const [proposal, setProposal] = useState<ToolProposal | null>(null);
	const [isLoading, setIsLoading] = useState(false);
	const [isApplying, setIsApplying] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [jobMessage, setJobMessage] = useState<string | null>(null);

	const loadTranscript = useCallback(async () => {
		if (!projectId) return;
		setIsLoading(true);
		setError(null);
		try {
			const result = await localCoreClient.invokeTool({
				name: "read_script",
				arguments: { projectId, includeSuggestions: true },
			});
			if (!result.ok) throw new Error(result.error?.message ?? "Could not read transcript");
			const script = unwrapScriptResult({ value: result.data });
			setTranscript(script?.transcript ?? null);
			setCleanupAnalysis(script?.cleanupAnalysis ?? null);
		} catch (nextError) {
			setError(nextError instanceof Error ? nextError.message : "Could not read transcript");
		} finally {
			setIsLoading(false);
		}
	}, [projectId]);

	useEffect(() => {
		void Promise.resolve().then(loadTranscript);
	}, [loadTranscript]);

	useEffect(() => {
		return localCoreClient.connectEvents({
			onEvent: (event) => {
				if (event.type === "revision.changed" && event.projectId === projectId) {
					void loadTranscript();
				}
				if (event.type === "job.changed") {
					setJobMessage(`${event.job.kind}: ${event.job.message ?? event.job.state}`);
					if (event.job.state === "succeeded") void loadTranscript();
				}
			},
		});
	}, [loadTranscript, projectId]);

	const selectedUtteranceIds = useMemo(() => {
		if (!transcript) return [];
		return transcript.utterances
			.filter((utterance) => utterance.words.some((word) => selectedWordIds.has(word.id)))
			.map((utterance) => utterance.id);
	}, [selectedWordIds, transcript]);

	const toggleWord = ({ wordId }: { wordId: string }) => {
		setSelectedWordIds((current) => {
			const next = new Set(current);
			if (next.has(wordId)) next.delete(wordId);
			else next.add(wordId);
			return next;
		});
	};

	const startTranscription = async () => {
		if (!projectId) return;
		setError(null);
		setJobMessage("Preparing local transcription…");
		try {
			const envelope = await localCoreClient.readProject<DomainProjectDocument>({
				projectId,
			});
			const candidates = envelope.document.assets.filter(
				(asset) =>
					Boolean(asset.contentHash) &&
					(asset.kind === "audio" ||
						(asset.kind === "video" && asset.hasAudio)),
			);
			if (candidates.length !== 1) {
				throw new Error(
					candidates.length === 0
						? "Import one managed audio or spoken-video asset before transcription."
						: "This project has multiple spoken assets. Start transcription from Codex and specify the asset.",
				);
			}
			const result = await localCoreClient.invokeTool({
				name: "start_transcription",
				arguments: {
					projectId,
					expectedRevision: envelope.revision,
					assetId: candidates[0].id,
					engine: "auto",
					diarization: false,
				},
			});
			if (!result.ok) throw new Error(result.error?.message ?? "Transcription could not start");
			setJobMessage(result.jobId ? `Transcription queued (${result.jobId})` : "Transcription queued");
		} catch (nextError) {
			setError(nextError instanceof Error ? nextError.message : "Transcription could not start");
		}
	};

	const buildEditPayload = (): Record<string, unknown> => {
		if (editKind === "reorder_words") {
			const selected = new Set(selectedUtteranceIds);
			return {
				kind: editKind,
				utteranceIds: [
					...selectedUtteranceIds,
					...(transcript?.utterances
						.filter((utterance) => !selected.has(utterance.id))
						.map((utterance) => utterance.id) ?? []),
				],
			};
		}
		return {
			kind: editKind,
			wordIds: [...selectedWordIds],
			utteranceIds: selectedUtteranceIds,
			...(editKind === "close_gaps" ? { thresholdMs: 1500, targetGapMs: 180 } : {}),
			...(editKind === "change_speaker" ? { speakerId: editValue.trim() } : {}),
			...(editKind === "correct_display_text" ? { displayText: editValue } : {}),
		};
	};

	const validateEdit = async () => {
		if (!projectId || !transcript) return;
		setError(null);
		try {
			const result = await localCoreClient.invokeTool({
				name: "apply_script_edit",
				arguments: {
					projectId,
					expectedRevision: transcript.revision,
					dryRun: true,
					edit: buildEditPayload(),
				},
			});
			if (!result.ok || !result.proposal) {
				throw new Error(result.error?.message ?? "No valid edit plan was returned");
			}
			setProposal(result.proposal);
		} catch (nextError) {
			setError(nextError instanceof Error ? nextError.message : "Script edit validation failed");
		}
	};

	const validateRecommendedCleanup = async () => {
		if (!projectId || !transcript) return;
		setError(null);
		try {
			const result = await localCoreClient.invokeTool({
				name: "apply_script_edit",
				arguments: {
					projectId,
					expectedRevision: transcript.revision,
					dryRun: true,
					edits: [
						{
							kind: "auto_cleanup",
							pauseThresholdMs: 1_500,
							targetGapMs: 180,
						},
						{
							kind: "add_captions",
							options: {
								presetId: "studio-clean",
								wordHighlight: true,
							},
						},
					],
				},
			});
			if (!result.ok || !result.proposal) {
				throw new Error(result.error?.message ?? "No cleanup proposal was returned");
			}
			setProposal(result.proposal);
		} catch (nextError) {
			setError(nextError instanceof Error ? nextError.message : "Cleanup validation failed");
		}
	};

	const applyProposal = async () => {
		if (!proposal) return;
		setIsApplying(true);
		setError(null);
		try {
			const result = await localCoreClient.invokeTool({
				name: "apply_script_edit",
				arguments: {
					projectId: proposal.projectId,
					expectedRevision: proposal.baseRevision,
					proposalId: proposal.proposalId,
					operations: proposal.payload,
					confirm: true,
				},
			});
			if (!result.ok) throw new Error(result.error?.message ?? "Script edit failed");
			setProposal(null);
			setSelectedWordIds(new Set());
			await loadTranscript();
		} catch (nextError) {
			setError(nextError instanceof Error ? nextError.message : "Script edit failed");
		} finally {
			setIsApplying(false);
		}
	};

	const needsSelection = !["close_gaps"].includes(editKind);
	const needsValue = ["change_speaker", "correct_display_text"].includes(editKind);
	const canValidate = Boolean(
		transcript &&
		(!needsSelection || selectedWordIds.size > 0) &&
		(!needsValue || editValue.trim()),
	);
	const recommendedCleanupCount =
		cleanupAnalysis?.suggestions.filter((suggestion) => suggestion.recommended).length ?? 0;

	return (
		<PanelView
			title="Script"
			contentClassName="flex min-h-full flex-col px-3 pb-3"
			actions={
				<div className="flex items-center gap-1.5">
					{transcript && <Badge variant="outline">r{transcript.revision}</Badge>}
					<Button size="icon" variant="ghost" onClick={() => void loadTranscript()} disabled={isLoading}>
						<RefreshCw className={cn(isLoading && "animate-spin")} />
						<span className="sr-only">Refresh transcript</span>
					</Button>
				</div>
			}
		>
			{!transcript && !isLoading ? (
				<div className="flex flex-1 flex-col items-center justify-center px-5 text-center">
					<div className="bg-muted mb-3 rounded-full p-3">
						<AudioLines className="size-5" />
					</div>
					<p className="text-sm font-medium">Word-accurate editing</p>
					<p className="text-muted-foreground mt-1 text-xs leading-relaxed">
						Transcribe locally with word timestamps. Spoken text stays immutable while display text can be corrected.
					</p>
					<Button className="mt-4" size="sm" onClick={() => void startTranscription()}>
						<Sparkles /> Transcribe locally
					</Button>
					{jobMessage && <p className="text-muted-foreground mt-2 text-[11px]">{jobMessage}</p>}
					{error && <p className="text-destructive mt-2 text-xs">{error}</p>}
				</div>
			) : (
				<>
					<div className="flex-1 space-y-2 py-2">
						{isLoading && (
							<div className="text-muted-foreground flex items-center gap-2 text-xs">
								<LoaderCircle className="size-3 animate-spin" /> Loading transcript…
							</div>
						)}
						{cleanupAnalysis && cleanupAnalysis.suggestions.length > 0 && (
							<div className="border-border bg-muted/30 rounded-md border p-2.5">
								<div className="flex items-start justify-between gap-3">
									<div>
										<p className="flex items-center gap-1.5 text-xs font-medium">
											<Sparkles className="size-3.5" /> Cleanup review
										</p>
										<p className="text-muted-foreground mt-1 text-[10px] leading-relaxed">
											Local timestamp analysis · {cleanupAnalysis.summary.fillerCount} filler · {cleanupAnalysis.summary.repeatedTakeCount} repeat · {cleanupAnalysis.summary.longPauseCount} pause · {cleanupAnalysis.summary.highlightCount} highlight
										</p>
									</div>
									<Badge variant="secondary">{recommendedCleanupCount} recommended</Badge>
								</div>
								<div className="mt-2 max-h-44 space-y-1 overflow-y-auto pr-1">
									{cleanupAnalysis.suggestions.map((suggestion) => (
										<button
											type="button"
											key={suggestion.id}
											onClick={() => setSelectedWordIds(new Set(suggestion.wordIds))}
											className="border-border hover:bg-muted flex w-full items-start gap-2 rounded border bg-background p-2 text-left transition-colors"
										>
											{suggestion.kind === "longPause" ? (
												<Clock3 className="text-muted-foreground mt-0.5 size-3.5 shrink-0" />
											) : suggestion.kind === "repeatedTake" ? (
												<Repeat2 className="text-muted-foreground mt-0.5 size-3.5 shrink-0" />
											) : (
												<Sparkles className="text-muted-foreground mt-0.5 size-3.5 shrink-0" />
											)}
											<span className="min-w-0 flex-1">
												<span className="flex items-center gap-1.5 text-[11px] font-medium">
													{CLEANUP_LABELS[suggestion.kind]}
													{suggestion.recommended && <Badge variant="outline">Review edit</Badge>}
												</span>
												<span className="text-muted-foreground mt-0.5 block text-[10px] leading-snug">
													{suggestion.reason} · {(suggestion.confidenceBps / 100).toFixed(0)}%
												</span>
											</span>
										</button>
									))}
								</div>
								<Button
									className="mt-2 w-full"
									size="sm"
									variant="secondary"
									disabled={recommendedCleanupCount === 0}
									onClick={() => void validateRecommendedCleanup()}
								>
									<Sparkles /> Preview cleanup + captions
								</Button>
							</div>
						)}
						{transcript?.utterances.map((utterance) => {
							const firstWord = utterance.words[0];
							return (
								<div key={utterance.id} className="border-border rounded-md border p-2.5">
									<div className="text-muted-foreground mb-1.5 flex items-center justify-between text-[10px]">
										<span>{utterance.speakerId ?? "Speaker"}</span>
										<span>{firstWord ? formatTimestamp({ milliseconds: firstWord.startMs }) : ""}</span>
									</div>
									<div className="flex flex-wrap gap-x-1 gap-y-1 text-sm leading-6">
										{utterance.words.map((word) => (
											<button
												type="button"
												key={word.id}
												onClick={() => toggleWord({ wordId: word.id })}
												className={cn(
													"rounded px-0.5 outline-none transition-colors",
													selectedWordIds.has(word.id) && "bg-primary text-primary-foreground",
													word.deleted && "text-muted-foreground line-through",
													word.displayText !== word.spokenText && !selectedWordIds.has(word.id) && "underline decoration-dotted",
												)}
												title={`${word.spokenText} · ${word.startMs}–${word.endMs} ms`}
											>
												{word.displayText}
											</button>
										))}
									</div>
								</div>
							);
						})}
						{proposal && (
							<ProposalCard
								proposal={proposal}
								isApplying={isApplying}
								onApply={() => void applyProposal()}
							/>
						)}
					</div>

					<div className="sticky bottom-0 space-y-2 border-t bg-background pt-3">
						<div className="flex items-center gap-2">
							<Select
								value={editKind}
								onValueChange={(value) => {
									if (isScriptEditKind(value)) setEditKind(value);
								}}
							>
								<SelectTrigger className="h-8 flex-1">
									<SelectValue />
								</SelectTrigger>
								<SelectContent>
									{Object.entries(EDIT_LABELS).map(([kind, label]) => (
										<SelectItem key={kind} value={kind}>{label}</SelectItem>
									))}
								</SelectContent>
							</Select>
							<Badge variant="secondary">{selectedWordIds.size}</Badge>
						</div>
						{needsValue && (
							<Input
								value={editValue}
								onChange={(event) => setEditValue(event.target.value)}
								placeholder={editKind === "change_speaker" ? "Speaker name" : "Corrected display text"}
							/>
						)}
						{error && <p className="text-destructive text-xs">{error}</p>}
						<Button className="w-full" size="sm" disabled={!canValidate} onClick={() => void validateEdit()}>
							<Scissors /> Preview semantic edit
						</Button>
					</div>
				</>
			)}
		</PanelView>
	);
}
