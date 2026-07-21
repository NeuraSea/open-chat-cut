"use client";

import { useState } from "react";
import { AlertTriangle, Check, LoaderCircle, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { ToolProposal } from "@/services/local-core";

export function ProposalCard({
	proposal,
	isApplying,
	onApply,
	applyLabel = "Apply changes",
	disabled = false,
	disabledReason,
}: {
	proposal: ToolProposal;
	isApplying?: boolean;
	onApply?: () => void;
	applyLabel?: string;
	disabled?: boolean;
	disabledReason?: string;
}) {
	const hasDanger = proposal.warnings.some(
		(warning) => warning.severity === "danger",
	);
	const [approval, setApproval] = useState({
		proposalId: proposal.proposalId,
		approved: false,
	});
	const dangerApproved =
		approval.proposalId === proposal.proposalId && approval.approved;
	const workflow = proposal.kind === "capabilityWorkflow";

	return (
		<div className="border-border bg-background rounded-lg border p-3 text-sm shadow-xs">
			<div className="mb-2 flex items-start gap-2">
				<ShieldCheck className="text-primary mt-0.5 size-4 shrink-0" />
				<div className="min-w-0 flex-1">
					<p className="font-medium">
						{workflow ? "Review creative workflow" : "Review edit plan"}
					</p>
					<p className="text-muted-foreground mt-0.5 text-xs leading-relaxed">
						{proposal.summary}
					</p>
				</div>
				<Badge variant="outline">r{proposal.baseRevision}</Badge>
			</div>

			<div className="space-y-1.5">
				{proposal.diffs.map((diff) => (
					<div
						key={diff.operationId}
						className="bg-muted/50 rounded-md px-2.5 py-2"
					>
						<div className="flex items-center justify-between gap-2">
							<span className="truncate text-xs font-medium">
								{diff.summary}
							</span>
							<Badge variant="secondary" className="shrink-0 text-[10px]">
								{diff.kind}
							</Badge>
						</div>
						{diff.targetIds.length > 0 && (
							<p className="text-muted-foreground mt-1 truncate font-mono text-[10px]">
								{diff.targetIds.join(", ")}
							</p>
						)}
					</div>
				))}
			</div>

			{proposal.warnings.length > 0 && (
				<div className="mt-2 space-y-1">
					{proposal.warnings.map((warning) => (
						<div
							key={`${warning.code}:${warning.message}`}
							className={
								warning.severity === "danger"
									? "text-destructive flex gap-1.5 text-xs"
									: "text-caution flex gap-1.5 text-xs"
							}
						>
							<AlertTriangle className="mt-0.5 size-3 shrink-0" />
							<span>{warning.message}</span>
						</div>
					))}
				</div>
			)}

			{proposal.dependencyImpact.length > 0 && (
				<p className="text-muted-foreground mt-2 text-[11px] leading-relaxed">
					Also updates: {proposal.dependencyImpact.join(", ")}
				</p>
			)}

			{hasDanger && onApply && (
				<div className="border-destructive/20 bg-destructive/5 mt-3 flex items-start gap-2 rounded-md border p-2 text-[11px] leading-relaxed">
					<Checkbox
						id={`approve-${proposal.proposalId}`}
						className="mt-0.5"
						checked={dangerApproved}
						onCheckedChange={(checked) =>
							setApproval({
								proposalId: proposal.proposalId,
								approved: checked === true,
							})
						}
					/>
					<label
						htmlFor={`approve-${proposal.proposalId}`}
						className="cursor-pointer"
					>
						I reviewed the destructive changes and want to apply them.
					</label>
				</div>
			)}

			{disabledReason && (
				<p className="text-caution mt-2 text-[11px]">{disabledReason}</p>
			)}

			<div className="mt-3 flex items-center justify-between gap-2 border-t pt-3">
				<span className="text-muted-foreground text-xs">
					{proposal.cost?.display ?? "No paid provider call"}
				</span>
				{onApply && (
					<Button
						size="sm"
						variant={hasDanger ? "destructive" : "default"}
						disabled={isApplying || disabled || (hasDanger && !dangerApproved)}
						onClick={onApply}
					>
						{isApplying ? <LoaderCircle className="animate-spin" /> : <Check />}
						{applyLabel}
					</Button>
				)}
			</div>
		</div>
	);
}
