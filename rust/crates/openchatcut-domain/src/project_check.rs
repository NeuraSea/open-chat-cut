use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ExportFormat, ItemContent, NleFormat, ProjectDocument, SubtitleFormat, build_basic_export_plan,
    build_scene_graph_export_plan, build_timeline_audio_export_plan, export_nle_xml,
    export_subtitle, validate_document,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectIssueSeverity {
    Blocker,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectValidationIssue {
    pub code: String,
    pub severity: ProjectIssueSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryValidationReport {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    pub issues: Vec<ProjectValidationIssue>,
}

impl DeliveryValidationReport {
    pub fn push(&mut self, issue: ProjectValidationIssue) {
        if issue.severity == ProjectIssueSeverity::Blocker {
            self.valid = false;
        }
        self.issues.push(issue);
    }
}

pub fn validate_project_delivery(
    document: &ProjectDocument,
    target: Option<&str>,
    headless_renderer_available: bool,
) -> DeliveryValidationReport {
    let mut report = DeliveryValidationReport {
        valid: true,
        target: target.map(str::to_owned),
        renderer: None,
        issues: Vec::new(),
    };
    if let Err(error) = validate_document(document) {
        report.push(issue(
            "invalid_document",
            ProjectIssueSeverity::Blocker,
            error.to_string(),
            None,
            Vec::new(),
        ));
        return report;
    }

    for (scene_index, scene) in document.scenes.iter().enumerate() {
        for (track_index, track) in scene.tracks.iter().enumerate() {
            for (item_index, item) in track.items.iter().enumerate() {
                let path =
                    format!("scenes[{scene_index}].tracks[{track_index}].items[{item_index}]");
                match &item.content {
                    ItemContent::Media { asset_id, .. } => {
                        if let Some(asset) =
                            document.assets.iter().find(|asset| &asset.id == asset_id)
                            && asset.content_hash.is_none()
                            && !asset.extensions.contains_key("linkedFile")
                        {
                            report.push(issue(
                                "asset_not_managed",
                                ProjectIssueSeverity::Blocker,
                                "Timeline media is not backed by the immutable managed content store",
                                Some(format!("{path}.content.assetId")),
                                vec![asset_id.to_string()],
                            ));
                        }
                    }
                    ItemContent::MotionGraphic { motion_graphic } => {
                        if !matches!(motion_graphic.dsl_version, 1 | 2) {
                            report.push(issue(
                                "mg_unsupported_version",
                                ProjectIssueSeverity::Blocker,
                                "Only motion graphic DSL version 1 and compiled JSX version 2 are supported",
                                Some(format!("{path}.content.motionGraphic.dslVersion")),
                                vec![item.id.to_string()],
                            ));
                        } else {
                            let mut visited = 0;
                            let validation = if motion_graphic.dsl_version == 1 {
                                validate_mg_definition(
                                    &motion_graphic.definition,
                                    document,
                                    0,
                                    &mut visited,
                                )
                            } else {
                                validate_compiled_jsx_definition(
                                    &motion_graphic.definition,
                                    document,
                                    &mut visited,
                                )
                            };
                            if let Err(message) = validation {
                                report.push(issue(
                                    "mg_unsafe_or_invalid",
                                    ProjectIssueSeverity::Blocker,
                                    message,
                                    Some(format!("{path}.content.motionGraphic.definition")),
                                    vec![item.id.to_string()],
                                ));
                            }
                        }
                    }
                    ItemContent::Custom { custom_type, .. } => report.push(issue(
                        "custom_item_unsupported",
                        ProjectIssueSeverity::Blocker,
                        format!(
                            "Custom timeline item {custom_type:?} has no deterministic renderer"
                        ),
                        Some(format!("{path}.content")),
                        vec![item.id.to_string()],
                    )),
                    _ => {}
                }
            }
        }
    }

    for (index, asset) in document.assets.iter().enumerate() {
        if asset.content_hash.is_none()
            && (asset.extensions.contains_key("linkedFile")
                || asset.extensions.contains_key("linkedPath"))
        {
            report.push(issue(
                "linked_asset_not_portable",
                ProjectIssueSeverity::Warning,
                "Linked-file media requires its authorized host path and makes the project non-portable",
                Some(format!("assets[{index}]")),
                vec![asset.id.to_string()],
            ));
        }
    }
    if !document.transcripts.is_empty() {
        report.push(issue(
            "transcript_text_is_untrusted_data",
            ProjectIssueSeverity::Info,
            "Transcript and caption text is isolated as untrusted project data and is never executed as instructions",
            Some("transcripts".to_owned()),
            Vec::new(),
        ));
    }

    if let Some(target) = target {
        if validate_native_delivery_target(document, target, &mut report) {
            return report;
        }
        let format = match parse_target(target) {
            Ok(format) => format,
            Err(message) => {
                report.push(issue(
                    "unsupported_delivery_target",
                    ProjectIssueSeverity::Blocker,
                    message,
                    None,
                    Vec::new(),
                ));
                return report;
            }
        };
        match build_basic_export_plan(document, format, None, None, None) {
            Ok(_) => report.renderer = Some("ffmpeg-single-source-v1".to_owned()),
            Err(fast_path_error) if format.has_video() => {
                match build_scene_graph_export_plan(document, format, None, None, None) {
                    Ok(_) if headless_renderer_available => {
                        report.renderer = Some("headless-scene-graph-v1".to_owned());
                        report.push(issue(
                            "headless_renderer_selected",
                            ProjectIssueSeverity::Info,
                            fast_path_error.to_string(),
                            None,
                            Vec::new(),
                        ));
                    }
                    Ok(_) => report.push(issue(
                        "headless_renderer_unavailable",
                        ProjectIssueSeverity::Blocker,
                        "This project needs Chromium scene-graph rendering, but the native worker is unavailable",
                        None,
                        Vec::new(),
                    )),
                    Err(error) => report.push(issue(
                        "invalid_export_plan",
                        ProjectIssueSeverity::Blocker,
                        error.to_string(),
                        None,
                        Vec::new(),
                    )),
                }
            }
            Err(fast_path_error) => {
                match build_timeline_audio_export_plan(document, format, None) {
                    Ok(_) if headless_renderer_available => {
                        report.renderer = Some("ffmpeg-timeline-audio-v1".to_owned());
                        report.push(issue(
                            "timeline_audio_renderer_selected",
                            ProjectIssueSeverity::Info,
                            fast_path_error.to_string(),
                            None,
                            Vec::new(),
                        ));
                    }
                    Ok(_) => report.push(issue(
                        "timeline_audio_renderer_unavailable",
                        ProjectIssueSeverity::Blocker,
                        "This project needs FFmpeg timeline audio mixing, but the native worker is unavailable",
                        None,
                        Vec::new(),
                    )),
                    Err(error) => report.push(issue(
                        "invalid_export_plan",
                        ProjectIssueSeverity::Blocker,
                        error.to_string(),
                        None,
                        Vec::new(),
                    )),
                }
            }
        }
    }

    report
}

fn validate_native_delivery_target(
    document: &ProjectDocument,
    target: &str,
    report: &mut DeliveryValidationReport,
) -> bool {
    let normalized = target.to_ascii_lowercase();
    let subtitle = match normalized.as_str() {
        "srt" => Some(SubtitleFormat::Srt),
        "vtt" => Some(SubtitleFormat::Vtt),
        "ass" => Some(SubtitleFormat::Ass),
        "txt" => Some(SubtitleFormat::Txt),
        _ => None,
    };
    if let Some(format) = subtitle {
        report.renderer = Some("rust-caption-export-v1".to_owned());
        if let Err(error) = export_subtitle(document, format, None) {
            report.push(issue(
                "subtitle_export_invalid",
                ProjectIssueSeverity::Blocker,
                error.to_string(),
                None,
                Vec::new(),
            ));
        }
        return true;
    }

    let nle = match normalized.as_str() {
        "premiere-xml" => Some(NleFormat::PremiereXml),
        "resolve-xml" => Some(NleFormat::ResolveXml),
        _ => None,
    };
    if let Some(format) = nle {
        report.renderer = Some(format.renderer().to_owned());
        let uris = document
            .assets
            .iter()
            .filter(|asset| asset.content_hash.is_some())
            .map(|asset| {
                (
                    asset.id.to_string(),
                    format!("file:///managed/{}", asset.id.as_str()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        match export_nle_xml(document, format, &uris) {
            Ok(exported) if !exported.unsupported_item_ids.is_empty() => report.push(issue(
                "nle_semantic_items_not_editable",
                ProjectIssueSeverity::Warning,
                "Captions, text, effects, and motion graphics are not emitted as editable NLE media clips",
                None,
                exported.unsupported_item_ids,
            )),
            Ok(_) => {}
            Err(error) => report.push(issue(
                "nle_export_invalid",
                ProjectIssueSeverity::Blocker,
                error.to_string(),
                None,
                Vec::new(),
            )),
        }
        return true;
    }

    if normalized == "project-package" {
        report.renderer = Some("openchatcut-project-package-v1".to_owned());
        for (index, asset) in document.assets.iter().enumerate() {
            if asset.content_hash.is_none() {
                report.push(issue(
                    "package_asset_not_managed",
                    ProjectIssueSeverity::Blocker,
                    "Portable project packages require every asset to be copied into the managed content store",
                    Some(format!("assets[{index}].contentHash")),
                    vec![asset.id.to_string()],
                ));
            }
        }
        return true;
    }
    false
}

fn parse_target(value: &str) -> Result<ExportFormat, String> {
    match value.to_ascii_lowercase().as_str() {
        "mp4" | "h264" | "h.264" => Ok(ExportFormat::Mp4),
        "webm" => Ok(ExportFormat::Webm),
        "wav" => Ok(ExportFormat::Wav),
        "mp3" => Ok(ExportFormat::Mp3),
        "png" => Ok(ExportFormat::Png),
        "png-sequence" => Ok(ExportFormat::PngSequence),
        "prores-4444" | "prores4444" => Ok(ExportFormat::ProRes4444),
        other => Err(format!(
            "Delivery target {other:?} is not implemented by this build"
        )),
    }
}

fn issue(
    code: impl Into<String>,
    severity: ProjectIssueSeverity,
    message: impl Into<String>,
    path: Option<String>,
    entity_ids: Vec<String>,
) -> ProjectValidationIssue {
    ProjectValidationIssue {
        code: code.into(),
        severity,
        message: message.into(),
        path,
        entity_ids,
    }
}

fn validate_compiled_jsx_definition(
    value: &Value,
    document: &ProjectDocument,
    visited: &mut usize,
) -> Result<(), String> {
    let definition = value
        .as_object()
        .ok_or_else(|| "Compiled JSX motion graphic must be an object".to_owned())?;
    if definition.get("version").and_then(Value::as_u64) != Some(1)
        || definition.get("mode").and_then(Value::as_str) != Some("jsx")
    {
        return Err("Compiled JSX motion graphic has an unsupported wrapper".to_owned());
    }
    if definition.keys().any(|key| {
        !matches!(
            key.as_str(),
            "version" | "mode" | "source" | "ir" | "validation" | "security"
        )
    }) {
        return Err("Compiled JSX motion graphic has an unknown wrapper property".to_owned());
    }
    let source = definition
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| "Compiled JSX motion graphic has no editable source".to_owned())?;
    if source.is_empty() || source.len() > 256 * 1024 || source.contains('\0') {
        return Err("Compiled JSX motion graphic source exceeds safe bounds".to_owned());
    }
    let ir = definition
        .get("ir")
        .and_then(Value::as_object)
        .ok_or_else(|| "Compiled JSX motion graphic has no safe IR".to_owned())?;
    if ir.get("version").and_then(Value::as_u64) != Some(1)
        || ir.get("kind").and_then(Value::as_str) != Some("jsxSafeIr")
        || !ir
            .get("width")
            .and_then(Value::as_u64)
            .is_some_and(|value| value == u64::from(document.settings.canvas_size.width))
        || !ir
            .get("height")
            .and_then(Value::as_u64)
            .is_some_and(|value| value == u64::from(document.settings.canvas_size.height))
        || !ir
            .get("durationSeconds")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 3_600.0)
        || !ir
            .get("fps")
            .and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 240.0)
    {
        return Err("Compiled JSX motion graphic has an unsupported safe IR".to_owned());
    }
    let validation = definition
        .get("validation")
        .and_then(Value::as_object)
        .ok_or_else(|| "Compiled JSX motion graphic has no validation metadata".to_owned())?;
    if validation.keys().any(|key| key != "astNodes")
        || !validation
            .get("astNodes")
            .and_then(Value::as_u64)
            .is_some_and(|value| (1..=20_000).contains(&value))
    {
        return Err("Compiled JSX motion graphic validation metadata is invalid".to_owned());
    }
    let security = definition
        .get("security")
        .and_then(Value::as_object)
        .ok_or_else(|| "Compiled JSX motion graphic has no security metadata".to_owned())?;
    if security.get("sourceExecuted").and_then(Value::as_bool) != Some(false)
        || security.get("networkAccess").and_then(Value::as_str) != Some("disabled")
        || security.get("fileAccess").and_then(Value::as_str) != Some("disabled")
        || security.get("sandboxOrigin").and_then(Value::as_str) != Some("opaque")
        || security.get("interpreter").and_then(Value::as_str)
            != Some("deterministic-allowlisted-ir-v1")
    {
        return Err("Compiled JSX motion graphic security metadata is invalid".to_owned());
    }
    validate_compiled_jsx_ir(
        definition
            .get("ir")
            .expect("safe IR was checked immediately above"),
        document,
        0,
        visited,
    )
}

fn validate_compiled_jsx_ir(
    value: &Value,
    document: &ProjectDocument,
    depth: usize,
    visited: &mut usize,
) -> Result<(), String> {
    if depth > 100 {
        return Err("Compiled JSX safe IR exceeds the nesting limit".to_owned());
    }
    *visited += 1;
    if *visited > 50_000 {
        return Err("Compiled JSX safe IR exceeds the value limit".to_owned());
    }
    match value {
        Value::String(value) => {
            if value.len() > 256 * 1024 {
                return Err("Compiled JSX safe IR contains an oversized string".to_owned());
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_compiled_jsx_ir(value, document, depth + 1, visited)?;
            }
        }
        Value::Object(values) => {
            for forbidden in ["__proto__", "prototype", "constructor"] {
                if values.contains_key(forbidden) {
                    return Err(format!("Compiled JSX property {forbidden:?} is forbidden"));
                }
            }
            if let Some(kind) = values.get("kind").and_then(Value::as_str) {
                if !matches!(
                    kind,
                    "jsxSafeIr"
                        | "literal"
                        | "identifier"
                        | "array"
                        | "object"
                        | "unary"
                        | "binary"
                        | "logical"
                        | "conditional"
                        | "member"
                        | "call"
                        | "template"
                        | "text"
                        | "expression"
                        | "fragment"
                        | "element"
                ) {
                    return Err(format!("Compiled JSX safe IR kind {kind:?} is not allowed"));
                }
                if kind == "call" {
                    let callee = values
                        .get("callee")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "Compiled JSX safe call has no callee".to_owned())?;
                    if !matches!(
                        callee,
                        "interpolate"
                            | "spring"
                            | "sequence"
                            | "clamp"
                            | "useCurrentFrame"
                            | "useVideoConfig"
                            | "Math.abs"
                            | "Math.ceil"
                            | "Math.cos"
                            | "Math.floor"
                            | "Math.max"
                            | "Math.min"
                            | "Math.pow"
                            | "Math.round"
                            | "Math.sin"
                            | "Math.sqrt"
                            | "Math.tan"
                    ) {
                        return Err(format!("Compiled JSX safe call {callee:?} is not allowed"));
                    }
                }
                if kind == "member"
                    && values
                        .get("property")
                        .and_then(Value::as_str)
                        .is_none_or(|property| {
                            matches!(property, "__proto__" | "prototype" | "constructor")
                        })
                {
                    return Err("Compiled JSX safe member is invalid".to_owned());
                }
                if kind == "element" {
                    let tag = values
                        .get("tag")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "Compiled JSX element has no tag".to_owned())?;
                    if !matches!(
                        tag,
                        "AbsoluteFill"
                            | "Sequence"
                            | "Img"
                            | "Video"
                            | "Audio"
                            | "div"
                            | "span"
                            | "p"
                            | "strong"
                            | "em"
                            | "img"
                            | "video"
                            | "audio"
                            | "svg"
                            | "g"
                            | "path"
                            | "rect"
                            | "circle"
                            | "ellipse"
                            | "line"
                            | "polyline"
                            | "polygon"
                            | "text"
                            | "tspan"
                            | "defs"
                            | "linearGradient"
                            | "radialGradient"
                            | "stop"
                            | "clipPath"
                            | "mask"
                    ) {
                        return Err(format!("Compiled JSX element {tag:?} is not allowed"));
                    }
                    validate_compiled_jsx_attributes(values.get("attributes"), document)?;
                }
            }
            for value in values.values() {
                validate_compiled_jsx_ir(value, document, depth + 1, visited)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_compiled_jsx_attributes(
    value: Option<&Value>,
    document: &ProjectDocument,
) -> Result<(), String> {
    let attributes = value
        .and_then(Value::as_array)
        .ok_or_else(|| "Compiled JSX element attributes are invalid".to_owned())?;
    for attribute in attributes {
        let attribute = attribute
            .as_array()
            .filter(|attribute| attribute.len() == 2)
            .ok_or_else(|| "Compiled JSX attribute is invalid".to_owned())?;
        let name = attribute[0]
            .as_str()
            .ok_or_else(|| "Compiled JSX attribute name is invalid".to_owned())?;
        if name.starts_with("on")
            || matches!(
                name,
                "dangerouslySetInnerHTML"
                    | "srcDoc"
                    | "srcSet"
                    | "action"
                    | "formAction"
                    | "poster"
                    | "__proto__"
                    | "prototype"
                    | "constructor"
            )
        {
            return Err(format!("Compiled JSX attribute {name:?} is forbidden"));
        }
        if matches!(name, "src" | "href" | "xlinkHref") {
            let resource = attribute[1]
                .as_object()
                .filter(|value| value.get("kind").and_then(Value::as_str) == Some("literal"))
                .and_then(|value| value.get("value"))
                .and_then(Value::as_str)
                .ok_or_else(|| "Compiled JSX resource must be a static literal".to_owned())?;
            if resource.starts_with("asset:") {
                let asset = document
                    .assets
                    .iter()
                    .find(|asset| asset.id.as_str() == resource)
                    .ok_or_else(|| format!("Compiled JSX references missing asset {resource:?}"))?;
                if asset.content_hash.is_none() {
                    return Err(format!("Compiled JSX asset {resource:?} is not managed"));
                }
            } else if !resource.starts_with('#') {
                return Err("Compiled JSX resource is not managed".to_owned());
            }
        }
    }
    Ok(())
}

fn validate_mg_definition(
    value: &Value,
    document: &ProjectDocument,
    depth: usize,
    visited: &mut usize,
) -> Result<(), String> {
    if depth > 20 {
        return Err("Motion graphic definition exceeds the nesting limit".to_owned());
    }
    *visited += 1;
    if *visited > 50_000 {
        return Err("Motion graphic definition exceeds the value limit".to_owned());
    }
    match value {
        Value::String(value) => {
            if value.len() > 20_000 {
                return Err("Motion graphic string exceeds the size limit".to_owned());
            }
            let lower = value.to_ascii_lowercase();
            if lower.contains("url(")
                || lower.contains("http:")
                || lower.contains("https:")
                || lower.contains("file:")
                || lower.contains("javascript:")
            {
                return Err("Motion graphic contains an external resource reference".to_owned());
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_mg_definition(value, document, depth + 1, visited)?;
            }
        }
        Value::Object(values) => {
            for forbidden in [
                "__proto__",
                "prototype",
                "constructor",
                "src",
                "srcSet",
                "href",
                "url",
                "uri",
                "dangerouslySetInnerHTML",
            ] {
                if values.contains_key(forbidden) {
                    return Err(format!(
                        "Motion graphic property {forbidden:?} is forbidden"
                    ));
                }
            }
            if values.get("type").and_then(Value::as_str) == Some("media") {
                let asset_id = values
                    .get("assetId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Motion graphic media node has no assetId".to_owned())?;
                let asset = document
                    .assets
                    .iter()
                    .find(|asset| asset.id.as_str() == asset_id)
                    .ok_or_else(|| {
                        format!("Motion graphic references missing asset {asset_id:?}")
                    })?;
                if asset.content_hash.is_none() {
                    return Err(format!("Motion graphic asset {asset_id:?} is not managed"));
                }
            }
            for value in values.values() {
                validate_mg_definition(value, document, depth + 1, visited)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        Asset, AssetId, AssetKind, ItemContent, ItemId, MediaKind, ProjectId, Scene, SceneId,
        Sha256Digest, TICKS_PER_SECOND, TimelineItem, Track, TrackId, TrackKind,
    };

    use super::*;

    fn layered_document() -> ProjectDocument {
        let mut document = ProjectDocument::new(ProjectId::new("check").unwrap(), "Check");
        let mut scene = Scene::new(SceneId::new("main").unwrap(), "Main");
        scene.is_main = true;
        let mut track = Track::new(TrackId::new("text").unwrap(), "Text", TrackKind::Text);
        track.items.push(TimelineItem::new(
            ItemId::new("title").unwrap(),
            "Title",
            0,
            TICKS_PER_SECOND,
            ItemContent::Text {
                text: "Title".to_owned(),
            },
        ));
        scene.tracks.push(track);
        document.current_scene_id = Some(scene.id.clone());
        document.scenes.push(scene);
        document
    }

    #[test]
    fn layered_video_target_selects_headless_without_becoming_a_blocker() {
        let report = validate_project_delivery(&layered_document(), Some("mp4"), true);
        assert!(report.valid, "{:?}", report.issues);
        assert_eq!(report.renderer.as_deref(), Some("headless-scene-graph-v1"));
    }

    #[test]
    fn multi_clip_audio_target_selects_timeline_mixer() {
        let mut document = ProjectDocument::new(ProjectId::new("audio-check").unwrap(), "Audio");
        let mut asset = Asset::new(
            AssetId::new("dialogue").unwrap(),
            "dialogue.wav",
            AssetKind::Audio,
        );
        asset.content_hash = Some(Sha256Digest::new("c".repeat(64)).unwrap());
        asset.duration_ticks = Some(2 * TICKS_PER_SECOND);
        asset.has_audio = true;
        document.assets.push(asset);
        let mut scene = Scene::new(SceneId::new("main").unwrap(), "Main");
        scene.is_main = true;
        let mut track = Track::new(
            TrackId::new("dialogue").unwrap(),
            "Dialogue",
            TrackKind::Audio,
        );
        for (id, start) in [("first", 0), ("second", TICKS_PER_SECOND)] {
            track.items.push(TimelineItem::new(
                ItemId::new(id).unwrap(),
                id,
                start,
                TICKS_PER_SECOND,
                ItemContent::Media {
                    asset_id: AssetId::new("dialogue").unwrap(),
                    media_kind: MediaKind::Audio,
                },
            ));
        }
        scene.tracks.push(track);
        document.current_scene_id = Some(scene.id.clone());
        document.scenes.push(scene);

        let report = validate_project_delivery(&document, Some("wav"), true);
        assert!(report.valid, "{:?}", report.issues);
        assert_eq!(report.renderer.as_deref(), Some("ffmpeg-timeline-audio-v1"));
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "timeline_audio_renderer_selected")
        );
    }

    #[test]
    fn unsafe_motion_graphic_resource_is_a_blocker() {
        let mut document = layered_document();
        document.scenes[0].tracks[0].items[0].content = ItemContent::MotionGraphic {
            motion_graphic: crate::MotionGraphicElement {
                dsl_version: 1,
                definition: serde_json::json!({
                    "version": 1,
                    "nodes": [{ "id": "bad", "type": "text", "text": "url(https://evil.test)" }]
                }),
                template_id: None,
            },
        };
        document.scenes[0].tracks[0].kind = TrackKind::Graphic;
        let report = validate_project_delivery(&document, None, true);
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "mg_unsafe_or_invalid")
        );
    }

    #[test]
    fn compiled_jsx_safe_ir_is_deliverable_but_imported_external_ir_is_blocked() {
        let mut document = layered_document();
        document.scenes[0].tracks[0].items[0].content = ItemContent::MotionGraphic {
            motion_graphic: crate::MotionGraphicElement {
                dsl_version: 2,
                definition: serde_json::json!({
                    "version": 1,
                    "mode": "jsx",
                    "source": "export default () => <div>Safe</div>",
                    "ir": {
                        "version": 1,
                        "kind": "jsxSafeIr",
                        "width": 1920,
                        "height": 1080,
                        "durationSeconds": 1,
                        "fps": 30,
                        "program": {
                            "bindings": [],
                            "root": {
                                "kind": "element",
                                "tag": "div",
                                "attributes": [],
                                "children": [{ "kind": "text", "value": "Safe" }]
                            }
                        }
                    },
                    "validation": { "astNodes": 8 },
                    "security": {
                        "sourceExecuted": false,
                        "interpreter": "deterministic-allowlisted-ir-v1",
                        "networkAccess": "disabled",
                        "fileAccess": "disabled",
                        "sandboxOrigin": "opaque"
                    }
                }),
                template_id: None,
            },
        };
        document.scenes[0].tracks[0].kind = TrackKind::Graphic;
        let report = validate_project_delivery(&document, None, true);
        assert!(report.valid, "{:?}", report.issues);

        let ItemContent::MotionGraphic { motion_graphic } =
            &mut document.scenes[0].tracks[0].items[0].content
        else {
            panic!("motion graphic expected");
        };
        motion_graphic.definition["ir"]["program"]["root"]["tag"] = serde_json::json!("iframe");
        let report = validate_project_delivery(&document, None, true);
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "mg_unsafe_or_invalid")
        );
    }

    #[test]
    fn native_delivery_targets_report_their_actual_renderers() {
        let mut document = layered_document();
        let mut asset = Asset::new(
            AssetId::new("asset:managed").unwrap(),
            "clip.mp4",
            AssetKind::Video,
        );
        asset.content_hash = Some(
            Sha256Digest::new("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .unwrap(),
        );
        document.assets.push(asset);
        let mut video = Track::new(TrackId::new("video").unwrap(), "Video", TrackKind::Video);
        video.items.push(TimelineItem::new(
            ItemId::new("clip").unwrap(),
            "Clip",
            0,
            TICKS_PER_SECOND,
            ItemContent::Media {
                asset_id: AssetId::new("asset:managed").unwrap(),
                media_kind: MediaKind::Video,
            },
        ));
        document.scenes[0].tracks.insert(0, video);

        let nle = validate_project_delivery(&document, Some("premiere-xml"), false);
        assert!(nle.valid, "{:?}", nle.issues);
        assert_eq!(nle.renderer.as_deref(), Some("premiere-fcp7-xml-v1"));
        assert!(
            nle.issues
                .iter()
                .any(|issue| issue.code == "nle_semantic_items_not_editable")
        );

        let package = validate_project_delivery(&document, Some("project-package"), false);
        assert!(package.valid, "{:?}", package.issues);
        assert_eq!(
            package.renderer.as_deref(),
            Some("openchatcut-project-package-v1")
        );
    }
}
