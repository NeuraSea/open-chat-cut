import Link from "next/link";
import { copy, repository, type Locale } from "@/lib/copy";

export function Brand({ href = "/" }: { href?: string }) {
	return <Link className="brand" href={href}><span className="brand-mark" aria-hidden="true" />OpenChatCut</Link>;
}

export function LandingPage({ locale }: { locale: Locale }) {
	const c = copy[locale];
	const creator = `${repository}/issues/new?template=creator_license.yml`;
	return <div className="landing-shell">
		<header className="site-nav"><div className="nav-inner">
			<Brand href={locale === "zh" ? "/zh/" : "/"} />
			<nav className="nav-links" aria-label="Primary"><a href="#workflow">{c.nav[0]}</a><a href="#local">{c.nav[1]}</a><a href="#pricing">{c.nav[2]}</a></nav>
			<div className="nav-actions"><Link className="nav-button hide-mobile" href={c.altHref}>{c.altLabel}</Link><Link className="nav-button hide-mobile" href={c.docsHref}>Docs</Link><a className="nav-button primary" href={repository}>GitHub ↗</a></div>
		</div></header>

		<main>
			<section className="hero"><div className="hero-content">
				<div className="hero-label">OpenChatCut</div>
				<h1>{c.headline.split(" ").slice(0, -1).join(" ")} <span className="gradient">{c.headline.split(" ").slice(-1)}</span></h1>
				<p className="hero-lead">{c.lead}</p>
				<div className="hero-actions"><a className="button primary" href={repository}>{c.primary} ↗</a><Link className="button secondary" href={c.docsHref}>{c.secondary} →</Link></div>
				<div className="hero-note">{c.note}</div>
			</div></section>

			<div className="editor-stage"><div className="editor-window"><div className="window-bar"><span className="window-dots"><i/><i/><i/></span><span>launch-film.occ · revision 31</span><span><i className="status-dot"/>saved locally</span></div><img src="/editor.png" width="1600" height="1000" alt="OpenChatCut editor showing transcript, preview, timeline and Agent plan" /></div>
				<div className="floating-card one"><strong><i className="status-dot"/>Agent plan ready</strong><span>4 operations · revision safe</span></div>
				<div className="floating-card two"><strong>Timeline remains editable</strong><span>captions · MG · B-roll · audio</span></div>
			</div>

			<div className="proof-strip">{c.proof.map(([value,label]) => <div className="proof-item" key={label}><strong>{value}</strong><span>{label}</span></div>)}</div>

			<section className="section" id="workflow"><div className="section-heading"><div><div className="kicker">{c.sectionKicker}</div><h2>{c.sectionTitle}</h2></div><p>{c.sectionBody}</p></div>
				<div className="feature-grid">{c.features.map(([icon,title,body],i) => <article className="feature-card" key={title}><span className="feature-index">0{i+1}</span><div className="feature-icon">{icon}</div><h3>{title}</h3><p>{body}</p></article>)}</div>
			</section>

			<section className="section" id="local"><div className="local-band"><div className="kicker">{c.localKicker}</div><h2>{c.localTitle}</h2><p>{c.localBody}</p><div className="pipeline">{c.pipeline.map(([n,label]) => <div key={n}><small>{n}</small><strong>{label}</strong></div>)}</div></div></section>

			<section className="section" id="pricing"><div className="section-heading"><div><div className="kicker">{c.pricingKicker}</div><h2>{c.pricingTitle}</h2></div></div><div className="pricing-grid">{c.plans.map(([name,price,detail,items,cta],i) => {
				const href = i === 0 ? `${repository}/blob/main/LICENSE` : i === 1 ? creator : `${repository}/issues/new?title=Commercial%20license%20request`;
				return <article className={`price-card ${i === 1 ? "featured" : ""}`} key={name as string}><div className="plan">{name as string}</div><div className="price">{price as string}</div><p>{detail as string}</p><ul>{(items as string[]).map(item => <li key={item}>{item}</li>)}</ul><a className="button secondary" href={href}>{cta as string} →</a></article>;
			})}</div></section>

			<section className="final-cta"><h2>{c.ctaTitle}</h2><p>{c.ctaBody}</p><div className="hero-actions"><a className="button primary" href={repository}>GitHub ↗</a><Link className="button secondary" href={c.docsHref}>Docs →</Link></div></section>
		</main>
		<footer className="site-footer"><span>© 2026 NeuraSea · OpenChatCut</span><span>{c.legal}</span></footer>
	</div>;
}
