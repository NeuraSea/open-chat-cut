# OpenChatCut Fumadocs site

This Next.js + Fumadocs app generates the bilingual marketing site and product
documentation as a static export for Cloudflare Pages. It deliberately does
not expose the local editor or daemon as a public SaaS service.

- English: `/`
- Simplified Chinese: `/zh/`
- Codex plugin: `/codex-plugin/`
- Codex 插件: `/zh/codex-plugin/`
- English docs: `/docs/`
- Simplified Chinese docs: `/zh/docs/`
- Production domain: `https://open-chatcut.nervafs.xyz`

Build and preview:

```bash
cd apps/landing
npm run build
npx wrangler pages dev out --port 3111
```

Deploy after authenticating Wrangler:

```bash
npm run deploy
```

The static site is emitted to `out/`. Fumadocs MDX content lives under
`content/docs/`, and the real editor image is copied from the deterministic
capture maintained by `apps/web/scripts/capture-landing-editor-preview.mjs`.
