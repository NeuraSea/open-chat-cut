import Link from "next/link";
import { Brand } from "@/components/landing-page";
import { BorderTrail } from "@/components/sora-ui/effects/border-trail";
import { TextEffect } from "@/components/sora-ui/texts/text-effect";
import { MaskedTextReveal } from "@/components/sora-ui/texts/text-reveal-mask";
import { TextScramble } from "@/components/sora-ui/texts/text-scramble";
import { SlotText } from "@/components/motion/slot-text";
import { repository, type Locale } from "@/lib/copy";

const pluginCopy = {
	en: {
		home: "/", alt: "/zh/codex-plugin/", altLabel: "中文", docs: "/docs/codex-plugin/",
		label: "OPENCHATCUT × CODEX", title: "VIDEO EDITING\nIN CODEX",
		lead: "Plan, inspect and deliver real OpenChatCut projects from Codex while every accepted change remains visible and editable in the timeline.",
		install: "Install the plugin", docsLabel: "Read the docs",
		terminalTitle: "One local bridge. The same project truth.",
		terminalLines: ["$ codex login", "$ ./scripts/install-codex-plugin.sh", "✓ 25 MCP tools registered", "✓ 10 editing skills loaded", "→ Open a new Codex task"],
		flowTitle: "Describe the cut. Review the plan. Keep editing.",
		flowBody: "Codex never rewrites project JSON. It calls the loopback daemon through a dependency-free STDIO bridge, validates a proposal, then submits revision-safe semantic operations through the same engine as the Web editor.",
		steps: [["01", "Ask", "Describe a cut, caption style, motion graphic, generated asset, audio treatment or delivery target."], ["02", "Review", "Inspect the normalized diff, affected dependencies, warnings and provider cost before any write."], ["03", "Apply", "Approved operations commit as one revision with CAS, idempotency and a single-step undo boundary."], ["04", "Handoff", "Open the editor URL at any time and continue manually on exactly the same project revision."]],
		capTitle: "A video-production toolbelt inside Codex.",
		caps: [["Timeline", "Inspect projects, validate edits, apply semantic operations and review history."], ["Speech & captions", "Transcribe, read the script, remove words or pauses and restyle linked captions."], ["Motion graphics", "Create editable lower thirds, title cards, callouts and charts with the safe MG DSL."], ["Generation", "List providers and generate images, B-roll, voice, music or SFX into managed local assets."], ["Audio", "Create reversible denoise and processing derivatives without replacing source media."], ["Delivery", "Render preview frames, validate the project, start pinned exports and track persistent jobs."]],
		securityTitle: "Local by default. Explicit when it leaves.",
		securityBody: "The bridge reads only the protected runtime descriptor and daemon token. It does not access SQLite, project JSON, Codex auth files or provider keys. Paid generation and outbound media remain approval-gated.",
		cta: "Install OpenChatCut for Codex", note: "Requires the local OpenChatCut daemon, Node.js 20+ and a signed-in Codex CLI.",
	},
	zh: {
		home: "/zh/", alt: "/codex-plugin/", altLabel: "English", docs: "/zh/docs/codex-plugin/",
		label: "OPENCHATCUT × CODEX", title: "在 CODEX\n完成视频剪辑",
		lead: "直接在 Codex 中规划、检查并交付真实 OpenChatCut 项目；每项已接受修改仍然完整保留在可见、可编辑的时间线上。",
		install: "安装插件", docsLabel: "阅读文档",
		terminalTitle: "一个本地 Bridge，同一份项目事实。",
		terminalLines: ["$ codex login", "$ ./scripts/install-codex-plugin.sh", "✓ 已注册 25 个 MCP 工具", "✓ 已加载 10 个剪辑 Skills", "→ 打开新的 Codex 任务"],
		flowTitle: "描述剪辑，审核计划，继续手动调整。",
		flowBody: "Codex 不会改写项目 JSON。它通过零依赖 STDIO bridge 调用 loopback daemon，验证 proposal 后，再通过与 Web 编辑器相同的 Operation Engine 提交 revision-safe 语义操作。",
		steps: [["01", "提出要求", "描述剪辑、字幕风格、MG、生成素材、音频处理或交付目标。"], ["02", "审核计划", "写入前检查标准化 diff、依赖影响、警告和 Provider 费用。"], ["03", "应用修改", "批准的操作通过 CAS 与幂等机制提交为一个 revision，并形成单步撤销边界。"], ["04", "回到编辑器", "随时打开编辑器 URL，在完全一致的项目 revision 上继续手动调整。"]],
		capTitle: "Codex 里的完整视频制作工具箱。",
		caps: [["时间线", "检查项目、验证编辑、应用语义操作并查看 revision 历史。"], ["文字稿与字幕", "转录、读取脚本、删除词语或停顿，并批量调整同步字幕。"], ["动态图形", "使用安全 MG DSL 创建可编辑 lower third、标题卡、callout 和图表。"], ["素材生成", "列出 Provider，并把图片、B-roll、旁白、音乐或 SFX 生成进本地媒体库。"], ["音频", "创建可撤销降噪与处理派生资产，不覆盖源素材。"], ["交付", "渲染预览帧、验证项目、启动固定 revision 导出并跟踪持久任务。"]],
		securityTitle: "默认留在本地，外发必须明确。",
		securityBody: "Bridge 只读取受保护的 runtime descriptor 和 daemon token，不访问 SQLite、项目 JSON、Codex 认证文件或 Provider 密钥。付费生成和素材外发始终需要审批。",
		cta: "安装 OpenChatCut Codex 插件", note: "需要本地 OpenChatCut daemon、Node.js 20+ 和已登录的 Codex CLI。",
	},
};

export function PluginPage({ locale }: { locale: Locale }) {
	const c = pluginCopy[locale];
	return <div className="plugin-shell">
		<header className="site-nav"><div className="nav-inner"><Brand href={c.home}/><nav className="nav-links"><Link href={c.home}>Product</Link><span className="active-tab">Codex Plugin</span><Link href={c.docs}>Docs</Link></nav><div className="nav-actions"><Link className="nav-button hide-mobile" href={c.alt}>{c.altLabel}</Link><a className="nav-button primary" href={repository}><TextScramble as="span" trigger={false} triggerOnHover>GitHub ↗</TextScramble></a></div></div></header>
		<main>
			<section className="plugin-hero"><div className="plugin-label">{c.label}</div><SlotText text={c.title} /><TextEffect as="p" preset="slide" delay={.25}>{c.lead}</TextEffect><div className="hero-actions"><a className="button primary" href="#install">{c.install} ↓</a><Link className="button secondary" href={c.docs}>{c.docsLabel} →</Link></div></section>
			<section className="plugin-terminal-wrap"><div className="plugin-terminal"><BorderTrail className="sora-trail" size={160} transition={{ duration: 6, ease: "linear", repeat: Number.POSITIVE_INFINITY }} /><div className="terminal-bar"><span>open-chat-cut / plugin</span><span>local stdio mcp</span></div><div className="terminal-grid"><div><div className="kicker">MCP BRIDGE</div><TextEffect as="h2" scrollTrigger preset="fade-in-blur">{c.terminalTitle}</TextEffect></div><pre>{c.terminalLines.map((line,i)=><code className={i > 1 ? "success" : ""} key={line}>{line}{"\n"}</code>)}</pre></div></div></section>
			<section className="section plugin-flow"><div className="section-heading"><div><div className="kicker">REVISION SAFE</div><MaskedTextReveal as="h2" splitBy={locale === "zh" ? "chars" : "lines"} text={c.flowTitle} /></div><p>{c.flowBody}</p></div><div className="plugin-steps">{c.steps.map(([n,title,body],i)=><article key={n}><span>{n}</span><TextEffect as="h3" scrollTrigger preset="slide" delay={i * .05}>{title}</TextEffect><p>{body}</p></article>)}</div></section>
			<section className="section"><div className="section-heading"><div><div className="kicker">25 MCP TOOLS · 10 SKILLS</div><TextEffect as="h2" scrollTrigger preset="fade-in-blur">{c.capTitle}</TextEffect></div></div><div className="plugin-cap-grid">{c.caps.map(([title,body],i)=><article key={title}><span>0{i+1}</span><TextEffect as="h3" scrollTrigger preset="slide" delay={(i % 3) * .05}>{title}</TextEffect><p>{body}</p></article>)}</div></section>
			<section className="plugin-security"><div><div className="kicker">SECURITY MODEL</div><h2>{c.securityTitle}</h2></div><p>{c.securityBody}</p></section>
			<section className="plugin-install" id="install"><div className="plugin-label">SOURCE-AVAILABLE WORKFLOW · LOCAL RUNTIME</div><TextEffect as="h2" scrollTrigger preset="fade-in-blur">{c.cta}</TextEffect><pre><code>git clone https://github.com/NeuraSea/open-chat-cut.git{"\n"}cd open-chat-cut{"\n"}./scripts/install.sh</code></pre><p>{c.note}</p><Link className="button secondary" href={c.docs}>{c.docsLabel} →</Link></section>
		</main>
		<footer className="site-footer"><span>© 2026 NeuraSea · OpenChatCut</span><span>Codex plugin · local daemon · revision-safe operations</span></footer>
	</div>;
}
