import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, Check } from "lucide-react";
import { BasePage } from "@/app/base-page";
import { Button } from "@/components/ui/button";

const CREATOR_REQUEST_URL =
	"https://github.com/NeuraSea/open-chat-cut/issues/new?template=creator_license.yml";
const BUSINESS_REQUEST_URL =
	"https://github.com/NeuraSea/open-chat-cut/issues/new?title=Commercial%20license%20request";

export const metadata: Metadata = {
	title: "Pricing - OpenChatCut",
	description:
		"OpenChatCut licensing for personal creators, independent commercial creators, teams, SaaS, and OEM products.",
};

const plans = [
	{
		name: "Personal",
		price: "Free",
		detail: "Under the BSL Additional Use Grant",
		description: "For personal, non-commercial production and evaluation.",
		features: [
			"One individual user",
			"Personal, non-commercial videos",
			"Development, testing, and evaluation",
			"Local projects and exports",
		],
		cta: "Read the license",
		href: "https://github.com/NeuraSea/open-chat-cut/blob/main/LICENSE",
		highlighted: false,
	},
	{
		name: "Creator",
		price: "¥18 / US$2.50",
		detail: "monthly · or ¥179 / US$25 annually",
		description:
			"For eligible independent creators doing monetized or paid client work.",
		features: [
			"One named user on up to three devices",
			"Monetized content and paid client edits",
			"Private modifications for your own work",
			"Royalty-free exported media",
			"100 NeuraSea AI credits monthly (Preview)",
			"For creators below US$100k annual revenue",
		],
		cta: "Request Creator License",
		href: CREATOR_REQUEST_URL,
		highlighted: true,
	},
	{
		name: "Business",
		price: "Custom",
		detail: "annual, site, or OEM terms",
		description:
			"For teams, internal production, SaaS, hosted services, and product integration.",
		features: [
			"Multiple users and managed deployment",
			"Business production workflows",
			"SaaS and hosted-service rights",
			"OEM, white-label, and embedding options",
			"Optional support, updates, and SLA",
		],
		cta: "Request Commercial Terms",
		href: BUSINESS_REQUEST_URL,
		highlighted: false,
	},
] as const;

export default function PricingPage() {
	return (
		<BasePage
			maxWidth="6xl"
			title="Simple licensing for every stage"
			description="Start free for personal work and evaluation. Upgrade only when OpenChatCut creates commercial production value."
		>
			<div className="grid gap-6 lg:grid-cols-3">
				{plans.map((plan) => (
					<section
						key={plan.name}
						className={
							plan.highlighted
								? "border-primary bg-primary/5 flex flex-col rounded-3xl border-2 p-7 shadow-lg"
								: "bg-card flex flex-col rounded-3xl border p-7"
						}
					>
						<div className="mb-6">
							<div className="mb-3 flex items-center justify-between gap-3">
								<h2 className="text-2xl font-semibold">{plan.name}</h2>
								{plan.highlighted && (
									<span className="bg-primary text-primary-foreground rounded-full px-3 py-1 text-xs font-semibold">
										Best for solo creators
									</span>
								)}
							</div>
							<p className="text-3xl font-bold tracking-tight">{plan.price}</p>
							<p className="text-muted-foreground mt-1 text-sm">
								{plan.detail}
							</p>
							<p className="text-muted-foreground mt-5 leading-relaxed">
								{plan.description}
							</p>
						</div>

						<ul className="mb-8 flex flex-1 flex-col gap-3">
							{plan.features.map((feature) => (
								<li key={feature} className="flex items-start gap-3 text-sm">
									<Check className="text-primary mt-0.5 size-4 shrink-0" />
									<span>{feature}</span>
								</li>
							))}
						</ul>

						<Link href={plan.href} target="_blank" rel="noopener noreferrer">
							<Button
								className="w-full"
								variant={plan.highlighted ? "default" : "outline"}
							>
								{plan.cta}
								<ArrowRight className="size-4" />
							</Button>
						</Link>
					</section>
				))}
			</div>

			<div className="bg-muted/40 rounded-3xl border p-6 text-sm leading-relaxed">
				<p className="font-medium">Your exported media stays yours.</p>
				<p className="text-muted-foreground mt-2">
					A valid Creator License lets you publish, monetize, and deliver media
					exported during its term without royalties to NeuraSea. The Creator
					plan does not include team, SaaS, hosted, OEM, white-label, embedding,
					or software redistribution rights. See the full{" "}
					<Link
						href="https://github.com/NeuraSea/open-chat-cut/blob/main/CREATOR-LICENSE.md"
						className="text-primary underline"
						target="_blank"
						rel="noopener noreferrer"
					>
						Creator terms
					</Link>{" "}
					before ordering.
				</p>
			</div>

			<div className="rounded-3xl border border-sky-500/20 bg-sky-500/5 p-6 text-sm leading-relaxed">
				<p className="font-medium">AI Credits are optional and in preview.</p>
				<p className="text-muted-foreground mt-2">
					The planned Creator allocation covers selected NeuraSea-hosted open
					models. Local and bring-your-own-key workflows remain available.
					Seedance, Suno, and other paid third-party providers are excluded
					unless an accepted order says otherwise. Read the full{" "}
					<Link
						href="https://github.com/NeuraSea/open-chat-cut/blob/main/AI-CREDITS.md"
						className="text-primary underline"
						target="_blank"
						rel="noopener noreferrer"
					>
						AI Credits policy
					</Link>
					.
				</p>
			</div>
		</BasePage>
	);
}
