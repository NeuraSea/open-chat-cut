import type { Metadata } from "next";
import { BasePage } from "@/app/base-page";
import {
	Accordion,
	AccordionContent,
	AccordionItem,
	AccordionTrigger,
} from "@/components/ui/accordion";
import { Separator } from "@/components/ui/separator";
import { SOCIAL_LINKS } from "@/site/social";

export const metadata: Metadata = {
	title: "Terms of Service - OpenChatCut",
	description:
		"OpenChatCut's Terms of Service. Transparent terms for our source-available video editor.",
	openGraph: {
		title: "Terms of Service - OpenChatCut",
		description:
			"OpenChatCut's Terms of Service. Transparent terms for our source-available video editor.",
		type: "website",
	},
};

export default function TermsPage() {
	return (
		<BasePage
			title="Terms of service"
			description="Transparent terms for our source-available video editor. Contact us if you have any questions."
		>
			<Accordion type="single" collapsible className="w-full">
				<AccordionItem
					value="quick-summary"
					className="rounded-2xl border px-5"
				>
					<AccordionTrigger className="no-underline!">
						Quick summary
					</AccordionTrigger>
					<AccordionContent>
						<h3 className="mb-3 text-lg font-medium">
							You own your content, we own nothing.
						</h3>
						<ol className="list-decimal space-y-2 pl-6">
							<li>
								Core project state stays local; external AI is used only when
								you configure or approve it
							</li>
							<li>We never claim ownership of your content</li>
							<li>
								Personal non-commercial use is free; commercial production
								requires a Creator or other commercial license
							</li>
							<li>
								You&apos;re responsible for how you use it - don&apos;t break
								the law
							</li>
							<li>
								Service provided &quot;as is&quot; - we can&apos;t guarantee
								perfect uptime
							</li>
							<li>
								Source-available code lets you review and evaluate the software
								under the Business Source License 1.1
							</li>
							<li>
								No account required - your exported videos are always yours
							</li>
						</ol>
						<p className="mt-4">
							Questions? Contact us through the{" "}
							<a
								href={`${SOCIAL_LINKS.github}/issues`}
								className="text-primary hover:underline"
							>
								OpenChatCut repository
							</a>
						</p>
					</AccordionContent>
				</AccordionItem>
			</Accordion>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Your Content, Your Rights</h2>
				<p>
					<strong>You own everything you create.</strong> All editing and
					processing happens locally on your device. We never see, store, or
					have access to your files. We make no claims to ownership, licensing,
					or rights over your videos, projects, or any content you create using
					OpenChatCut.
				</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>
						Your content stays local unless you choose a feature that clearly
						discloses an external destination
					</li>
					<li>You retain all intellectual property rights to your content</li>
					<li>You can export and use your content however you choose</li>
					<li>No watermarks or output royalties claimed by NeuraSea</li>
				</ul>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">How You Can Use OpenChatCut</h2>
				<p>Your permitted use depends on the applicable license:</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>Personal, non-commercial production use is free</li>
					<li>Development, testing, and evaluation are permitted by BSL 1.1</li>
					<li>
						An active Creator License permits eligible solo creators to produce
						monetized content and paid client work
					</li>
					<li>
						Team, business, SaaS, hosted, OEM, white-label, and redistribution
						uses require separate commercial terms
					</li>
					<li>Exported media remains yours and carries no NeuraSea royalty</li>
				</ul>
				<p>
					You&apos;re responsible for how you use OpenChatCut and the content
					you create. Don&apos;t use it for anything illegal in your
					jurisdiction.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">AI Features</h2>
				<p>
					AI features can use local models or optional external providers. Local
					processing stays on infrastructure you control. If you configure and
					approve an external provider, relevant prompts or media may be sent to
					that provider under its own terms. AI features are optional.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Service</h2>
				<p>
					OpenChatCut does not currently require an account. The service is
					provided &quot;as is&quot; without warranties. While we strive for
					reliability, we can&apos;t guarantee uninterrupted service.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Source-Available License</h2>
				<p>
					OpenChatCut source code is available under the Business Source License
					1.1 and separate commercial terms:
				</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>Review our code to see exactly how we handle your data</li>
					<li>Evaluate and self-host OpenChatCut for non-production use</li>
					<li>Use it personally for non-commercial production work</li>
					<li>
						Purchase the low-cost Creator License for eligible solo commercial
						work
					</li>
					<li>Purchase commercial rights for business production use</li>
					<li>Contribute improvements back to the community</li>
				</ul>
				<p>
					View our source code and license on{" "}
					<a
						href={SOCIAL_LINKS.github}
						target="_blank"
						rel="noopener noreferrer"
						className="text-primary hover:underline"
					>
						GitHub
					</a>
					.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Limitations and Liability</h2>
				<p>
					OpenChatCut is provided under its applicable source or commercial
					license. To the extent permitted by law:
				</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>We&apos;re not liable for any loss of data or content</li>
					<li>
						Projects are stored in your browser and may be lost if you clear
						browser data
					</li>
					<li>We&apos;re not responsible for how you use the service</li>
					<li>Our liability is limited to the maximum extent allowed by law</li>
				</ul>
				<p>
					Since your content stays on your device, we have no way to recover
					lost projects. Consider exporting important videos when finished
					editing.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Service Changes</h2>
				<p>We may update OpenChatCut and these terms:</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>We&apos;ll notify you of significant changes to these terms</li>
					<li>Continued use means you accept any updates</li>
					<li>
						Older versions remain governed by the license shipped with them
					</li>
					<li>Major changes will be discussed with the community on GitHub</li>
				</ul>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Stopping Use</h2>
				<p>You can stop using OpenChatCut at any time:</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>Clear your browser data to remove local projects</li>
				</ul>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Contact Us</h2>
				<p>Questions about these terms or need to report an issue?</p>
				<p>
					Contact us through the{" "}
					<a
						href={`${SOCIAL_LINKS.github}/issues`}
						target="_blank"
						rel="noopener noreferrer"
						className="text-primary hover:underline"
					>
						OpenChatCut GitHub repository
					</a>
					.
				</p>
				<p>
					These terms are governed by applicable law in your jurisdiction. We
					prefer to resolve disputes through friendly discussion in our
					developer community.
				</p>
			</section>
			<Separator />
			<p className="text-muted-foreground text-sm">
				Last updated: July 21, 2026
			</p>
		</BasePage>
	);
}
