import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const dist = resolve(root, "dist");
const domain = "https://open-chatcut.nervafs.xyz";
const repository = "https://github.com/NeuraSea/open-chat-cut";
const creatorRequest = `${repository}/issues/new?template=creator_license.yml`;

const locales = {
	en: {
		lang: "en",
		path: "/",
		alternatePath: "/zh/",
		alternateLabel: "中文",
		title: "OpenChatCut — The AI video editor that stays yours",
		description:
			"Plan edits with an agent, approve the diff, and keep every clip, caption, motion graphic, and generated asset editable.",
		nav: { features: "Features", credits: "AI Credits", pricing: "Pricing" },
		badge: "Local-first · Source-available · Codex-ready",
		headline: "The AI video editor that stays yours.",
		subtitle:
			"Describe the cut. Review the plan. Keep every clip, caption, motion graphic, and generated asset editable on a real timeline.",
		primaryCta: "View source & install",
		secondaryCta: "See Creator plan",
		heroNote:
			"Runs on your machine. Bring your own providers or use optional AI credits.",
		captureCaption:
			"Actual OpenChatCut Web editor · deterministic MG/audio fixture · no mock controls",
		featureEyebrow: "One creative loop",
		featureTitle: "AI speed without giving up the edit.",
		featureIntro:
			"The agent works through the same operation engine as manual edits. Every accepted batch becomes a revision you can inspect and undo.",
		features: [
			["Agent editing, with receipts", "Review the plan, timeline diff, estimated cost, and warnings before anything changes."],
			["Edit video like a document", "Cut filler words, tighten pauses, relabel speakers, and keep linked audio and video in sync."],
			["Editable motion graphics", "Create lower thirds, charts, callouts, CTAs, and title cards from a safe versioned model."],
			["Generate, then keep the asset", "Images, voice, B-roll, music, and SFX become managed local assets with provenance."],
			["Reversible audio work", "Denoise, normalize, compress, duck music, and regenerate voice without replacing the source."],
			["Professional export", "Export video, audio, captions, alpha motion graphics, and interchange formats from a pinned revision."],
		],
		creditsBadge: "AI Credits · Preview",
		creditsTitle: "Use local compute. Add credits only when useful.",
		creditsBody:
			"Local and bring-your-own-key workflows remain available. NeuraSea AI Credits add optional hosted access to selected open models.",
		creditsAmount: "100 credits",
		creditsInterval: "planned monthly Creator allocation",
		creditsItems: [
			"Edit planning and content understanding",
			"Transcription, captions, and semantic search",
			"Open-model image and voice generation",
		],
		creditsExclusion:
			"Seedance, Suno, and other paid third-party providers are excluded. Connect your own key or approve their separate cost.",
		pricingEyebrow: "Licensing",
		pricingTitle: "Start personally. Pay when your work earns.",
		plans: [
			{
				name: "Personal",
				price: "Free",
				detail: "Personal non-commercial use and evaluation",
				items: ["One individual", "Local projects and exports", "No mandatory cloud account"],
				cta: "Read BSL terms",
				href: `${repository}/blob/main/LICENSE`,
			},
			{
				name: "Creator",
				price: "¥18 / US$2.50",
				detail: "monthly · ¥179 / US$25 annually",
				items: ["One named creator · three devices", "Monetized and paid client videos", "100 monthly AI credits when launched"],
				cta: "Request Creator License",
				href: creatorRequest,
				highlighted: true,
			},
			{
				name: "Business",
				price: "Custom",
				detail: "Team, SaaS, hosted, OEM, and white-label terms",
				items: ["Multiple users", "Managed deployments", "Optional support and SLA"],
				cta: "Request commercial terms",
				href: `${repository}/issues/new?title=Commercial%20license%20request`,
			},
		],
		legalNote:
			"OpenChatCut is source-available under Business Source License 1.1. OpenCut Classic attribution is retained separately and is not sponsorship or endorsement.",
		footerTagline: "Local-first AI video editing with an auditable timeline.",
	},
	zh: {
		lang: "zh-CN",
		path: "/zh/",
		alternatePath: "/",
		alternateLabel: "English",
		title: "OpenChatCut — 真正属于你的 AI 视频编辑器",
		description:
			"让 Agent 规划剪辑，审核差异后再应用；视频片段、字幕、MG 和生成素材始终可继续编辑。",
		nav: { features: "功能", credits: "AI 点数", pricing: "价格" },
		badge: "本地优先 · 源码可见 · 支持 Codex",
		headline: "真正属于你的 AI 视频编辑器。",
		subtitle:
			"描述你的剪辑目标，审核计划和差异，再把每个片段、字幕、MG 与生成素材保留在真实可编辑的时间线上。",
		primaryCta: "查看源码与安装",
		secondaryCta: "查看 Creator 方案",
		heroNote: "运行在你的设备上；可自带模型密钥，也可选择 AI 点数。",
		captureCaption: "真实 OpenChatCut Web 编辑器 · MG/音频验收项目 · 非手绘模拟界面",
		featureEyebrow: "完整创作闭环",
		featureTitle: "获得 AI 的速度，不放弃剪辑控制权。",
		featureIntro:
			"Agent 与手动编辑共用同一套 Operation Engine。每批已接受修改都会成为可以检查和撤销的项目 revision。",
		features: [
			["有凭据的 Agent 剪辑", "执行前先查看计划、时间线差异、预计费用和风险提示。"],
			["像编辑文档一样剪视频", "删除口头禅、压缩停顿、修改说话人，同时保持音画和字幕同步。"],
			["可编辑的动态图形", "使用安全、版本化模型创建 lower third、图表、callout、CTA 和标题卡。"],
			["生成后立即归档", "图片、旁白、B-roll、音乐和 SFX 会成为带来源记录的本地受管素材。"],
			["可撤销的音频处理", "降噪、响度、压缩、音乐闪避和局部旁白重生成都不会覆盖原文件。"],
			["专业交付", "从固定 revision 导出视频、音频、字幕、透明 MG 和专业交换格式。"],
		],
		creditsBadge: "AI 点数 · 预览",
		creditsTitle: "优先使用本地算力，只在有价值时使用点数。",
		creditsBody:
			"本地模型和自带密钥始终可用。NeuraSea AI Credits 为部分托管开源模型提供可选的即用能力。",
		creditsAmount: "100 点数",
		creditsInterval: "Creator 计划每月拟包含额度",
		creditsItems: ["剪辑规划与内容理解", "转录、字幕和语义搜索", "开源图片与语音生成"],
		creditsExclusion:
			"Seedance、Suno 等付费第三方服务不包含在内；请连接自己的密钥或单独确认费用。",
		pricingEyebrow: "许可证",
		pricingTitle: "个人创作免费，产生商业价值后再付费。",
		plans: [
			{
				name: "Personal",
				price: "免费",
				detail: "个人非商业创作和评估",
				items: ["一名个人用户", "本地项目和导出", "无需强制云端账号"],
				cta: "阅读 BSL 条款",
				href: `${repository}/blob/main/LICENSE`,
			},
			{
				name: "Creator",
				price: "¥18 / US$2.50",
				detail: "每月 · 年付 ¥179 / US$25",
				items: ["一名创作者 · 三台设备", "商业化内容与付费客户项目", "服务上线后每月 100 AI 点数"],
				cta: "申请 Creator 许可证",
				href: creatorRequest,
				highlighted: true,
			},
			{
				name: "Business",
				price: "定制",
				detail: "团队、SaaS、托管、OEM 与白标",
				items: ["多用户", "受管部署", "可选支持与 SLA"],
				cta: "申请商业条款",
				href: `${repository}/issues/new?title=Commercial%20license%20request`,
			},
		],
		legalNote:
			"OpenChatCut 采用 Business Source License 1.1 源码可见许可。OpenCut Classic 的归属单独保留，不代表赞助或背书。",
		footerTagline: "本地优先、过程可审计的 AI 视频编辑器。",
	},
};

function escapeHtml(value) {
	return String(value)
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;")
		.replaceAll("'", "&#039;");
}

function checks(items) {
	return items.map((item) => `<li><span>✓</span>${escapeHtml(item)}</li>`).join("");
}

function page(copy) {
	const alternateLocale = copy.lang === "en" ? "zh-CN" : "en";
	const cards = copy.features
		.map(
			([title, body], index) => `
			<article class="feature-card">
				<div class="feature-number">0${index + 1}</div>
				<h3>${escapeHtml(title)}</h3>
				<p>${escapeHtml(body)}</p>
			</article>`,
		)
		.join("");
	const plans = copy.plans
		.map(
			(plan) => `
			<article class="plan ${plan.highlighted ? "featured" : ""}">
				${plan.highlighted ? '<div class="plan-badge">Creator</div>' : ""}
				<h3>${escapeHtml(plan.name)}</h3>
				<div class="price">${escapeHtml(plan.price)}</div>
				<p>${escapeHtml(plan.detail)}</p>
				<ul>${checks(plan.items)}</ul>
				<a class="button ${plan.highlighted ? "primary" : "secondary"}" href="${escapeHtml(plan.href)}" target="_blank" rel="noopener noreferrer">${escapeHtml(plan.cta)} →</a>
			</article>`,
		)
		.join("");

	return `<!doctype html>
<html lang="${copy.lang}">
<head>
	<meta charset="utf-8">
	<meta name="viewport" content="width=device-width, initial-scale=1">
	<title>${escapeHtml(copy.title)}</title>
	<meta name="description" content="${escapeHtml(copy.description)}">
	<meta name="theme-color" content="#080b10">
	<link rel="canonical" href="${domain}${copy.path}">
	<link rel="alternate" hreflang="en" href="${domain}/">
	<link rel="alternate" hreflang="zh-CN" href="${domain}/zh/">
	<link rel="alternate" hreflang="x-default" href="${domain}/">
	<meta property="og:type" content="website">
	<meta property="og:title" content="${escapeHtml(copy.title)}">
	<meta property="og:description" content="${escapeHtml(copy.description)}">
	<meta property="og:url" content="${domain}${copy.path}">
	<meta property="og:image" content="${domain}/assets/editor.png">
	<link rel="icon" href="/assets/logo.svg" type="image/svg+xml">
	<link rel="stylesheet" href="/assets/styles.css">
</head>
<body>
	<header class="site-header">
		<a class="brand" href="${copy.path}"><img src="/assets/logo.svg" alt=""><span>OpenChatCut</span></a>
		<nav aria-label="Primary navigation">
			<a href="#features">${escapeHtml(copy.nav.features)}</a>
			<a href="#credits">${escapeHtml(copy.nav.credits)}</a>
			<a href="#pricing">${escapeHtml(copy.nav.pricing)}</a>
		</nav>
		<div class="header-actions">
			<a class="language" href="${copy.alternatePath}" hreflang="${alternateLocale}">${escapeHtml(copy.alternateLabel)}</a>
			<a class="github" href="${repository}" target="_blank" rel="noopener noreferrer">GitHub ↗</a>
		</div>
	</header>

	<main>
		<section class="hero">
			<div class="pill"><span></span>${escapeHtml(copy.badge)}</div>
			<h1>${escapeHtml(copy.headline)}</h1>
			<p>${escapeHtml(copy.subtitle)}</p>
			<div class="actions">
				<a class="button primary" href="${repository}#install-and-run" target="_blank" rel="noopener noreferrer">${escapeHtml(copy.primaryCta)} →</a>
				<a class="button secondary" href="#pricing">${escapeHtml(copy.secondaryCta)}</a>
			</div>
			<small>${escapeHtml(copy.heroNote)}</small>
		</section>

		<figure class="product-shot">
			<img src="/assets/editor.png" width="1600" height="1000" alt="OpenChatCut Web editor" fetchpriority="high">
			<figcaption>${escapeHtml(copy.captureCaption)}</figcaption>
		</figure>

		<section class="section" id="features">
			<div class="section-copy">
				<div class="eyebrow">${escapeHtml(copy.featureEyebrow)}</div>
				<h2>${escapeHtml(copy.featureTitle)}</h2>
				<p>${escapeHtml(copy.featureIntro)}</p>
			</div>
			<div class="feature-grid">${cards}</div>
		</section>

		<section class="section credits" id="credits">
			<div class="section-copy">
				<div class="eyebrow">${escapeHtml(copy.creditsBadge)}</div>
				<h2>${escapeHtml(copy.creditsTitle)}</h2>
				<p>${escapeHtml(copy.creditsBody)}</p>
			</div>
			<div class="credit-card">
				<div class="credit-orb"></div>
				<strong>${escapeHtml(copy.creditsAmount)}</strong>
				<span>${escapeHtml(copy.creditsInterval)}</span>
				<ul>${checks(copy.creditsItems)}</ul>
				<p>${escapeHtml(copy.creditsExclusion)}</p>
			</div>
		</section>

		<section class="section" id="pricing">
			<div class="section-copy centered">
				<div class="eyebrow">${escapeHtml(copy.pricingEyebrow)}</div>
				<h2>${escapeHtml(copy.pricingTitle)}</h2>
			</div>
			<div class="pricing-grid">${plans}</div>
			<p class="legal-note">${escapeHtml(copy.legalNote)}</p>
		</section>
	</main>

	<footer>
		<div class="footer-brand"><img src="/assets/logo.svg" alt=""><div><strong>OpenChatCut</strong><p>${escapeHtml(copy.footerTagline)}</p></div></div>
		<div class="footer-links">
			<a href="${repository}" target="_blank" rel="noopener noreferrer">GitHub</a>
			<a href="${repository}/blob/main/LICENSE" target="_blank" rel="noopener noreferrer">License</a>
			<a href="${repository}/blob/main/CREATOR-LICENSE.md" target="_blank" rel="noopener noreferrer">Creator</a>
			<a href="${repository}/blob/main/AI-CREDITS.md" target="_blank" rel="noopener noreferrer">AI Credits</a>
		</div>
	</footer>
</body>
</html>`;
}

await rm(dist, { recursive: true, force: true });
await mkdir(resolve(dist, "assets"), { recursive: true });
await mkdir(resolve(dist, "zh"), { recursive: true });

await cp(
	resolve(root, "../web/public/landing/openchatcut-editor-fixture.png"),
	resolve(dist, "assets/editor.png"),
);
await cp(
	resolve(root, "../web/public/logos/openchatcut/logo.svg"),
	resolve(dist, "assets/logo.svg"),
);
await cp(resolve(root, "src/styles.css"), resolve(dist, "assets/styles.css"));
await writeFile(resolve(dist, "index.html"), page(locales.en));
await writeFile(resolve(dist, "zh/index.html"), page(locales.zh));
await writeFile(
	resolve(dist, "404.html"),
	page({ ...locales.en, headline: "Page not found.", subtitle: "Return to OpenChatCut.", path: "/404" }),
);
await writeFile(
	resolve(dist, "robots.txt"),
	`User-agent: *\nAllow: /\nSitemap: ${domain}/sitemap.xml\n`,
);
await writeFile(
	resolve(dist, "sitemap.xml"),
	`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml"><url><loc>${domain}/</loc><xhtml:link rel="alternate" hreflang="en" href="${domain}/"/><xhtml:link rel="alternate" hreflang="zh-CN" href="${domain}/zh/"/></url><url><loc>${domain}/zh/</loc><xhtml:link rel="alternate" hreflang="en" href="${domain}/"/><xhtml:link rel="alternate" hreflang="zh-CN" href="${domain}/zh/"/></url></urlset>\n`,
);
await writeFile(
	resolve(dist, "_headers"),
	`/*\n  X-Content-Type-Options: nosniff\n  Referrer-Policy: strict-origin-when-cross-origin\n  Permissions-Policy: camera=(), microphone=(), geolocation=()\n  Content-Security-Policy: default-src 'self'; img-src 'self'; style-src 'self'; script-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\n\n/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n`,
);
await writeFile(resolve(dist, "_redirects"), "/zh /zh/ 301\n");

console.log(`Built bilingual static landing site in ${dist}`);
