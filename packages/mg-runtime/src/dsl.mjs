const NODE_KINDS = new Set(["text", "shape", "svg", "path", "chart", "media", "group"]);
const EASINGS = new Set([
  "linear",
  "ease-in",
  "ease-out",
  "ease-in-out",
  "back-in",
  "back-out",
  "elastic-out",
  "bounce-out",
]);
const ROOT_KEYS = new Set(["version", "width", "height", "durationSeconds", "nodes", "designStyle", "background"]);
const COMMON_NODE_KEYS = new Set([
  "id", "type", "name", "x", "y", "width", "height", "opacity", "rotation",
  "scale", "scaleX", "scaleY", "anchorX", "anchorY", "visible", "blendMode",
  "stagger", "animations", "children",
]);
const NODE_KEYS = Object.freeze({
  text: new Set(["text", "fontFamily", "fontSize", "fontWeight", "fontStyle", "textAlign", "lineHeight", "letterSpacing", "color", "maxWidth"]),
  shape: new Set(["shape", "fill", "stroke", "strokeWidth", "borderRadius"]),
  svg: new Set(["viewBox", "pathData", "fill", "stroke", "strokeWidth", "fillRule"]),
  path: new Set(["pathData", "fill", "stroke", "strokeWidth", "fillRule", "trimStart", "trimEnd"]),
  chart: new Set(["chartType", "data", "labels", "colors", "min", "max", "showLegend", "showAxes"]),
  media: new Set(["assetId", "fit", "volume", "muted", "playbackRate"]),
  group: new Set(["layout", "gap", "clip", "maskId"]),
});
const NUMERIC_NODE_KEYS = new Set([
  "x", "y", "width", "height", "opacity", "rotation", "scale", "scaleX", "scaleY",
  "anchorX", "anchorY", "fontSize", "fontWeight", "lineHeight", "letterSpacing",
  "maxWidth", "strokeWidth", "borderRadius", "trimStart", "trimEnd", "min", "max",
  "gap", "volume", "playbackRate",
]);
const ANIMATABLE_PROPERTIES = new Set([
  "x", "y", "width", "height", "opacity", "rotation", "scale", "scaleX", "scaleY",
  "anchorX", "anchorY", "fill", "stroke", "strokeWidth", "borderRadius", "fontSize",
  "lineHeight", "letterSpacing", "trimStart", "trimEnd", "volume",
]);
const FORBIDDEN_KEYS = new Set([
  "__proto__", "prototype", "constructor", "src", "srcSet", "href", "url", "uri",
  "poster", "action", "formAction", "html", "innerHTML", "dangerouslySetInnerHTML",
]);

export class MotionGraphicValidationError extends Error {
  constructor(code, message, path = "$") {
    super(message);
    this.name = "MotionGraphicValidationError";
    this.code = code;
    this.path = path;
  }
}

function assertRecord(value, path) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new MotionGraphicValidationError("MG_INVALID_OBJECT", "Expected an object", path);
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new MotionGraphicValidationError("MG_INVALID_OBJECT", "Objects must not use a custom prototype", path);
  }
}

function assertFinite(value, path) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new MotionGraphicValidationError("MG_INVALID_NUMBER", "Expected a finite number", path);
  }
}

function assertAllowedKeys(value, allowed, path) {
  for (const key of Object.keys(value)) {
    if (FORBIDDEN_KEYS.has(key) || !allowed.has(key)) {
      throw new MotionGraphicValidationError("MG_UNKNOWN_PROPERTY", `Unsupported property: ${key}`, `${path}.${key}`);
    }
  }
}

function assertNoResourceSyntax(value, path) {
  if (typeof value === "string" && /(?:url\s*\(|\b(?:https?|file|ftp|javascript):)/iu.test(value)) {
    throw new MotionGraphicValidationError("MG_EXTERNAL_RESOURCE", "External resource syntax is not allowed in MG styles", path);
  }
}

function validateDataValue(value, path, limits, state, depth = 0) {
  if (depth > 12) {
    throw new MotionGraphicValidationError("MG_DATA_DEPTH_LIMIT", "Structured MG data is too deeply nested", path);
  }
  state.dataValues += 1;
  if (state.dataValues > limits.maxDataValues) {
    throw new MotionGraphicValidationError("MG_DATA_LIMIT", "Motion graphic contains too much structured data", path);
  }
  if (value === null || typeof value === "boolean") return;
  if (typeof value === "number") {
    assertFinite(value, path);
    return;
  }
  if (typeof value === "string") {
    if (value.length > limits.maxStringLength) {
      throw new MotionGraphicValidationError("MG_STRING_LIMIT", "Structured data string is too long", path);
    }
    assertNoResourceSyntax(value, path);
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((item, index) => validateDataValue(item, `${path}[${index}]`, limits, state, depth + 1));
    return;
  }
  assertRecord(value, path);
  for (const [key, item] of Object.entries(value)) {
    if (FORBIDDEN_KEYS.has(key)) {
      throw new MotionGraphicValidationError("MG_UNSAFE_PROPERTY", `Unsafe structured-data key: ${key}`, `${path}.${key}`);
    }
    validateDataValue(item, `${path}.${key}`, limits, state, depth + 1);
  }
}

function validateKeyframes(keyframes, path, limits) {
  if (keyframes === undefined) return 0;
  if (!Array.isArray(keyframes) || keyframes.length > limits.maxKeyframesPerProperty) {
    throw new MotionGraphicValidationError("MG_KEYFRAME_LIMIT", "Invalid or excessive keyframes", path);
  }
  let previousTime = -Infinity;
  for (let index = 0; index < keyframes.length; index++) {
    const keyframe = keyframes[index];
    assertRecord(keyframe, `${path}[${index}]`);
    assertAllowedKeys(keyframe, new Set(["time", "value", "easing"]), `${path}[${index}]`);
    assertFinite(keyframe.time, `${path}[${index}].time`);
    if (keyframe.time < 0 || keyframe.time > limits.maxDurationSeconds || keyframe.time < previousTime) {
      throw new MotionGraphicValidationError(
        "MG_INVALID_KEYFRAME_TIME",
        "Keyframe times must be ordered and within the composition duration limit",
        `${path}[${index}].time`,
      );
    }
    previousTime = keyframe.time;
    if (keyframe.easing !== undefined && !EASINGS.has(keyframe.easing)) {
      throw new MotionGraphicValidationError("MG_INVALID_EASING", "Unknown easing", `${path}[${index}].easing`);
    }
    if (typeof keyframe.value === "string" && keyframe.value.length > limits.maxStringLength) {
      throw new MotionGraphicValidationError("MG_STRING_LIMIT", "Keyframe string is too long", `${path}[${index}].value`);
    }
    assertNoResourceSyntax(keyframe.value, `${path}[${index}].value`);
    validateDataValue(keyframe.value, `${path}[${index}].value`, limits, limits.state);
  }
  return keyframes.length;
}

function validateNode(node, path, state, limits, depth) {
  assertRecord(node, path);
  if (depth > limits.maxDepth) {
    throw new MotionGraphicValidationError("MG_DEPTH_LIMIT", "Motion graphic nesting is too deep", path);
  }
  if (!NODE_KINDS.has(node.type)) {
    throw new MotionGraphicValidationError("MG_UNKNOWN_NODE", `Unknown node type: ${String(node.type)}`, `${path}.type`);
  }
  assertAllowedKeys(node, new Set([...COMMON_NODE_KEYS, ...NODE_KEYS[node.type]]), path);
  if (typeof node.id !== "string" || !/^[A-Za-z0-9._:-]{1,128}$/.test(node.id)) {
    throw new MotionGraphicValidationError("MG_INVALID_ID", "Node id must be stable and portable", `${path}.id`);
  }
  if (state.ids.has(node.id)) {
    throw new MotionGraphicValidationError("MG_DUPLICATE_ID", "Node ids must be unique", `${path}.id`);
  }
  state.ids.add(node.id);
  state.nodes += 1;
  if (state.nodes > limits.maxNodes) {
    throw new MotionGraphicValidationError("MG_NODE_LIMIT", "Motion graphic has too many nodes", path);
  }
  for (const [key, value] of Object.entries(node)) {
    if (typeof value === "string" && value.length > limits.maxStringLength) {
      throw new MotionGraphicValidationError("MG_STRING_LIMIT", "Node string is too long", `${path}.${key}`);
    }
    if (NUMERIC_NODE_KEYS.has(key) && value !== undefined) assertFinite(value, `${path}.${key}`);
    if (["fill", "stroke", "color", "pathData", "viewBox"].includes(key)) assertNoResourceSyntax(value, `${path}.${key}`);
  }
  if (node.type === "media" && (typeof node.assetId !== "string" || !/^[A-Za-z0-9._-]{1,256}$/.test(node.assetId))) {
    throw new MotionGraphicValidationError("MG_INVALID_ASSET", "Media nodes must reference a managed asset id", `${path}.assetId`);
  }
  if (node.type === "chart" && node.data !== undefined) {
    validateDataValue(node.data, `${path}.data`, limits, state);
  }
  if (node.type === "chart" && node.labels !== undefined) {
    validateDataValue(node.labels, `${path}.labels`, limits, state);
  }
  if (node.type === "chart" && node.colors !== undefined) {
    validateDataValue(node.colors, `${path}.colors`, limits, state);
  }
  if (node.stagger !== undefined) {
    assertFinite(node.stagger, `${path}.stagger`);
    if (node.stagger < 0 || node.stagger > 10) {
      throw new MotionGraphicValidationError("MG_INVALID_STAGGER", "Stagger must be between 0 and 10 seconds", `${path}.stagger`);
    }
  }
  if (node.animations !== undefined) {
    assertRecord(node.animations, `${path}.animations`);
    for (const [property, keyframes] of Object.entries(node.animations)) {
      const segments = property.split(".");
      if (
        segments.length === 0 ||
        segments.some((segment) => !/^[A-Za-z][A-Za-z0-9]{0,40}$/.test(segment) || FORBIDDEN_KEYS.has(segment)) ||
        !ANIMATABLE_PROPERTIES.has(segments[0])
      ) {
        throw new MotionGraphicValidationError("MG_INVALID_PROPERTY", "Invalid animation property", `${path}.animations.${property}`);
      }
      state.keyframes += validateKeyframes(keyframes, `${path}.animations.${property}`, limits);
      if (state.keyframes > limits.maxKeyframes) {
        throw new MotionGraphicValidationError("MG_KEYFRAME_LIMIT", "Motion graphic has too many keyframes", path);
      }
    }
  }
  const children = node.children ?? [];
  if (!Array.isArray(children)) {
    throw new MotionGraphicValidationError("MG_INVALID_CHILDREN", "children must be an array", `${path}.children`);
  }
  children.forEach((child, index) => validateNode(child, `${path}.children[${index}]`, state, limits, depth + 1));
}

export const DEFAULT_DSL_LIMITS = Object.freeze({
  maxNodes: 500,
  maxDepth: 20,
  maxKeyframes: 5000,
  maxKeyframesPerProperty: 240,
  maxStringLength: 20_000,
  maxDataValues: 20_000,
  maxDurationSeconds: 60 * 60,
});

export function validateMotionGraphicDsl(value, options = {}) {
  const limits = { ...DEFAULT_DSL_LIMITS, ...options };
  const state = { nodes: 0, keyframes: 0, dataValues: 0, ids: new Set() };
  limits.state = state;
  assertRecord(value, "$");
  assertAllowedKeys(value, ROOT_KEYS, "$");
  if (value.version !== 1) {
    throw new MotionGraphicValidationError("MG_UNSUPPORTED_VERSION", "Only motion graphic DSL version 1 is supported", "$.version");
  }
  assertFinite(value.width, "$.width");
  assertFinite(value.height, "$.height");
  assertFinite(value.durationSeconds, "$.durationSeconds");
  if (value.designStyle !== undefined && (typeof value.designStyle !== "string" || !/^[A-Za-z0-9._-]{1,128}$/.test(value.designStyle))) {
    throw new MotionGraphicValidationError("MG_INVALID_STYLE", "designStyle must be a stable local style id", "$.designStyle");
  }
  if (value.background !== undefined) {
    if (typeof value.background !== "string" || value.background.length > limits.maxStringLength) {
      throw new MotionGraphicValidationError("MG_INVALID_BACKGROUND", "background must be a bounded color string", "$.background");
    }
    assertNoResourceSyntax(value.background, "$.background");
  }
  if (value.width < 1 || value.width > 8192 || value.height < 1 || value.height > 8192) {
    throw new MotionGraphicValidationError("MG_CANVAS_LIMIT", "Canvas dimensions must be between 1 and 8192", "$.");
  }
  if (value.durationSeconds <= 0 || value.durationSeconds > limits.maxDurationSeconds) {
    throw new MotionGraphicValidationError("MG_DURATION_LIMIT", "Composition duration is outside the allowed range", "$.durationSeconds");
  }
  if (!Array.isArray(value.nodes)) {
    throw new MotionGraphicValidationError("MG_INVALID_NODES", "nodes must be an array", "$.nodes");
  }
  value.nodes.forEach((node, index) => validateNode(node, `$.nodes[${index}]`, state, limits, 0));
  return {
    version: value.version,
    width: value.width,
    height: value.height,
    durationSeconds: value.durationSeconds,
    nodes: value.nodes,
    stats: { nodes: state.nodes, keyframes: state.keyframes },
  };
}
