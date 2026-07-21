"use client";

import { ArrowRightIcon, CheckIcon } from "lucide-react";
import { useState } from "react";
import { useLocalStorage } from "@/services/storage/use-local-storage";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogTitle } from "../ui/dialog";

const STEPS = [
	{
		title: "Welcome to OpenChatCut",
		description:
			"A local-first video editor where manual edits, Agent plans, generated assets, and exports share one revisioned project.",
		points: [
			"Edit with the Classic timeline, Script workspace, or Codex Agent",
			"Undo Agent changes as a single project revision",
		],
	},
	{
		title: "Your local core owns the project",
		description:
			"The loopback daemon stores projects in SQLite and managed media in a content-addressed library. Browser storage is only a cache and migration source.",
		points: [
			"Closing the browser does not stop Codex jobs or exports",
			"Linked files are optional and explicitly marked non-portable",
		],
	},
	{
		title: "You control external services",
		description:
			"Telemetry is off. Codex uses its own signed-in session, while paid or third-party providers require explicit configuration and approval before project context leaves this machine.",
		points: [
			"Review the normalized diff, cost, dependencies, and warnings first",
			"Start with the Agent or Script tabs, or keep editing manually",
		],
	},
] as const;

export function Onboarding() {
	const [step, setStep] = useState(0);
	const [hasSeenOnboarding, setHasSeenOnboarding] = useLocalStorage({
		key: "hasSeenOnboarding",
		defaultValue: false,
	});

	const isOpen = !hasSeenOnboarding;

	const current = STEPS[Math.min(step, STEPS.length - 1)];
	const isLastStep = step === STEPS.length - 1;

	const handleNext = () => setStep((currentStep) => currentStep + 1);

	const handleClose = () => {
		setHasSeenOnboarding({ value: true });
	};

	return (
		<Dialog open={isOpen} onOpenChange={handleClose}>
			<DialogContent className="sm:max-w-[480px]">
				<DialogTitle>
					<span className="sr-only">{current.title}</span>
				</DialogTitle>
				<DialogBody>
					<div className="space-y-6">
						<div className="space-y-3">
							<div className="text-muted-foreground text-xs font-medium tracking-wider uppercase">
								Step {step + 1} of {STEPS.length}
							</div>
							<h2 className="text-xl font-semibold tracking-tight">
								{current.title}
							</h2>
							<p className="text-muted-foreground text-sm leading-6">
								{current.description}
							</p>
							<ul className="space-y-2 pt-1">
								{current.points.map((point) => (
									<li key={point} className="flex gap-2 text-sm leading-5">
										<CheckIcon className="text-primary mt-0.5 size-4 shrink-0" />
										<span>{point}</span>
									</li>
								))}
							</ul>
						</div>
						<NextButton onClick={isLastStep ? handleClose : handleNext}>
							{isLastStep ? "Start editing" : "Next"}
						</NextButton>
					</div>
				</DialogBody>
			</DialogContent>
		</Dialog>
	);
}

function NextButton({
	children,
	onClick,
}: {
	children: React.ReactNode;
	onClick: () => void;
}) {
	return (
		<Button onClick={onClick} variant="default" className="w-full">
			{children}
			<ArrowRightIcon className="size-4" />
		</Button>
	);
}
