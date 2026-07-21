import type { Metadata } from "next";
import Link from "next/link";
import { BasePage } from "@/app/base-page";
import { Button } from "@/components/ui/button";
import { SOCIAL_LINKS } from "@/site/social";

export const metadata: Metadata = {
	title: "Sponsors - OpenChatCut",
	description: "Verified sponsorship information for OpenChatCut.",
};

export default function SponsorsPage() {
	return (
		<BasePage
			title="Sponsors"
			description="OpenChatCut does not currently publish a sponsorship roster."
		>
			<div className="rounded-3xl border bg-muted/20 p-10 text-center">
				<p className="text-lg font-medium">No sponsors are listed.</p>
				<p className="text-muted-foreground mx-auto mt-3 max-w-xl leading-relaxed">
					We only display a company or project here after NeuraSea verifies a
					current sponsorship relationship and receives permission to use its
					name and logo. OpenCut Classic attribution is not sponsorship.
				</p>
				<Button asChild variant="outline" className="mt-7">
					<Link
						href={`${SOCIAL_LINKS.github}/issues/new?title=Sponsorship%20inquiry`}
						target="_blank"
						rel="noopener noreferrer"
					>
						Contact NeuraSea through GitHub
					</Link>
				</Button>
			</div>
		</BasePage>
	);
}
