# OpenChatCut static landing site

This app generates the bilingual, marketing-only static site for Cloudflare
Pages. It deliberately does not expose the local editor or daemon as a public
SaaS service.

- English: `/`
- Simplified Chinese: `/zh/`
- Production domain: `https://open-chatcut.nervafs.xyz`

Build and preview:

```bash
cd apps/landing
npm run build
npx wrangler pages dev dist --port 3111
```

Deploy after authenticating Wrangler:

```bash
npm run deploy
```

The real editor image is copied from the deterministic capture maintained by
`apps/web/scripts/capture-landing-editor-preview.mjs`.
