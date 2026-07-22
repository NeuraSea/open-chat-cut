---
name: motion-graphics
description: "Create editable OpenChatCut motion graphics. Use for lower thirds, title cards, charts, callouts, logo reveals, CTAs, end cards, animated text or shapes, safe motion-graphics DSL authoring, or explicitly requested sandboxed JSX."
---

# Motion Graphics

## Prefer the safe DSL

1. Read the project to learn canvas, frame rate, theme, tracks, and current revision.
2. Build a versioned DSL definition from text, shapes, paths, charts, media, groups, keyframes, easing, and stagger.
3. Keep all resource references managed and all animation deterministic by timeline time.
4. Call `create_motion_graphic` with `mode: dsl`, explicit insertion time/duration, current revision, and a new idempotency key.

## Motion-first built-ins

- Use `motion-slot-title` for the rolling, clipped editorial title treatment used by the OpenChatCut site.
- Use `motion-mask-lower-third` for a block-wipe lower third with masked text entry.
- Both are five-second editable DSL templates. They use timeline keyframes and safe group clipping, not React code at playback time, so preview and export remain deterministic.

## Advanced JSX

- Use `mode: jsx` only when the user explicitly requests advanced scripted graphics that the DSL cannot express.
- Check `get_status.capabilities.motionGraphicJsx` first. If false, use the DSL or report the missing local compiler instead of attempting an out-of-band execution.
- Do not include network, filesystem, dynamic module loading, process access, timers detached from the timeline, or secrets.
- The daemon AST-validates the source and stores bounded, non-executable safe IR. Preview and export interpret that IR through the same capability-free renderer; raw JSX is never evaluated.
- Treat AST or safe-IR rejection as final until the definition is revised; never bypass the daemon validator.

## Verify

Re-read the project, run `validate_project`, and render start/middle/end plus transition frames. Confirm safe areas, legibility, animation bounds, and that preview/export use the same runtime. Report `CAPABILITY_UNAVAILABLE` honestly.
