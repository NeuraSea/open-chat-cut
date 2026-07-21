import type { Metadata } from "next";
import Link from "next/link";
import {
	ArrowRight,
	AudioWaveform,
	Bot,
	Captions,
	Check,
	Clapperboard,
	Code2,
	Film,
	ImageIcon,
	Layers3,
	LockKeyhole,
	MousePointer2,
	Play,
	Sparkles,
	WandSparkles,
} from "lucide-react";
import { Footer } from "@/components/footer";
import { Header } from "@/components/header";
import { Button } from "@/components/ui/button";

export const metadata: Metadata = {
	title: "OpenChatCut - The AI video editor that stays yours",
	description:
		"Plan edits with an agent, approve the diff, and keep every clip, caption, motion graphic, and generated asset editable.",
};

const features = [
	{
		icon: Bot,
		title: "Agent editing, with receipts",
		body: "Describe the outcome. OpenChatCut shows the plan, timeline diff, cost, and warnings before anything changes.",
	},
	{
		icon: Captions,
		title: "Edit video like a document",
		body: "Cut filler words, tighten pauses, relabel speakers, and restyle captions while linked audio and video stay in sync.",
	},
	{
		icon: Layers3,
		title: "Motion graphics stay editable",
		body: "Create lower thirds, callouts, charts, CTAs, and title cards from a safe, versioned motion-graphics model.",
	},
	{
		icon: ImageIcon,
		title: "Generate, then keep the asset",
		body: "Images, voice, B-roll, music, and SFX are downloaded into the managed media library with provenance attached.",
	},
	{
		icon: AudioWaveform,
		title: "Audio work is reversible",
		body: "Denoise, normalize, compress dialogue, duck music, and regenerate a voice segment without overwriting the source.",
	},
	{
		icon: Film,
		title: "A professional way out",
		body: "Export video, audio, captions, image sequences, alpha motion graphics, and interchange formats from a pinned revision.",
	},
] as const;

const creditCapabilities = [
	"Edit planning and content understanding",
	"Transcription, captions, and semantic search",
	"Open-model image and voice generation",
] as const;

function ProductPreview() {
	return (
		<div className="relative mx-auto w-full max-w-6xl">
			<div className="absolute -inset-10 -z-10 bg-[radial-gradient(circle_at_50%_30%,rgba(14,165,233,0.22),transparent_60%)] blur-2xl" />
			<div className="overflow-hidden rounded-[1.75rem] border border-white/10 bg-[#090b0f] text-white shadow-2xl shadow-sky-950/30 ring-1 ring-black/10">
				<div className="flex h-11 items-center justify-between border-b border-white/10 px-4 text-[11px] text-white/45">
					<div className="flex items-center gap-2">
						<span className="size-2.5 rounded-full bg-[#ff605c]" />
						<span className="size-2.5 rounded-full bg-[#ffbd44]" />
						<span className="size-2.5 rounded-full bg-[#00ca4e]" />
					</div>
					<span>launch-film.occ · revision 31</span>
					<span className="rounded-full bg-emerald-400/10 px-2 py-1 text-emerald-300">
						Saved locally
					</span>
				</div>

				<div className="grid min-h-[520px] lg:grid-cols-[minmax(0,1fr)_340px]">
					<div className="flex min-w-0 flex-col border-white/10 lg:border-r">
						<div className="grid flex-1 gap-3 p-3 sm:grid-cols-[180px_minmax(0,1fr)]">
							<div className="hidden rounded-xl border border-white/10 bg-white/[0.025] p-3 sm:block">
								<p className="mb-4 text-[10px] font-semibold tracking-[0.18em] text-white/35 uppercase">
									Transcript
								</p>
								<div className="space-y-3 text-[11px] leading-relaxed text-white/45">
									<p>
										<span className="text-sky-300">00:00</span> Today we are
										shipping a faster way to edit.
									</p>
									<p className="rounded-md bg-rose-400/10 p-2 text-rose-200/60 line-through">
										Um, let me start that again.
									</p>
									<p>
										<span className="text-sky-300">00:04</span> Every decision
										remains visible and reversible.
									</p>
									<p>
										<span className="text-sky-300">00:09</span> Start with an
										idea, finish with an editable timeline.
									</p>
								</div>
							</div>

							<div className="flex min-w-0 flex-col gap-3">
								<div className="relative flex flex-1 items-center justify-center overflow-hidden rounded-xl border border-white/10 bg-[radial-gradient(circle_at_55%_20%,#17324a_0%,#0c131d_48%,#07080b_100%)] p-6">
									<div className="absolute top-5 left-5 rounded-full border border-white/10 bg-black/30 px-3 py-1 text-[10px] text-white/55 backdrop-blur">
										Preview · 1080p
									</div>
									<div className="text-center">
										<div className="mx-auto mb-5 flex size-11 items-center justify-center rounded-full border border-sky-300/25 bg-sky-300/10">
											<Play className="ml-0.5 size-4 fill-sky-200 text-sky-200" />
										</div>
										<p className="text-2xl font-semibold tracking-[-0.04em] sm:text-3xl">
											Make the cut.
										</p>
										<p className="mt-2 text-sm text-white/45">
											Keep creative control.
										</p>
									</div>
									<div className="absolute right-5 bottom-5 left-5 rounded-lg border border-sky-300/25 bg-sky-300/10 px-4 py-3 backdrop-blur">
										<p className="text-xs font-semibold">Avery Chen</p>
										<p className="text-[10px] text-sky-100/60">
											Product designer
										</p>
									</div>
								</div>
							</div>
						</div>

						<div className="h-48 border-t border-white/10 bg-[#0b0d12] p-3">
							<div className="mb-2 flex items-center justify-between text-[10px] text-white/35">
								<span>00:00:03:18</span>
								<span>30 fps · 00:00:14:00</span>
							</div>
							<div className="relative space-y-2 overflow-hidden pl-12">
								<div className="absolute top-0 bottom-0 left-[39%] z-10 w-px bg-sky-400">
									<span className="absolute -top-1 -left-1 size-2 rotate-45 bg-sky-400" />
								</div>
								<div className="absolute top-1 left-0 text-[9px] text-white/30">
									VIDEO
								</div>
								<div className="flex h-9 gap-1">
									<div className="w-[27%] rounded border border-indigo-300/20 bg-indigo-400/20 p-2 text-[9px] text-indigo-100/70">
										Intro.mov
									</div>
									<div className="w-[42%] rounded border border-indigo-300/20 bg-indigo-400/20 p-2 text-[9px] text-indigo-100/70">
										Main take.mov
									</div>
									<div className="flex-1 rounded border border-indigo-300/20 bg-indigo-400/20 p-2 text-[9px] text-indigo-100/70">
										Close.mov
									</div>
								</div>
								<div className="absolute top-12 left-0 text-[9px] text-white/30">
									MG
								</div>
								<div className="ml-[9%] h-7 w-[36%] rounded border border-sky-300/25 bg-sky-300/15 p-1.5 text-[9px] text-sky-100/70">
									lower-third-signal
								</div>
								<div className="absolute top-[78px] left-0 text-[9px] text-white/30">
									AUDIO
								</div>
								<div className="h-7 rounded border border-emerald-300/20 bg-[repeating-linear-gradient(90deg,rgba(52,211,153,.22)_0_2px,transparent_2px_5px)]" />
							</div>
						</div>
					</div>

					<aside className="flex min-h-[430px] flex-col bg-[#0d1016] p-4">
						<div className="flex items-center justify-between border-b border-white/10 pb-3">
							<div className="flex items-center gap-2 text-xs font-medium">
								<Sparkles className="size-3.5 text-sky-300" />
								OpenChatCut Agent
							</div>
							<span className="size-2 rounded-full bg-emerald-400" />
						</div>
						<div className="flex-1 space-y-4 py-4 text-[11px] leading-relaxed">
							<div className="ml-8 rounded-xl rounded-tr-sm bg-white/8 p-3 text-white/75">
								Remove the false start, tighten pauses over 1.5 seconds, add
								captions, then place a lower third in the first five seconds.
							</div>
							<div className="space-y-3 rounded-xl border border-white/10 p-3">
								<div className="flex items-center justify-between">
									<span className="font-medium text-white/80">
										Review edit plan
									</span>
									<span className="text-white/35">4 operations</span>
								</div>
								{[
									"Remove false start at 00:02.1",
									"Close two pauses · save 2.4s",
									"Create linked caption track",
									"Add lower-third-signal · 0–5s",
								].map((operation) => (
									<div
										key={operation}
										className="flex items-start gap-2 text-white/50"
									>
										<Check className="mt-0.5 size-3 shrink-0 text-emerald-300" />
										<span>{operation}</span>
									</div>
								))}
								<div className="flex items-center justify-between border-t border-white/10 pt-3 text-[10px]">
									<span className="text-white/35">Cost · 3 AI credits</span>
									<span className="rounded-md bg-white px-3 py-1.5 font-medium text-black">
										Apply changes
									</span>
								</div>
							</div>
						</div>
						<div className="rounded-xl border border-white/10 bg-black/20 p-3 text-[11px] text-white/35">
							Ask for an edit, generation, or export…
							<div className="mt-3 flex items-center justify-between">
								<span>Auto-apply off</span>
								<ArrowRight className="size-3.5 text-white/55" />
							</div>
						</div>
					</aside>
				</div>
			</div>
		</div>
	);
}

export default function Home() {
	return (
		<div className="min-h-screen overflow-hidden bg-background">
			<Header />
			<main>
				<section className="relative px-6 pt-20 pb-16 md:pt-28 md:pb-24">
					<div className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-[680px] bg-[radial-gradient(ellipse_at_top,rgba(14,165,233,0.12),transparent_64%)]" />
					<div className="mx-auto max-w-5xl text-center">
						<div className="mb-7 inline-flex items-center gap-2 rounded-full border bg-background/75 px-3 py-1.5 text-xs font-medium shadow-sm backdrop-blur">
							<span className="size-1.5 rounded-full bg-emerald-500" />
							Local-first · Source-available · Codex-ready
						</div>
						<h1 className="mx-auto max-w-4xl text-5xl leading-[0.98] font-semibold tracking-[-0.055em] sm:text-6xl md:text-8xl">
							The AI video editor that stays yours.
						</h1>
						<p className="text-muted-foreground mx-auto mt-7 max-w-2xl text-lg leading-relaxed md:text-xl">
							Describe the cut. Review the plan. Keep every clip, caption,
							motion graphic, and generated asset editable on a real timeline.
						</p>
						<div className="mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row">
							<Button asChild size="lg" className="min-w-40 rounded-full">
								<Link href="/projects">
									Open the editor <ArrowRight />
								</Link>
							</Button>
							<Button
								asChild
								size="lg"
								variant="outline"
								className="min-w-40 rounded-full bg-background/70"
							>
								<Link href="/pricing">See Creator plan</Link>
							</Button>
						</div>
						<p className="text-muted-foreground mt-4 text-xs">
							Runs on your machine. Bring your own providers or use optional AI
							credits.
						</p>
					</div>
				</section>

				<section className="px-4 pb-24 sm:px-6 md:pb-32">
					<ProductPreview />
				</section>

				<section className="border-y bg-muted/20 px-6 py-24 md:py-32">
					<div className="mx-auto max-w-6xl">
						<div className="mb-14 max-w-2xl">
							<p className="text-primary mb-4 text-xs font-semibold tracking-[0.2em] uppercase">
								One creative loop
							</p>
							<h2 className="text-4xl font-semibold tracking-[-0.04em] md:text-6xl">
								AI speed without giving up the edit.
							</h2>
							<p className="text-muted-foreground mt-5 text-lg leading-relaxed">
								The agent works through the same operation engine as manual
								edits. Every accepted batch becomes a revision you can inspect
								and undo.
							</p>
						</div>
						<div className="grid gap-px overflow-hidden rounded-3xl border bg-border md:grid-cols-2 lg:grid-cols-3">
							{features.map(({ icon: Icon, title, body }) => (
								<article key={title} className="bg-background p-7 md:p-8">
									<div className="mb-8 flex size-10 items-center justify-center rounded-xl border bg-muted/30">
										<Icon className="size-4.5" />
									</div>
									<h3 className="text-lg font-semibold">{title}</h3>
									<p className="text-muted-foreground mt-3 text-sm leading-relaxed">
										{body}
									</p>
								</article>
							))}
						</div>
					</div>
				</section>

				<section className="px-6 py-24 md:py-32">
					<div className="mx-auto grid max-w-6xl items-center gap-14 lg:grid-cols-[0.9fr_1.1fr]">
						<div>
							<div className="mb-5 inline-flex items-center gap-2 rounded-full border border-sky-500/20 bg-sky-500/8 px-3 py-1.5 text-xs font-semibold text-sky-600 dark:text-sky-300">
								<Sparkles className="size-3.5" /> AI Credits · Preview
							</div>
							<h2 className="text-4xl font-semibold tracking-[-0.04em] md:text-6xl">
								Use local compute. Add credits only when useful.
							</h2>
							<p className="text-muted-foreground mt-6 text-lg leading-relaxed">
								OpenChatCut does not turn your editor into a compulsory cloud
								subscription. Local and BYOK workflows remain available.
								NeuraSea AI Credits add convenient hosted access to selected
								open models.
							</p>
							<Link
								href="https://github.com/NeuraSea/open-chat-cut/blob/main/AI-CREDITS.md"
								className="mt-7 inline-flex items-center gap-2 text-sm font-semibold"
								target="_blank"
								rel="noopener noreferrer"
							>
								How credits work <ArrowRight className="size-4" />
							</Link>
						</div>
						<div className="relative overflow-hidden rounded-[2rem] border bg-[#0c1017] p-7 text-white shadow-2xl md:p-10">
							<div className="absolute -top-28 -right-20 size-72 rounded-full bg-sky-500/20 blur-3xl" />
							<div className="relative">
								<div className="flex items-start justify-between gap-6">
									<div>
										<p className="text-sm font-medium text-sky-300">Creator</p>
										<p className="mt-2 text-4xl font-semibold tracking-tight">
											100 credits
										</p>
										<p className="mt-1 text-sm text-white/45">
											planned monthly allocation
										</p>
									</div>
									<div className="rounded-xl border border-white/10 bg-white/5 p-3">
										<WandSparkles className="size-5 text-sky-300" />
									</div>
								</div>
								<div className="my-8 h-px bg-white/10" />
								<ul className="space-y-4">
									{creditCapabilities.map((capability) => (
										<li
											key={capability}
											className="flex items-center gap-3 text-sm text-white/70"
										>
											<Check className="size-4 text-emerald-300" />
											{capability}
										</li>
									))}
								</ul>
								<div className="mt-8 rounded-xl border border-amber-300/15 bg-amber-300/5 p-4 text-xs leading-relaxed text-amber-100/60">
									Seedance, Suno, and other paid third-party providers are not
									included. Connect your own key or approve their separate cost.
								</div>
							</div>
						</div>
					</div>
				</section>

				<section className="px-6 pb-24 md:pb-32">
					<div className="mx-auto max-w-6xl overflow-hidden rounded-[2rem] border bg-muted/25">
						<div className="grid lg:grid-cols-2">
							<div className="p-8 md:p-12">
								<p className="text-primary text-xs font-semibold tracking-[0.2em] uppercase">
									Creator License
								</p>
								<h2 className="mt-5 text-4xl font-semibold tracking-[-0.04em] md:text-5xl">
									Commercial work for the price of a coffee.
								</h2>
								<p className="text-muted-foreground mt-5 leading-relaxed">
									For eligible solo creators under US$100k annual revenue.
									Publish monetized content, deliver paid client work, and keep
									exported media royalty-free.
								</p>
								<div className="mt-8 flex flex-wrap gap-3">
									<Button asChild size="lg" className="rounded-full">
										<Link href="/pricing">
											View pricing <ArrowRight />
										</Link>
									</Button>
									<Button
										asChild
										size="lg"
										variant="outline"
										className="rounded-full"
									>
										<Link href="/projects">Try personal mode</Link>
									</Button>
								</div>
							</div>
							<div className="border-t p-8 lg:border-t-0 lg:border-l md:p-12">
								<div className="flex items-end gap-3">
									<span className="text-5xl font-semibold tracking-[-0.05em]">
										¥18
									</span>
									<span className="text-muted-foreground pb-1">/ month</span>
								</div>
								<p className="text-muted-foreground mt-2 text-sm">
									or ¥179 / year · US$2.50 monthly · US$25 yearly
								</p>
								<ul className="mt-8 grid gap-4 text-sm sm:grid-cols-2 lg:grid-cols-1">
									{[
										"One named creator · up to 3 devices",
										"Monetized and paid client videos",
										"Royalty-free exported media",
										"100 monthly AI credits when launched",
									].map((item) => (
										<li key={item} className="flex items-start gap-3">
											<Check className="text-primary mt-0.5 size-4 shrink-0" />
											{item}
										</li>
									))}
								</ul>
							</div>
						</div>
					</div>
				</section>

				<section className="border-t px-6 py-24 md:py-32">
					<div className="mx-auto max-w-4xl text-center">
						<div className="mx-auto mb-7 flex size-12 items-center justify-center rounded-2xl border bg-muted/30">
							<LockKeyhole className="size-5" />
						</div>
						<h2 className="text-4xl font-semibold tracking-[-0.04em] md:text-6xl">
							Your project is not a prompt history.
						</h2>
						<p className="text-muted-foreground mx-auto mt-6 max-w-2xl text-lg leading-relaxed">
							SQLite revisions, content-addressed media, resumable jobs,
							portable project packages, and a loopback-only daemon make the
							project yours before, during, and after AI assistance.
						</p>
						<div className="mt-8 flex flex-wrap justify-center gap-6 text-xs text-muted-foreground">
							<span className="flex items-center gap-2">
								<Code2 className="size-3.5" /> MCP tools
							</span>
							<span className="flex items-center gap-2">
								<MousePointer2 className="size-3.5" /> Manual editing
							</span>
							<span className="flex items-center gap-2">
								<Clapperboard className="size-3.5" /> Professional export
							</span>
						</div>
					</div>
				</section>
			</main>
			<Footer />
		</div>
	);
}
