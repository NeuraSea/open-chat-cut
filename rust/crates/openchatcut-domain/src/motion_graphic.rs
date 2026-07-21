use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

const NODE_KINDS: &[&str] = &["text", "shape", "svg", "path", "chart", "media", "group"];
const EASINGS: &[&str] = &[
    "linear",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "back-in",
    "back-out",
    "elastic-out",
    "bounce-out",
];
const ROOT_KEYS: &[&str] = &[
    "version",
    "width",
    "height",
    "durationSeconds",
    "nodes",
    "designStyle",
    "background",
];
const COMMON_NODE_KEYS: &[&str] = &[
    "id",
    "type",
    "name",
    "x",
    "y",
    "width",
    "height",
    "opacity",
    "rotation",
    "scale",
    "scaleX",
    "scaleY",
    "anchorX",
    "anchorY",
    "visible",
    "blendMode",
    "stagger",
    "animations",
    "children",
];
const FORBIDDEN_KEYS: &[&str] = &[
    "__proto__",
    "prototype",
    "constructor",
    "src",
    "srcSet",
    "href",
    "url",
    "uri",
    "poster",
    "action",
    "formAction",
    "html",
    "innerHTML",
    "dangerouslySetInnerHTML",
];
const NUMERIC_NODE_KEYS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "opacity",
    "rotation",
    "scale",
    "scaleX",
    "scaleY",
    "anchorX",
    "anchorY",
    "fontSize",
    "fontWeight",
    "lineHeight",
    "letterSpacing",
    "maxWidth",
    "strokeWidth",
    "borderRadius",
    "trimStart",
    "trimEnd",
    "min",
    "max",
    "gap",
    "volume",
    "playbackRate",
];
const ANIMATABLE_PROPERTIES: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "opacity",
    "rotation",
    "scale",
    "scaleX",
    "scaleY",
    "anchorX",
    "anchorY",
    "fill",
    "stroke",
    "strokeWidth",
    "borderRadius",
    "fontSize",
    "lineHeight",
    "letterSpacing",
    "trimStart",
    "trimEnd",
    "volume",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionGraphicLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_keyframes: usize,
    pub max_keyframes_per_property: usize,
    pub max_string_length: usize,
    pub max_data_values: usize,
    pub max_duration_seconds: u64,
}

impl Default for MotionGraphicLimits {
    fn default() -> Self {
        Self {
            max_nodes: 500,
            max_depth: 20,
            max_keyframes: 5_000,
            max_keyframes_per_property: 240,
            max_string_length: 20_000,
            max_data_values: 20_000,
            max_duration_seconds: 60 * 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{code} at {path}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct MotionGraphicValidationError {
    pub code: String,
    pub path: String,
    pub message: String,
}

impl MotionGraphicValidationError {
    fn new(code: &str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionGraphicValidationReport {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub duration_milliseconds: u64,
    pub node_count: usize,
    pub keyframe_count: usize,
    pub asset_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionGraphicTemplateDescriptor {
    pub id: String,
    pub name: String,
    pub category: String,
    pub definition: Value,
}

pub fn builtin_motion_graphic_templates() -> Vec<MotionGraphicTemplateDescriptor> {
    vec![
        template(
            "lower-third-signal",
            "Signal Lower Third",
            "lower-third",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 5,
                "designStyle": "signal-dark", "background": "#00000000",
                "nodes": [{
                    "id": "lower-third", "type": "group", "x": 96, "y": 790,
                    "width": 760, "height": 180, "stagger": 0.08,
                    "animations": { "x": [{"time": 0, "value": -820, "easing": "back-out"}, {"time": 0.55, "value": 96, "easing": "ease-out"}] },
                    "children": [
                        {"id": "accent", "type": "shape", "shape": "rectangle", "x": 0, "y": 0, "width": 14, "height": 180, "fill": "#55E6C1"},
                        {"id": "panel", "type": "shape", "shape": "rectangle", "x": 14, "y": 0, "width": 746, "height": 180, "fill": "#101418E8", "borderRadius": 18},
                        {"id": "name", "type": "text", "text": "Speaker Name", "x": 54, "y": 36, "fontFamily": "Inter", "fontSize": 58, "fontWeight": 760, "color": "#FFFFFF"},
                        {"id": "role", "type": "text", "text": "Role or context", "x": 56, "y": 112, "fontFamily": "Inter", "fontSize": 30, "fontWeight": 520, "color": "#A8B4BE"}
                    ]
                }]
            }),
        ),
        template(
            "title-card-editorial",
            "Editorial Title Card",
            "title-card",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 5,
                "designStyle": "editorial-ink", "background": "#F2EFE7",
                "nodes": [
                    {"id": "rule", "type": "shape", "shape": "rectangle", "x": 180, "y": 230, "width": 1560, "height": 8, "fill": "#D4512A", "animations": {"width": [{"time": 0, "value": 0, "easing": "ease-out"}, {"time": 0.7, "value": 1560, "easing": "ease-out"}]}},
                    {"id": "eyebrow", "type": "text", "text": "OPENCHATCUT / STORY", "x": 180, "y": 282, "fontFamily": "Inter", "fontSize": 28, "fontWeight": 700, "letterSpacing": 5, "color": "#D4512A"},
                    {"id": "headline", "type": "text", "text": "A clear title with\nan editorial rhythm", "x": 180, "y": 350, "fontFamily": "Georgia", "fontSize": 104, "fontWeight": 700, "lineHeight": 1.02, "color": "#161513", "animations": {"opacity": [{"time": 0.2, "value": 0, "easing": "ease-out"}, {"time": 1.0, "value": 1, "easing": "ease-out"}], "y": [{"time": 0.2, "value": 410, "easing": "ease-out"}, {"time": 1.0, "value": 350, "easing": "ease-out"}]}},
                    {"id": "deck", "type": "text", "text": "Subtitle, chapter, or framing statement", "x": 184, "y": 710, "fontFamily": "Inter", "fontSize": 34, "fontWeight": 450, "color": "#5C5851"}
                ]
            }),
        ),
        template(
            "data-chart-neon",
            "Neon Data Chart",
            "data-chart",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 6,
                "designStyle": "neon-grid", "background": "#07111D",
                "nodes": [
                    {"id": "chart-title", "type": "text", "text": "Quarterly momentum", "x": 150, "y": 100, "fontFamily": "Inter", "fontSize": 62, "fontWeight": 720, "color": "#EAF8FF"},
                    {"id": "chart", "type": "chart", "chartType": "bar", "x": 150, "y": 240, "width": 1620, "height": 650, "data": [38, 54, 71, 93], "labels": ["Q1", "Q2", "Q3", "Q4"], "colors": ["#2DD4BF", "#38BDF8", "#818CF8", "#F472B6"], "min": 0, "max": 100, "showLegend": false, "showAxes": true, "animations": {"opacity": [{"time": 0.2, "value": 0, "easing": "ease-out"}, {"time": 0.9, "value": 1, "easing": "ease-out"}]}},
                    {"id": "chart-note", "type": "text", "text": "+145% since Q1", "x": 1400, "y": 930, "fontFamily": "Inter", "fontSize": 34, "fontWeight": 700, "color": "#2DD4BF"}
                ]
            }),
        ),
        template(
            "callout-focus",
            "Focus Callout",
            "callout",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 4,
                "designStyle": "focus-yellow", "background": "#00000000",
                "nodes": [
                    {"id": "leader", "type": "path", "pathData": "M 360 720 C 520 650 600 570 720 480", "fill": "#00000000", "stroke": "#FFD43B", "strokeWidth": 10, "trimStart": 0, "trimEnd": 1, "animations": {"trimEnd": [{"time": 0, "value": 0, "easing": "ease-out"}, {"time": 0.65, "value": 1, "easing": "ease-out"}]}},
                    {"id": "target", "type": "shape", "shape": "ellipse", "x": 680, "y": 430, "width": 92, "height": 92, "fill": "#FFD43B33", "stroke": "#FFD43B", "strokeWidth": 8, "animations": {"scale": [{"time": 0.45, "value": 0, "easing": "back-out"}, {"time": 0.9, "value": 1, "easing": "back-out"}]}},
                    {"id": "callout-panel", "type": "shape", "shape": "rectangle", "x": 130, "y": 720, "width": 660, "height": 190, "fill": "#111827F2", "borderRadius": 24},
                    {"id": "callout-text", "type": "text", "text": "Draw attention to\nthe important detail", "x": 180, "y": 765, "fontFamily": "Inter", "fontSize": 46, "fontWeight": 680, "lineHeight": 1.1, "color": "#FFFFFF"}
                ]
            }),
        ),
        template(
            "logo-reveal-orbit",
            "Orbit Logo Reveal",
            "logo-reveal",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 4,
                "designStyle": "orbit-blue", "background": "#06101C",
                "nodes": [
                    {"id": "orbit-a", "type": "shape", "shape": "ellipse", "x": 710, "y": 290, "width": 500, "height": 500, "fill": "#00000000", "stroke": "#38BDF8", "strokeWidth": 5, "animations": {"rotation": [{"time": 0, "value": -120, "easing": "ease-out"}, {"time": 1.4, "value": 0, "easing": "ease-out"}], "opacity": [{"time": 0, "value": 0, "easing": "ease-out"}, {"time": 0.5, "value": 1, "easing": "ease-out"}]}},
                    {"id": "orbit-b", "type": "shape", "shape": "ellipse", "x": 765, "y": 345, "width": 390, "height": 390, "fill": "#00000000", "stroke": "#A78BFA", "strokeWidth": 8, "animations": {"rotation": [{"time": 0, "value": 160, "easing": "ease-out"}, {"time": 1.4, "value": 0, "easing": "ease-out"}]}},
                    {"id": "logo-mark", "type": "svg", "x": 870, "y": 450, "width": 180, "height": 180, "viewBox": "0 0 100 100", "pathData": "M15 20 H85 V38 H52 V80 H33 V38 H15 Z", "fill": "#FFFFFF", "animations": {"scale": [{"time": 0.5, "value": 0, "easing": "back-out"}, {"time": 1.25, "value": 1, "easing": "back-out"}]}},
                    {"id": "logo-name", "type": "text", "text": "YOUR BRAND", "x": 760, "y": 830, "fontFamily": "Inter", "fontSize": 44, "fontWeight": 720, "letterSpacing": 9, "color": "#DDEEFF", "animations": {"opacity": [{"time": 1.0, "value": 0, "easing": "ease-out"}, {"time": 1.7, "value": 1, "easing": "ease-out"}]}}
                ]
            }),
        ),
        template(
            "cta-pill",
            "Action Pill",
            "cta",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 5,
                "designStyle": "cta-coral", "background": "#00000000",
                "nodes": [{
                    "id": "cta", "type": "group", "x": 510, "y": 780, "width": 900, "height": 150,
                    "animations": {"scale": [{"time": 0, "value": 0.75, "easing": "back-out"}, {"time": 0.65, "value": 1, "easing": "back-out"}], "opacity": [{"time": 0, "value": 0, "easing": "ease-out"}, {"time": 0.4, "value": 1, "easing": "ease-out"}]},
                    "children": [
                        {"id": "cta-shadow", "type": "shape", "shape": "rectangle", "x": 12, "y": 14, "width": 876, "height": 136, "fill": "#00000055", "borderRadius": 68},
                        {"id": "cta-body", "type": "shape", "shape": "rectangle", "x": 0, "y": 0, "width": 876, "height": 136, "fill": "#FF6B5E", "borderRadius": 68},
                        {"id": "cta-label", "type": "text", "text": "Start creating today  →", "x": 130, "y": 42, "fontFamily": "Inter", "fontSize": 46, "fontWeight": 760, "color": "#FFFFFF"}
                    ]
                }]
            }),
        ),
        template(
            "end-card-modular",
            "Modular End Card",
            "end-card",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 7,
                "designStyle": "modular-slate", "background": "#111827",
                "nodes": [
                    {"id": "end-title", "type": "text", "text": "Keep watching", "x": 130, "y": 110, "fontFamily": "Inter", "fontSize": 74, "fontWeight": 780, "color": "#FFFFFF"},
                    {"id": "video-a", "type": "shape", "shape": "rectangle", "x": 130, "y": 260, "width": 760, "height": 430, "fill": "#263449", "stroke": "#475569", "strokeWidth": 4, "borderRadius": 22},
                    {"id": "video-b", "type": "shape", "shape": "rectangle", "x": 1030, "y": 260, "width": 760, "height": 430, "fill": "#263449", "stroke": "#475569", "strokeWidth": 4, "borderRadius": 22},
                    {"id": "next-a", "type": "text", "text": "Next story", "x": 160, "y": 730, "fontFamily": "Inter", "fontSize": 34, "fontWeight": 650, "color": "#CBD5E1"},
                    {"id": "next-b", "type": "text", "text": "Recommended", "x": 1060, "y": 730, "fontFamily": "Inter", "fontSize": 34, "fontWeight": 650, "color": "#CBD5E1"},
                    {"id": "subscribe", "type": "shape", "shape": "ellipse", "x": 850, "y": 820, "width": 220, "height": 120, "fill": "#22C55E", "animations": {"scale": [{"time": 0.8, "value": 0, "easing": "back-out"}, {"time": 1.4, "value": 1, "easing": "back-out"}]}},
                    {"id": "subscribe-label", "type": "text", "text": "FOLLOW", "x": 884, "y": 858, "fontFamily": "Inter", "fontSize": 34, "fontWeight": 800, "color": "#052E16"}
                ]
            }),
        ),
        template(
            "stat-card-mono",
            "Monochrome Stat Card",
            "design-style",
            json!({
                "version": 1, "width": 1920, "height": 1080, "durationSeconds": 5,
                "designStyle": "mono-precision", "background": "#FAFAF7",
                "nodes": [
                    {"id": "index", "type": "text", "text": "01 / KEY METRIC", "x": 160, "y": 150, "fontFamily": "Inter", "fontSize": 26, "fontWeight": 720, "letterSpacing": 4, "color": "#6B6B66"},
                    {"id": "stat", "type": "text", "text": "93%", "x": 150, "y": 280, "fontFamily": "Georgia", "fontSize": 260, "fontWeight": 700, "color": "#11110F", "animations": {"opacity": [{"time": 0, "value": 0, "easing": "ease-out"}, {"time": 0.8, "value": 1, "easing": "ease-out"}], "y": [{"time": 0, "value": 360, "easing": "ease-out"}, {"time": 0.8, "value": 280, "easing": "ease-out"}]}},
                    {"id": "stat-rule", "type": "shape", "shape": "rectangle", "x": 160, "y": 650, "width": 1600, "height": 5, "fill": "#11110F"},
                    {"id": "stat-copy", "type": "text", "text": "A concise explanation that makes the number meaningful.", "x": 165, "y": 710, "fontFamily": "Inter", "fontSize": 42, "fontWeight": 480, "maxWidth": 1100, "color": "#3D3D38"}
                ]
            }),
        ),
    ]
}

pub fn builtin_motion_graphic_template(id: &str) -> Option<MotionGraphicTemplateDescriptor> {
    builtin_motion_graphic_templates()
        .into_iter()
        .find(|template| template.id == id)
}

fn template(
    id: &str,
    name: &str,
    category: &str,
    definition: Value,
) -> MotionGraphicTemplateDescriptor {
    MotionGraphicTemplateDescriptor {
        id: id.to_owned(),
        name: name.to_owned(),
        category: category.to_owned(),
        definition,
    }
}

#[derive(Default)]
struct ValidationState {
    ids: HashSet<String>,
    asset_ids: BTreeSet<String>,
    nodes: usize,
    keyframes: usize,
    data_values: usize,
}

fn fail<T>(
    code: &str,
    path: impl Into<String>,
    message: impl Into<String>,
) -> Result<T, MotionGraphicValidationError> {
    Err(MotionGraphicValidationError::new(code, path, message))
}

fn object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a Map<String, Value>, MotionGraphicValidationError> {
    value.as_object().ok_or_else(|| {
        MotionGraphicValidationError::new("MG_INVALID_OBJECT", path, "Expected an object")
    })
}

fn finite_number(value: Option<&Value>, path: &str) -> Result<f64, MotionGraphicValidationError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            MotionGraphicValidationError::new("MG_INVALID_NUMBER", path, "Expected a finite number")
        })
}

fn has_resource_syntax(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("url(")
        || ["http:", "https:", "file:", "ftp:", "javascript:"]
            .iter()
            .any(|scheme| lowered.contains(scheme))
}

fn validate_string(
    value: &str,
    path: &str,
    limits: MotionGraphicLimits,
) -> Result<(), MotionGraphicValidationError> {
    if value.chars().count() > limits.max_string_length {
        return fail(
            "MG_STRING_LIMIT",
            path,
            "String exceeds the motion graphic limit",
        );
    }
    if has_resource_syntax(value) {
        return fail(
            "MG_EXTERNAL_RESOURCE",
            path,
            "External resource syntax is not allowed in motion graphics",
        );
    }
    Ok(())
}

fn allowed_node_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "text" => &[
            "text",
            "fontFamily",
            "fontSize",
            "fontWeight",
            "fontStyle",
            "textAlign",
            "lineHeight",
            "letterSpacing",
            "color",
            "maxWidth",
        ],
        "shape" => &["shape", "fill", "stroke", "strokeWidth", "borderRadius"],
        "svg" => &[
            "viewBox",
            "pathData",
            "fill",
            "stroke",
            "strokeWidth",
            "fillRule",
        ],
        "path" => &[
            "pathData",
            "fill",
            "stroke",
            "strokeWidth",
            "fillRule",
            "trimStart",
            "trimEnd",
        ],
        "chart" => &[
            "chartType",
            "data",
            "labels",
            "colors",
            "min",
            "max",
            "showLegend",
            "showAxes",
        ],
        "media" => &["assetId", "fit", "volume", "muted", "playbackRate"],
        "group" => &["layout", "gap", "clip", "maskId"],
        _ => &[],
    }
}

fn valid_stable_id(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn validate_data(
    value: &Value,
    path: &str,
    depth: usize,
    state: &mut ValidationState,
    limits: MotionGraphicLimits,
) -> Result<(), MotionGraphicValidationError> {
    if depth > 12 {
        return fail(
            "MG_DATA_DEPTH_LIMIT",
            path,
            "Structured MG data is too deeply nested",
        );
    }
    state.data_values += 1;
    if state.data_values > limits.max_data_values {
        return fail(
            "MG_DATA_LIMIT",
            path,
            "Motion graphic contains too much structured data",
        );
    }
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(number) if number.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Value::Number(_) => fail("MG_INVALID_NUMBER", path, "Expected a finite number"),
        Value::String(value) => validate_string(value, path, limits),
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_data(value, &format!("{path}[{index}]"), depth + 1, state, limits)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            for (key, value) in values {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    return fail(
                        "MG_UNSAFE_PROPERTY",
                        format!("{path}.{key}"),
                        format!("Unsafe structured-data key: {key}"),
                    );
                }
                validate_data(value, &format!("{path}.{key}"), depth + 1, state, limits)?;
            }
            Ok(())
        }
    }
}

fn validate_keyframes(
    value: &Value,
    path: &str,
    duration_seconds: f64,
    state: &mut ValidationState,
    limits: MotionGraphicLimits,
) -> Result<(), MotionGraphicValidationError> {
    let frames = value.as_array().ok_or_else(|| {
        MotionGraphicValidationError::new("MG_KEYFRAME_LIMIT", path, "Keyframes must be an array")
    })?;
    if frames.len() > limits.max_keyframes_per_property {
        return fail(
            "MG_KEYFRAME_LIMIT",
            path,
            "Too many keyframes for one property",
        );
    }
    let mut previous = -1.0;
    for (index, frame) in frames.iter().enumerate() {
        let frame_path = format!("{path}[{index}]");
        let frame = object(frame, &frame_path)?;
        for key in frame.keys() {
            if !["time", "value", "easing"].contains(&key.as_str()) {
                return fail(
                    "MG_UNKNOWN_PROPERTY",
                    format!("{frame_path}.{key}"),
                    format!("Unsupported keyframe property: {key}"),
                );
            }
        }
        let time = finite_number(frame.get("time"), &format!("{frame_path}.time"))?;
        if time < 0.0 || time > duration_seconds || time < previous {
            return fail(
                "MG_INVALID_KEYFRAME_TIME",
                format!("{frame_path}.time"),
                "Keyframe times must be ordered and within the composition duration",
            );
        }
        previous = time;
        let keyframe_value = frame.get("value").ok_or_else(|| {
            MotionGraphicValidationError::new(
                "MG_MISSING_VALUE",
                format!("{frame_path}.value"),
                "Keyframe value is required",
            )
        })?;
        validate_data(
            keyframe_value,
            &format!("{frame_path}.value"),
            0,
            state,
            limits,
        )?;
        if let Some(easing) = frame.get("easing") {
            let easing = easing.as_str().ok_or_else(|| {
                MotionGraphicValidationError::new(
                    "MG_INVALID_EASING",
                    format!("{frame_path}.easing"),
                    "Easing must be a supported string",
                )
            })?;
            if !EASINGS.contains(&easing) {
                return fail(
                    "MG_INVALID_EASING",
                    format!("{frame_path}.easing"),
                    "Unknown easing",
                );
            }
        }
    }
    state.keyframes += frames.len();
    if state.keyframes > limits.max_keyframes {
        return fail(
            "MG_KEYFRAME_LIMIT",
            path,
            "Motion graphic has too many keyframes",
        );
    }
    Ok(())
}

fn validate_node(
    value: &Value,
    path: &str,
    depth: usize,
    duration_seconds: f64,
    state: &mut ValidationState,
    limits: MotionGraphicLimits,
) -> Result<(), MotionGraphicValidationError> {
    if depth > limits.max_depth {
        return fail("MG_DEPTH_LIMIT", path, "Motion graphic nesting is too deep");
    }
    let node = object(value, path)?;
    let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
    if !NODE_KINDS.contains(&kind) {
        return fail(
            "MG_UNKNOWN_NODE",
            format!("{path}.type"),
            format!("Unknown node type: {kind}"),
        );
    }
    let id = node.get("id").and_then(Value::as_str).unwrap_or("");
    if !valid_stable_id(id, 128) {
        return fail(
            "MG_INVALID_ID",
            format!("{path}.id"),
            "Node id must be stable and portable",
        );
    }
    if !state.ids.insert(id.to_owned()) {
        return fail(
            "MG_DUPLICATE_ID",
            format!("{path}.id"),
            "Node ids must be unique",
        );
    }
    state.nodes += 1;
    if state.nodes > limits.max_nodes {
        return fail("MG_NODE_LIMIT", path, "Motion graphic has too many nodes");
    }

    let kind_keys = allowed_node_keys(kind);
    for (key, value) in node {
        if FORBIDDEN_KEYS.contains(&key.as_str())
            || (!COMMON_NODE_KEYS.contains(&key.as_str()) && !kind_keys.contains(&key.as_str()))
        {
            return fail(
                "MG_UNKNOWN_PROPERTY",
                format!("{path}.{key}"),
                format!("Unsupported property: {key}"),
            );
        }
        if let Some(value) = value.as_str() {
            validate_string(value, &format!("{path}.{key}"), limits)?;
        }
        if NUMERIC_NODE_KEYS.contains(&key.as_str()) {
            finite_number(Some(value), &format!("{path}.{key}"))?;
        }
    }

    if kind == "media" {
        let asset_id = node.get("assetId").and_then(Value::as_str).unwrap_or("");
        if !valid_stable_id(asset_id, 256) {
            return fail(
                "MG_INVALID_ASSET",
                format!("{path}.assetId"),
                "Media nodes must reference a stable managed asset id",
            );
        }
        state.asset_ids.insert(asset_id.to_owned());
    }
    for key in ["data", "labels", "colors"] {
        if let Some(value) = node.get(key) {
            validate_data(value, &format!("{path}.{key}"), 0, state, limits)?;
        }
    }
    if let Some(stagger) = node.get("stagger") {
        let stagger = finite_number(Some(stagger), &format!("{path}.stagger"))?;
        if !(0.0..=10.0).contains(&stagger) {
            return fail(
                "MG_INVALID_STAGGER",
                format!("{path}.stagger"),
                "Stagger must be between 0 and 10 seconds",
            );
        }
    }
    if let Some(animations) = node.get("animations") {
        let animations = object(animations, &format!("{path}.animations"))?;
        for (property, frames) in animations {
            let root_property = property.split('.').next().unwrap_or("");
            let valid_segments = property.split('.').all(|segment| {
                !segment.is_empty()
                    && segment.len() <= 41
                    && segment.as_bytes()[0].is_ascii_alphabetic()
                    && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    && !FORBIDDEN_KEYS.contains(&segment)
            });
            if !valid_segments || !ANIMATABLE_PROPERTIES.contains(&root_property) {
                return fail(
                    "MG_INVALID_PROPERTY",
                    format!("{path}.animations.{property}"),
                    "Invalid animation property",
                );
            }
            validate_keyframes(
                frames,
                &format!("{path}.animations.{property}"),
                duration_seconds,
                state,
                limits,
            )?;
        }
    }
    let children = match node.get("children") {
        None => &[][..],
        Some(Value::Array(children)) => children,
        Some(_) => {
            return fail(
                "MG_INVALID_CHILDREN",
                format!("{path}.children"),
                "children must be an array",
            );
        }
    };
    for (index, child) in children.iter().enumerate() {
        validate_node(
            child,
            &format!("{path}.children[{index}]"),
            depth + 1,
            duration_seconds,
            state,
            limits,
        )?;
    }
    Ok(())
}

pub fn validate_motion_graphic_dsl(
    value: &Value,
) -> Result<MotionGraphicValidationReport, MotionGraphicValidationError> {
    validate_motion_graphic_dsl_with_limits(value, MotionGraphicLimits::default())
}

pub fn validate_motion_graphic_dsl_with_limits(
    value: &Value,
    limits: MotionGraphicLimits,
) -> Result<MotionGraphicValidationReport, MotionGraphicValidationError> {
    let root = object(value, "$")?;
    for key in root.keys() {
        if FORBIDDEN_KEYS.contains(&key.as_str()) || !ROOT_KEYS.contains(&key.as_str()) {
            return fail(
                "MG_UNKNOWN_PROPERTY",
                format!("$.{key}"),
                format!("Unsupported property: {key}"),
            );
        }
    }
    let version = root.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version != 1 {
        return fail(
            "MG_UNSUPPORTED_VERSION",
            "$.version",
            "Only motion graphic DSL version 1 is supported",
        );
    }
    let width = finite_number(root.get("width"), "$.width")?;
    let height = finite_number(root.get("height"), "$.height")?;
    let duration_seconds = finite_number(root.get("durationSeconds"), "$.durationSeconds")?;
    if !(1.0..=8192.0).contains(&width) || !(1.0..=8192.0).contains(&height) {
        return fail(
            "MG_CANVAS_LIMIT",
            "$",
            "Canvas dimensions must be between 1 and 8192",
        );
    }
    if duration_seconds <= 0.0 || duration_seconds > limits.max_duration_seconds as f64 {
        return fail(
            "MG_DURATION_LIMIT",
            "$.durationSeconds",
            "Composition duration is outside the allowed range",
        );
    }
    if let Some(style) = root.get("designStyle") {
        let style = style.as_str().unwrap_or("");
        if !valid_stable_id(style, 128) {
            return fail(
                "MG_INVALID_STYLE",
                "$.designStyle",
                "designStyle must be a stable local style id",
            );
        }
    }
    if let Some(background) = root.get("background") {
        let background = background.as_str().ok_or_else(|| {
            MotionGraphicValidationError::new(
                "MG_INVALID_BACKGROUND",
                "$.background",
                "background must be a string",
            )
        })?;
        validate_string(background, "$.background", limits)?;
    }
    let nodes = root.get("nodes").and_then(Value::as_array).ok_or_else(|| {
        MotionGraphicValidationError::new("MG_INVALID_NODES", "$.nodes", "nodes must be an array")
    })?;
    let mut state = ValidationState::default();
    for (index, node) in nodes.iter().enumerate() {
        validate_node(
            node,
            &format!("$.nodes[{index}]"),
            0,
            duration_seconds,
            &mut state,
            limits,
        )?;
    }
    Ok(MotionGraphicValidationReport {
        version: version as u32,
        width: width.round() as u32,
        height: height.round() as u32,
        duration_milliseconds: (duration_seconds * 1_000.0).round() as u64,
        node_count: state.nodes,
        keyframe_count: state.keyframes,
        asset_ids: state.asset_ids.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_safe_editable_dsl_and_collects_assets() {
        let report = validate_motion_graphic_dsl(&json!({
            "version": 1,
            "width": 1920,
            "height": 1080,
            "durationSeconds": 5,
            "designStyle": "editorial-dark",
            "nodes": [{
                "id": "title",
                "type": "text",
                "text": "OpenChatCut",
                "x": 100,
                "y": 200,
                "animations": {
                    "opacity": [
                        { "time": 0, "value": 0, "easing": "ease-out" },
                        { "time": 1, "value": 1 }
                    ]
                }
            }, {
                "id": "photo",
                "type": "media",
                "assetId": "asset:photo",
                "x": 500,
                "y": 100
            }]
        }))
        .unwrap();
        assert_eq!(report.node_count, 2);
        assert_eq!(report.keyframe_count, 2);
        assert_eq!(report.asset_ids, ["asset:photo"]);
    }

    #[test]
    fn rejects_remote_resources_and_out_of_range_keyframes() {
        let unsafe_value = json!({
            "version": 1,
            "width": 100,
            "height": 100,
            "durationSeconds": 1,
            "nodes": [{ "id": "bad", "type": "shape", "fill": "url(https://evil.test/a)" }]
        });
        assert_eq!(
            validate_motion_graphic_dsl(&unsafe_value).unwrap_err().code,
            "MG_EXTERNAL_RESOURCE"
        );

        let late = json!({
            "version": 1,
            "width": 100,
            "height": 100,
            "durationSeconds": 1,
            "nodes": [{
                "id": "late",
                "type": "text",
                "text": "late",
                "animations": { "opacity": [{ "time": 2, "value": 1 }] }
            }]
        });
        assert_eq!(
            validate_motion_graphic_dsl(&late).unwrap_err().code,
            "MG_INVALID_KEYFRAME_TIME"
        );
    }

    #[test]
    fn builtin_template_catalog_is_independently_valid_and_complete() {
        let templates = builtin_motion_graphic_templates();
        assert_eq!(templates.len(), 8);
        let ids = templates
            .iter()
            .map(|template| template.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), templates.len());
        for category in [
            "lower-third",
            "title-card",
            "data-chart",
            "callout",
            "logo-reveal",
            "cta",
            "end-card",
            "design-style",
        ] {
            assert!(
                templates
                    .iter()
                    .any(|template| template.category == category),
                "missing {category} template"
            );
        }
        for template in templates {
            validate_motion_graphic_dsl(&template.definition)
                .unwrap_or_else(|error| panic!("template {} is invalid: {error}", template.id));
        }
    }
}
