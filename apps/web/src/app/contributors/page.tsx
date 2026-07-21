import type { Metadata } from "next";
import Link from "next/link";
import { BasePage } from "@/app/base-page";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { SOCIAL_LINKS } from "@/site/social";

export const metadata: Metadata = {
	title: "Contributors - OpenChatCut",
	description: "Contributors to the NeuraSea OpenChatCut repository on GitHub.",
};

interface Contributor {
	id: number;
	login: string;
	avatar_url: string;
	html_url: string;
	contributions: number;
	type: string;
}

async function getContributors(): Promise<Contributor[]> {
	try {
		const response = await fetch(
			"https://api.github.com/repos/NeuraSea/open-chat-cut/contributors?per_page=100",
			{
				headers: {
					Accept: "application/vnd.github.v3+json",
					"User-Agent": "NeuraSea-OpenChatCut-Web",
				},
				next: { revalidate: 600 },
			},
		);

		if (!response.ok) return [];
		const contributors: unknown = await response.json();
		if (!Array.isArray(contributors)) return [];

		return contributors.filter(
			(contributor): contributor is Contributor =>
				typeof contributor === "object" &&
				contributor !== null &&
				"type" in contributor &&
				contributor.type === "User" &&
				"login" in contributor &&
				typeof contributor.login === "string" &&
				"id" in contributor &&
				typeof contributor.id === "number" &&
				"avatar_url" in contributor &&
				typeof contributor.avatar_url === "string" &&
				"html_url" in contributor &&
				typeof contributor.html_url === "string" &&
				"contributions" in contributor &&
				typeof contributor.contributions === "number",
		);
	} catch {
		return [];
	}
}

export default async function ContributorsPage() {
	const contributors = await getContributors();

	return (
		<BasePage
			maxWidth="6xl"
			title="OpenChatCut contributors"
			description="This list is sourced only from the NeuraSea/open-chat-cut repository. Upstream OpenCut Classic attribution is documented separately in NOTICE.md."
		>
			{contributors.length > 0 ? (
				<div className="grid grid-cols-2 gap-6 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
					{contributors.map((contributor) => (
						<Link
							key={contributor.id}
							href={contributor.html_url}
							target="_blank"
							rel="noopener noreferrer"
						>
							<Card className="h-full transition-colors hover:bg-muted/30">
								<CardContent className="flex flex-col items-center gap-3 p-5 text-center">
									<Avatar className="size-16">
										<AvatarImage
											src={contributor.avatar_url}
											alt={`${contributor.login}'s avatar`}
										/>
										<AvatarFallback>
											{contributor.login.charAt(0).toUpperCase()}
										</AvatarFallback>
									</Avatar>
									<div>
										<p className="font-medium">{contributor.login}</p>
										<p className="text-muted-foreground mt-1 text-xs">
											{contributor.contributions} repository contributions
										</p>
									</div>
								</CardContent>
							</Card>
						</Link>
					))}
				</div>
			) : (
				<div className="rounded-3xl border bg-muted/20 p-10 text-center">
					<p className="font-medium">
						Contributor data is temporarily unavailable.
					</p>
					<p className="text-muted-foreground mt-2 text-sm">
						No cached third-party or upstream contributor list is displayed.
					</p>
				</div>
			)}

			<div className="flex justify-center">
				<Button asChild variant="outline">
					<Link
						href={SOCIAL_LINKS.github}
						target="_blank"
						rel="noopener noreferrer"
					>
						View the OpenChatCut repository
					</Link>
				</Button>
			</div>
		</BasePage>
	);
}
