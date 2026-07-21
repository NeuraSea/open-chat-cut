import { describe, expect, test } from "bun:test";
import { validateMotionGraphicDsl } from "../src/dsl.mjs";
import { compileMotionGraphicJsx, sandboxDocument, validateMotionGraphicJsx } from "../src/jsx-policy.mjs";

describe("motion graphic safety", () => {
  test("accepts the versioned editable DSL", () => {
    const value = validateMotionGraphicDsl({
      version: 1,
      width: 1920,
      height: 1080,
      durationSeconds: 4,
      nodes: [{
        id: "title",
        type: "text",
        text: "Launch day",
        animations: { opacity: [{ time: 0, value: 0 }, { time: 0.4, value: 1, easing: "ease-out" }] },
      }],
    });
    expect(value.stats).toEqual({ nodes: 1, keyframes: 2 });
  });

  test("rejects duplicate ids and resource exhaustion", () => {
    expect(() => validateMotionGraphicDsl({
      version: 1, width: 100, height: 100, durationSeconds: 1,
      nodes: [{ id: "same", type: "shape" }, { id: "same", type: "text" }],
    })).toThrow("unique");
  });

  test.each([
    "export default () => <img src=\"https://attacker.invalid/a.png\" />",
    "export default props => <img src={props.src} />",
    "export default props => <div {...props} />",
    "export default () => <div dangerouslySetInnerHTML={{__html: '<img>'}} />",
    "export default () => <iframe srcDoc=\"<script>void 0</script>\" />",
    "export default () => <Evil />",
    "export default () => { fetch('https://attacker.invalid'); return <div /> }",
    "export default () => <div>{globalThis['process']}</div>",
    "import fs from 'node:fs'; export default () => <div />",
    "export default () => { while(true){} }",
    "export default () => { for (;;) {} }",
    "export default () => new Function('return process')()",
    "const interpolate = () => 1; export default () => <div>{interpolate()}</div>",
    "export default () => <div style={{constructor: 'pollute'}} />",
  ])("rejects malicious JSX: %s", (source) => {
    expect(() => validateMotionGraphicJsx(source)).toThrow();
  });

  test("accepts bounded Remotion-style expressions", () => {
    const result = validateMotionGraphicJsx(`
      export default function Title() {
        const frame = useCurrentFrame();
        const opacity = interpolate(frame, [0, 12], [0, 1]);
        return <div style={{ opacity }}>Hello</div>;
      }
    `);
    expect(result.stats.astNodes).toBeGreaterThan(10);
  });

  test("compiles approved JSX into deterministic non-executable IR", () => {
    const result = compileMotionGraphicJsx(`
      export default function Title() {
        const frame = useCurrentFrame();
        const opacity = interpolate(frame, [0, 12], [0, 1]);
        return <AbsoluteFill style={{backgroundColor: "#112233", opacity}}><div>Hello</div></AbsoluteFill>;
      }
    `, { width: 1920, height: 1080, durationSeconds: 2, fps: 30 });
    expect(result.ir.kind).toBe("jsxSafeIr");
    expect(result.ir.program.bindings.map((binding) => binding.name)).toEqual(["frame", "opacity"]);
    expect(result.ir.program.root.tag).toBe("AbsoluteFill");
    expect(result.security).toEqual(expect.objectContaining({
      sourceExecuted: false,
      networkAccess: "disabled",
      fileAccess: "disabled",
    }));
  });

  test("compiler rejects extra program statements even when individual AST nodes are safe", () => {
    expect(() => compileMotionGraphicJsx(
      "const label = 'unsafe extra scope'; export default () => <div>{label}</div>",
      { width: 1920, height: 1080, durationSeconds: 2, fps: 30 },
    )).toThrow("Program must contain only one");
  });

  test("sandbox CSP disables network and frame capabilities", () => {
    const html = sandboxDocument({ compiledSource: "void 0", nonce: "fixedNonce123456" });
    expect(html).toContain("connect-src 'none'");
    expect(html).toContain("worker-src 'none'");
    expect(html).toContain("frame-src 'none'");
    expect(html).toContain("form-action 'none'");
  });

  test("sandbox script escaping is case-insensitive and nonce injection is rejected", () => {
    const html = sandboxDocument({
      compiledSource: "const value = '</ScRiPt><script>bad()</script>';",
      nonce: "nonceValue123456",
    });
    expect(html).not.toContain("</ScRiPt");
    expect(html).toContain("<\\/script");
    expect(() => sandboxDocument({ compiledSource: "void 0", nonce: "x\"; fetch('bad')" })).toThrow("nonce");
  });

  test.each([
    {
      name: "URL-shaped managed asset id",
      node: { id: "media", type: "media", assetId: "http://attacker.invalid/a" },
    },
    {
      name: "unknown resource property",
      node: { id: "shape", type: "shape", src: "file:///etc/passwd" },
    },
    {
      name: "prototype-chain animation path",
      node: { id: "shape", type: "shape", animations: { "constructor.prototype.opacity": [{ time: 0, value: 1 }] } },
    },
    {
      name: "external fill paint server",
      node: { id: "shape", type: "shape", fill: "url(https://attacker.invalid/fill.svg)" },
    },
  ])("rejects unsafe DSL: $name", ({ node }) => {
    expect(() => validateMotionGraphicDsl({
      version: 1,
      width: 1920,
      height: 1080,
      durationSeconds: 1,
      nodes: [node],
    })).toThrow();
  });
});
