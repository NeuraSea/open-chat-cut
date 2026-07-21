import type { Metadata } from "next";
import { BasePage } from "@/app/base-page";
import { Separator } from "@/components/ui/separator";

export const metadata: Metadata = {
	title: "Privacy - OpenChatCut",
	description:
		"How the local OpenChatCut editor stores projects and uses optional providers.",
};

export default function PrivacyPage() {
	return (
		<BasePage
			title="Privacy"
			description="OpenChatCut is local-first. Network access is limited to features you explicitly configure or approve."
		>
			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Local project authority</h2>
				<p>
					The loopback-only <code>openchatcutd</code> daemon stores project
					revisions, jobs, transcripts and metadata in local SQLite. Imported
					media is copied into a local SHA-256 content store by default. Browser
					storage is only a UI cache and Classic migration source, not the
					authoritative project database.
				</p>
				<p>
					Linked-file mode is optional, requires an explicit portability
					warning, and keeps the selected host path outside the managed media
					store.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">
					When data can leave the device
				</h2>
				<p>
					The editor contacts an external service only for an approved workflow:
				</p>
				<ul className="list-disc space-y-2 pl-6">
					<li>
						Codex Agent and image generation delegate authentication to your
						installed Codex login. Planning may include the pinned project
						document and up to eight managed thumbnails or contact sheets;
						source video is not attached.
					</li>
					<li>
						A configured OpenAI-compatible or Ollama Agent receives the
						disclosed pinned project, transcript, caption and asset metadata
						only after you confirm the external context transfer. It does not
						receive visual frames.
					</li>
					<li>
						Seedance-compatible, fal.ai, Suno and other configured generation
						providers receive the reviewed prompt and parameters only after
						confirmation. Their terms, retention and charges are controlled by
						that provider and your key.
					</li>
					<li>
						Remote media import and URL capture contact the approved public URL
						through SSRF, redirect and size checks. Downloaded results
						immediately become local managed assets.
					</li>
				</ul>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">
					Credentials and browser sessions
				</h2>
				<p>
					OpenChatCut has no cloud account system. It never reads or copies
					Codex credential files. Provider keys remain in the private daemon
					configuration and are not returned to the Web editor or MCP bridge.
				</p>
				<p>
					The daemon issues a short-lived HttpOnly loopback cookie and CSRF
					token to the local editor. The daemon bearer token and runtime
					descriptor are stored in permission-restricted local files.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Telemetry</h2>
				<p>
					Telemetry and product analytics are disabled by default. The local
					daemon does not send usage, project, crash or media data to an
					OpenChatCut service.
				</p>
			</section>

			<section className="flex flex-col gap-3">
				<h2 className="text-2xl font-semibold">Retention and control</h2>
				<p>
					Projects, named versions, job records and managed assets remain on the
					local machine until you delete them. Safe media garbage collection
					preserves bytes that are still referenced by history, versions or
					active jobs. Portable project packages contain the pinned project and
					managed media you choose to export.
				</p>
				<p>
					You can inspect this behavior in the repository source and remove the
					local OpenChatCut data directory when you no longer need it.
				</p>
			</section>

			<Separator />
			<p className="text-muted-foreground text-sm">
				Last updated: July 16, 2026
			</p>
		</BasePage>
	);
}
