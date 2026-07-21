import { parse } from "@babel/parser";
import { MotionGraphicValidationError } from "./dsl.mjs";

const DENIED_IDENTIFIERS = new Set([
  "fetch", "XMLHttpRequest", "WebSocket", "EventSource", "Worker", "SharedWorker",
  "navigator", "location", "document", "window", "globalThis", "process", "require",
  "module", "exports", "eval", "Function", "AsyncFunction", "WebAssembly", "Deno", "Bun",
  "localStorage", "sessionStorage", "indexedDB", "caches", "crypto",
]);
const DENIED_NODE_TYPES = new Set([
  "ImportDeclaration", "ImportExpression", "ExportAllDeclaration", "NewExpression",
  "WithStatement", "DebuggerStatement", "AwaitExpression", "YieldExpression",
  "WhileStatement", "DoWhileStatement", "ForStatement", "ForInStatement", "ForOfStatement",
  "TryStatement", "ThrowStatement", "ClassDeclaration", "ClassExpression",
  "AssignmentExpression", "UpdateExpression", "ObjectMethod", "TaggedTemplateExpression",
  "OptionalCallExpression", "OptionalMemberExpression", "SpreadElement", "RestElement",
  "JSXSpreadAttribute",
]);
const SAFE_CALLS = new Set([
  "interpolate", "spring", "sequence", "clamp", "useCurrentFrame", "useVideoConfig",
  "Math.abs", "Math.ceil", "Math.cos", "Math.floor", "Math.max", "Math.min", "Math.pow",
  "Math.round", "Math.sin", "Math.sqrt", "Math.tan",
]);
const SAFE_INTRINSIC_ELEMENTS = new Set([
  "div", "span", "p", "strong", "em", "img", "video", "audio",
  "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline",
  "polygon", "text", "tspan", "defs", "linearGradient", "radialGradient", "stop",
  "clipPath", "mask",
]);
const SAFE_COMPONENTS = new Set(["AbsoluteFill", "Sequence", "Img", "Video", "Audio"]);
const PROTECTED_BINDINGS = new Set([
  "Math",
  ...[...SAFE_CALLS].filter((name) => !name.includes(".")),
  ...SAFE_COMPONENTS,
]);
const DENIED_JSX_ATTRIBUTES = new Set([
  "dangerouslySetInnerHTML", "srcDoc", "srcSet", "action", "formAction", "poster",
]);
const RESOURCE_ATTRIBUTES = new Set(["src", "href", "xlinkHref"]);
const SAFE_RESOURCE = /^(?:asset:[A-Za-z0-9._:-]{1,256}|#[A-Za-z][A-Za-z0-9._:-]{0,127})$/;
const MAX_SOURCE_BYTES = 256 * 1024;
const MAX_AST_NODES = 20_000;
const MAX_AST_DEPTH = 100;

function memberName(node) {
  if (node.type !== "MemberExpression" || node.computed) return null;
  if (node.object?.type !== "Identifier" || node.property?.type !== "Identifier") return null;
  return `${node.object.name}.${node.property.name}`;
}

function callName(node) {
  if (node.type === "Identifier") return node.name;
  return memberName(node);
}

function rejectProtectedBinding(pattern, path) {
  if (!pattern || typeof pattern !== "object") return;
  if (pattern.type === "Identifier" && PROTECTED_BINDINGS.has(pattern.name)) {
    throw new MotionGraphicValidationError(
      "MG_JSX_SHADOWED_RUNTIME",
      `${pattern.name} is a protected runtime binding`,
      path,
    );
  }
  if (pattern.type === "AssignmentPattern") {
    rejectProtectedBinding(pattern.left, `${path}.left`);
  } else if (pattern.type === "ArrayPattern") {
    pattern.elements.forEach((element, index) => rejectProtectedBinding(element, `${path}.elements[${index}]`));
  } else if (pattern.type === "ObjectPattern") {
    pattern.properties.forEach((property, index) => {
      if (property.type === "ObjectProperty") rejectProtectedBinding(property.value, `${path}.properties[${index}].value`);
    });
  }
}

function jsxName(node) {
  return node?.type === "JSXIdentifier" ? node.name : null;
}

function walk(node, state, path = "Program", depth = 0) {
  if (!node || typeof node !== "object") return;
  state.nodes += 1;
  if (state.nodes > MAX_AST_NODES) {
    throw new MotionGraphicValidationError("MG_JSX_NODE_LIMIT", "Advanced motion graphic has too many AST nodes", path);
  }
  if (depth > MAX_AST_DEPTH) {
    throw new MotionGraphicValidationError("MG_JSX_DEPTH_LIMIT", "Advanced motion graphic is too deeply nested", path);
  }
  if (DENIED_NODE_TYPES.has(node.type)) {
    throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_SYNTAX", `${node.type} is not allowed`, path);
  }
  if (node.type === "Identifier" && DENIED_IDENTIFIERS.has(node.name)) {
    throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_GLOBAL", `${node.name} is not available in the sandbox`, path);
  }
  if (node.type === "MemberExpression" && node.computed) {
    throw new MotionGraphicValidationError("MG_JSX_COMPUTED_MEMBER", "Computed property access is not allowed", path);
  }
  if (node.type === "VariableDeclarator") rejectProtectedBinding(node.id, `${path}.id`);
  if (node.type === "FunctionDeclaration" || node.type === "FunctionExpression" || node.type === "ArrowFunctionExpression") {
    rejectProtectedBinding(node.id, `${path}.id`);
    node.params.forEach((parameter, index) => rejectProtectedBinding(parameter, `${path}.params[${index}]`));
  }
  if (node.type === "ObjectProperty") {
    if (node.computed) {
      throw new MotionGraphicValidationError("MG_JSX_COMPUTED_PROPERTY", "Computed object keys are not allowed", path);
    }
    const key = node.key?.type === "Identifier" || node.key?.type === "StringLiteral" ? node.key.name ?? node.key.value : null;
    if (["__proto__", "prototype", "constructor"].includes(key)) {
      throw new MotionGraphicValidationError("MG_JSX_PROTOTYPE_KEY", "Prototype-chain keys are not allowed", path);
    }
  }
  if (node.type === "CallExpression") {
    const name = callName(node.callee);
    if (!name || !SAFE_CALLS.has(name)) {
      throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_CALL", `Call is not allowlisted: ${name ?? "dynamic"}`, path);
    }
  }
  if (node.type === "JSXOpeningElement") {
    const name = jsxName(node.name);
    if (!name || (!SAFE_INTRINSIC_ELEMENTS.has(name) && !SAFE_COMPONENTS.has(name))) {
      throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_ELEMENT", `JSX element is not allowlisted: ${name ?? "dynamic"}`, path);
    }
  }
  if (node.type === "JSXAttribute") {
    const name = jsxName(node.name);
    if (!name) {
      throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_ATTRIBUTE", "Namespaced JSX attributes are not allowed", path);
    }
    if (DENIED_JSX_ATTRIBUTES.has(name) || /^on/i.test(name)) {
      throw new MotionGraphicValidationError("MG_JSX_FORBIDDEN_ATTRIBUTE", `${name} is not allowed`, path);
    }
    if (RESOURCE_ATTRIBUTES.has(name)) {
      if (node.value?.type !== "StringLiteral" || !SAFE_RESOURCE.test(node.value.value)) {
        throw new MotionGraphicValidationError(
          "MG_JSX_EXTERNAL_RESOURCE",
          "Resource attributes require a static managed asset or local SVG fragment",
          path,
        );
      }
    }
  }
  for (const [key, value] of Object.entries(node)) {
    if (["loc", "start", "end", "leadingComments", "trailingComments", "innerComments"].includes(key)) continue;
    if (Array.isArray(value)) value.forEach((child, index) => walk(child, state, `${path}.${key}[${index}]`, depth + 1));
    else if (value && typeof value === "object" && typeof value.type === "string") walk(value, state, `${path}.${key}`, depth + 1);
  }
}

function parseValidatedMotionGraphicJsx(source) {
  if (typeof source !== "string" || Buffer.byteLength(source, "utf8") > MAX_SOURCE_BYTES) {
    throw new MotionGraphicValidationError("MG_JSX_SOURCE_LIMIT", "Advanced motion graphic source is empty or too large");
  }
  let ast;
  try {
    ast = parse(source, {
      sourceType: "module",
      plugins: ["jsx", "typescript"],
      allowAwaitOutsideFunction: false,
      errorRecovery: false,
    });
  } catch (error) {
    throw new MotionGraphicValidationError("MG_JSX_PARSE_ERROR", error instanceof Error ? error.message : "Invalid JSX");
  }
  const state = { nodes: 0 };
  walk(ast, state);
  return { ast, state };
}

export function validateMotionGraphicJsx(source) {
  const { state } = parseValidatedMotionGraphicJsx(source);
  return { source, stats: { astNodes: state.nodes } };
}

const SAFE_BINARY_OPERATORS = new Set([
  "+", "-", "*", "/", "%", "**", "===", "!==", "<", "<=", ">", ">=",
]);
const SAFE_LOGICAL_OPERATORS = new Set(["&&", "||", "??"]);
const SAFE_UNARY_OPERATORS = new Set(["+", "-", "!"]);

function compilationError(code, message, path) {
  throw new MotionGraphicValidationError(code, message, path);
}

function propertyKey(node, path) {
  if (node?.type === "Identifier") return node.name;
  if (node?.type === "StringLiteral") return node.value;
  return compilationError("MG_JSX_UNSUPPORTED_PROPERTY", "Property key is not supported by the safe IR", path);
}

function compileExpression(node, state, path) {
  if (!node || typeof node !== "object") {
    return compilationError("MG_JSX_UNSUPPORTED_EXPRESSION", "Missing expression", path);
  }
  if (["TSAsExpression", "TSTypeAssertion", "TSNonNullExpression"].includes(node.type)) {
    return compileExpression(node.expression, state, `${path}.expression`);
  }
  switch (node.type) {
    case "StringLiteral":
    case "NumericLiteral":
    case "BooleanLiteral":
      return { kind: "literal", value: node.value };
    case "NullLiteral":
      return { kind: "literal", value: null };
    case "Identifier":
      if (!state.bindings.has(node.name) && node.name !== "Math") {
        return compilationError(
          "MG_JSX_UNKNOWN_BINDING",
          `Unknown safe-runtime binding: ${node.name}`,
          path,
        );
      }
      return { kind: "identifier", name: node.name };
    case "ArrayExpression":
      return {
        kind: "array",
        items: node.elements.map((item, index) => {
          if (!item) return { kind: "literal", value: null };
          return compileExpression(item, state, `${path}.elements[${index}]`);
        }),
      };
    case "ObjectExpression": {
      const entries = [];
      for (const [index, property] of node.properties.entries()) {
        if (property.type !== "ObjectProperty" || property.computed) {
          return compilationError(
            "MG_JSX_UNSUPPORTED_OBJECT",
            "Only static object properties are supported by the safe IR",
            `${path}.properties[${index}]`,
          );
        }
        const key = propertyKey(property.key, `${path}.properties[${index}].key`);
        entries.push([
          key,
          compileExpression(property.value, state, `${path}.properties[${index}].value`),
        ]);
      }
      return { kind: "object", entries };
    }
    case "UnaryExpression":
      if (!SAFE_UNARY_OPERATORS.has(node.operator)) {
        return compilationError("MG_JSX_UNSUPPORTED_OPERATOR", `Unary ${node.operator} is not supported`, path);
      }
      return {
        kind: "unary",
        operator: node.operator,
        argument: compileExpression(node.argument, state, `${path}.argument`),
      };
    case "BinaryExpression":
      if (!SAFE_BINARY_OPERATORS.has(node.operator)) {
        return compilationError("MG_JSX_UNSUPPORTED_OPERATOR", `Binary ${node.operator} is not supported`, path);
      }
      return {
        kind: "binary",
        operator: node.operator,
        left: compileExpression(node.left, state, `${path}.left`),
        right: compileExpression(node.right, state, `${path}.right`),
      };
    case "LogicalExpression":
      if (!SAFE_LOGICAL_OPERATORS.has(node.operator)) {
        return compilationError("MG_JSX_UNSUPPORTED_OPERATOR", `Logical ${node.operator} is not supported`, path);
      }
      return {
        kind: "logical",
        operator: node.operator,
        left: compileExpression(node.left, state, `${path}.left`),
        right: compileExpression(node.right, state, `${path}.right`),
      };
    case "ConditionalExpression":
      return {
        kind: "conditional",
        test: compileExpression(node.test, state, `${path}.test`),
        consequent: compileExpression(node.consequent, state, `${path}.consequent`),
        alternate: compileExpression(node.alternate, state, `${path}.alternate`),
      };
    case "MemberExpression":
      if (node.computed || node.property?.type !== "Identifier") {
        return compilationError("MG_JSX_COMPUTED_MEMBER", "Only static member access is supported", path);
      }
      return {
        kind: "member",
        object: compileExpression(node.object, state, `${path}.object`),
        property: node.property.name,
      };
    case "CallExpression": {
      const callee = callName(node.callee);
      if (!callee || !SAFE_CALLS.has(callee)) {
        return compilationError("MG_JSX_FORBIDDEN_CALL", "Call is not available in the safe IR", path);
      }
      return {
        kind: "call",
        callee,
        arguments: node.arguments.map((argument, index) =>
          compileExpression(argument, state, `${path}.arguments[${index}]`)),
      };
    }
    case "TemplateLiteral":
      return {
        kind: "template",
        quasis: node.quasis.map((quasi) => quasi.value.cooked ?? quasi.value.raw),
        expressions: node.expressions.map((expression, index) =>
          compileExpression(expression, state, `${path}.expressions[${index}]`)),
      };
    case "JSXElement":
    case "JSXFragment":
      return compileJsx(node, state, path);
    default:
      return compilationError(
        "MG_JSX_UNSUPPORTED_EXPRESSION",
        `${node.type} is not supported by the deterministic safe IR`,
        path,
      );
  }
}

function compileJsxChild(node, state, path) {
  if (node.type === "JSXText") {
    const value = node.value.replace(/\s+/gu, " ");
    return value.trim() ? { kind: "text", value } : null;
  }
  if (node.type === "JSXExpressionContainer") {
    if (node.expression?.type === "JSXEmptyExpression") return null;
    return { kind: "expression", expression: compileExpression(node.expression, state, `${path}.expression`) };
  }
  if (node.type === "JSXElement" || node.type === "JSXFragment") {
    return compileJsx(node, state, path);
  }
  return compilationError("MG_JSX_UNSUPPORTED_CHILD", `${node.type} is not a safe JSX child`, path);
}

function compileJsx(node, state, path) {
  if (node.type === "JSXFragment") {
    return {
      kind: "fragment",
      children: node.children
        .map((child, index) => compileJsxChild(child, state, `${path}.children[${index}]`))
        .filter(Boolean),
    };
  }
  const tag = jsxName(node.openingElement.name);
  if (!tag) {
    return compilationError("MG_JSX_FORBIDDEN_ELEMENT", "Dynamic JSX tags are not supported", path);
  }
  const attributes = [];
  for (const [index, attribute] of node.openingElement.attributes.entries()) {
    if (attribute.type !== "JSXAttribute") {
      return compilationError("MG_JSX_FORBIDDEN_ATTRIBUTE", "Spread attributes are not supported", `${path}.attributes[${index}]`);
    }
    const name = jsxName(attribute.name);
    if (!name) {
      return compilationError("MG_JSX_FORBIDDEN_ATTRIBUTE", "Namespaced attributes are not supported", `${path}.attributes[${index}]`);
    }
    let value;
    if (attribute.value === null) value = { kind: "literal", value: true };
    else if (attribute.value.type === "StringLiteral") value = { kind: "literal", value: attribute.value.value };
    else if (attribute.value.type === "JSXExpressionContainer") {
      value = compileExpression(attribute.value.expression, state, `${path}.attributes[${index}].value`);
    } else {
      return compilationError("MG_JSX_FORBIDDEN_ATTRIBUTE", "Attribute value is not supported", `${path}.attributes[${index}]`);
    }
    if (RESOURCE_ATTRIBUTES.has(name) && value.kind === "literal" && typeof value.value === "string" && value.value.startsWith("asset:")) {
      state.assetIds.add(value.value);
    }
    attributes.push([name, value]);
  }
  return {
    kind: "element",
    tag,
    attributes,
    children: node.children
      .map((child, index) => compileJsxChild(child, state, `${path}.children[${index}]`))
      .filter(Boolean),
  };
}

function compileComponent(declaration, state, path) {
  if (!["FunctionDeclaration", "FunctionExpression", "ArrowFunctionExpression"].includes(declaration.type)) {
    return compilationError("MG_JSX_DEFAULT_EXPORT", "Default export must be one bounded component function", path);
  }
  if (declaration.params.length !== 0) {
    return compilationError("MG_JSX_COMPONENT_PARAMS", "Safe motion graphic components do not accept runtime props", `${path}.params`);
  }
  const bindings = [];
  let root;
  if (declaration.body.type !== "BlockStatement") {
    root = compileExpression(declaration.body, state, `${path}.body`);
  } else {
    for (const [index, statement] of declaration.body.body.entries()) {
      const statementPath = `${path}.body.body[${index}]`;
      if (statement.type === "VariableDeclaration" && statement.kind === "const") {
        for (const [declarationIndex, item] of statement.declarations.entries()) {
          if (item.id.type !== "Identifier" || !item.init) {
            return compilationError("MG_JSX_UNSUPPORTED_BINDING", "Only initialized const identifiers are supported", `${statementPath}.declarations[${declarationIndex}]`);
          }
          const expression = compileExpression(item.init, state, `${statementPath}.declarations[${declarationIndex}].init`);
          state.bindings.add(item.id.name);
          bindings.push({ name: item.id.name, expression });
        }
      } else if (statement.type === "ReturnStatement" && statement.argument) {
        if (root) {
          return compilationError("MG_JSX_MULTIPLE_RETURNS", "Component must have one deterministic return", statementPath);
        }
        root = compileExpression(statement.argument, state, `${statementPath}.argument`);
      } else {
        return compilationError("MG_JSX_UNSUPPORTED_STATEMENT", `${statement.type} is not supported by the safe IR`, statementPath);
      }
    }
  }
  if (!root || !["element", "fragment"].includes(root.kind)) {
    return compilationError("MG_JSX_RETURN_REQUIRED", "Component must return JSX", path);
  }
  return { bindings, root };
}

export function compileMotionGraphicJsx(source, context) {
  const { ast, state: validationState } = parseValidatedMotionGraphicJsx(source);
  if (!context || typeof context !== "object") {
    throw new MotionGraphicValidationError("MG_JSX_CONTEXT_REQUIRED", "Compilation context is required");
  }
  const width = context.width;
  const height = context.height;
  const durationSeconds = context.durationSeconds;
  const fps = context.fps;
  if (![width, height, durationSeconds, fps].every((value) => typeof value === "number" && Number.isFinite(value))) {
    throw new MotionGraphicValidationError("MG_JSX_INVALID_CONTEXT", "Width, height, durationSeconds, and fps must be finite numbers");
  }
  if (width < 1 || width > 16384 || height < 1 || height > 16384 || durationSeconds <= 0 || durationSeconds > 3600 || fps <= 0 || fps > 240) {
    throw new MotionGraphicValidationError("MG_JSX_INVALID_CONTEXT", "Compilation context is outside safe bounds");
  }
  if (ast.program.body.length !== 1 || ast.program.body[0].type !== "ExportDefaultDeclaration") {
    throw new MotionGraphicValidationError("MG_JSX_PROGRAM_SHAPE", "Program must contain only one default component export", "Program.body");
  }
  const compileState = { bindings: new Set(), assetIds: new Set() };
  const program = compileComponent(ast.program.body[0].declaration, compileState, "Program.body[0].declaration");
  return {
    source,
    stats: { astNodes: validationState.nodes },
    assetIds: [...compileState.assetIds].sort(),
    ir: {
      version: 1,
      kind: "jsxSafeIr",
      width,
      height,
      durationSeconds,
      fps,
      program,
    },
    security: {
      sourceExecuted: false,
      interpreter: "deterministic-allowlisted-ir-v1",
      networkAccess: "disabled",
      fileAccess: "disabled",
      sandboxOrigin: "opaque",
    },
  };
}

export function sandboxDocument({ compiledSource, nonce }) {
  if (typeof compiledSource !== "string" || typeof nonce !== "string") {
    throw new TypeError("compiledSource and nonce are required");
  }
  if (!/^[A-Za-z0-9_-]{16,128}$/.test(nonce)) {
    throw new TypeError("nonce must be 16-128 URL-safe characters");
  }
  if (Buffer.byteLength(compiledSource, "utf8") > MAX_SOURCE_BYTES * 4) {
    throw new TypeError("compiledSource is too large");
  }
  const escapedSource = compiledSource.replace(/<\/script/giu, "<\\/script");
  return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; base-uri 'none'; object-src 'none'; script-src 'nonce-${nonce}'; img-src data: blob:; media-src data: blob:; style-src 'unsafe-inline'; connect-src 'none'; worker-src 'none'; frame-src 'none'; form-action 'none';"><div id="root"></div><script nonce="${nonce}">"use strict";Object.freeze(globalThis);${escapedSource}</script>`;
}
