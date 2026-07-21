#!/usr/bin/env node
import { validateMotionGraphicDsl, MotionGraphicValidationError } from "./dsl.mjs";
import { compileMotionGraphicJsx } from "./jsx-policy.mjs";

let inputBytes = 0;
const inputChunks = [];
for await (const chunk of process.stdin) {
  inputBytes += chunk.length;
  if (inputBytes > 1024 * 1024) {
    throw new MotionGraphicValidationError("MG_INPUT_LIMIT", "Motion graphic request is too large");
  }
  inputChunks.push(chunk);
}
const input = JSON.parse(Buffer.concat(inputChunks).toString("utf8"));
try {
  const result = input.mode === "jsx"
    ? compileMotionGraphicJsx(input.definition, input.context)
    : validateMotionGraphicDsl(input.definition);
  process.stdout.write(`${JSON.stringify({ ok: true, result })}\n`);
} catch (error) {
  const payload = error instanceof MotionGraphicValidationError
    ? { code: error.code, message: error.message, path: error.path }
    : { code: "MG_VALIDATION_FAILED", message: error instanceof Error ? error.message : String(error) };
  process.stdout.write(`${JSON.stringify({ ok: false, error: payload })}\n`);
  process.exitCode = 1;
}
