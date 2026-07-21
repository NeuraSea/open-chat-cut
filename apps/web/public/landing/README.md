# Landing editor capture

`openchatcut-editor-fixture.png` is a static capture of the real OpenChatCut Web
editor, not a hand-built UI mock. It uses the deterministic motion-graphics and
audio acceptance fixture so the repository does not publish private user media.

To regenerate it, start the daemon and Web editor, load the acceptance fixture,
and launch Chrome with a debugging port. From `apps/web`, run:

```bash
node scripts/capture-landing-editor-preview.mjs \
  --cdp-url http://127.0.0.1:9229 \
  --editor-url http://127.0.0.1:3100/editor/ACCEPTANCE_FIXTURE_PROJECT_ID
```

The script fixes the viewport at 1600×1000, selects the dark theme, suppresses
onboarding, waits for the actual editor shell, and captures the rendered page.
