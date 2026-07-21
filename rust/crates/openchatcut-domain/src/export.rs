use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AssetId, Background, FrameRate, ItemContent, MediaKind, ProjectDocument, Scene,
    TICKS_PER_SECOND, TimelineItem, TrackKind,
};

/// Delivery formats supported by the deterministic single-source export path.
/// More complex projects must use the scene-graph renderer rather than silently
/// dropping overlays, captions, motion graphics, or mixed audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    #[serde(rename = "mp4")]
    Mp4,
    #[serde(rename = "webm")]
    Webm,
    #[serde(rename = "wav")]
    Wav,
    #[serde(rename = "mp3")]
    Mp3,
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "png-sequence")]
    PngSequence,
    #[serde(rename = "prores-4444")]
    ProRes4444,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Png => "png",
            Self::PngSequence => "zip",
            Self::ProRes4444 => "mov",
        }
    }

    pub const fn has_video(self) -> bool {
        matches!(
            self,
            Self::Mp4 | Self::Webm | Self::Png | Self::PngSequence | Self::ProRes4444
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRange {
    pub start_ticks: i64,
    pub end_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasicExportSource {
    pub asset_id: AssetId,
    pub media_kind: MediaKind,
    pub source_start_ticks: i64,
    pub duration_ticks: i64,
    pub has_audio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasicExportPlan {
    pub renderer: &'static str,
    pub format: ExportFormat,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub timeline_start_ticks: i64,
    pub duration_ticks: i64,
    pub ticks_per_second: i64,
    pub source: BasicExportSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneGraphAudioSource {
    pub asset_id: AssetId,
    pub timeline_start_ticks: i64,
    pub source_start_ticks: i64,
    pub duration_ticks: i64,
    pub playback_rate: f64,
    pub gain: f64,
    pub fade_in_ticks: i64,
    pub fade_out_ticks: i64,
    pub fade_curve: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneGraphExportPlan {
    pub renderer: &'static str,
    pub format: ExportFormat,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub timeline_start_ticks: i64,
    pub duration_ticks: i64,
    pub ticks_per_second: i64,
    pub audio_sources: Vec<SceneGraphAudioSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineAudioExportPlan {
    pub renderer: &'static str,
    pub format: ExportFormat,
    pub timeline_start_ticks: i64,
    pub duration_ticks: i64,
    pub ticks_per_second: i64,
    pub audio_sources: Vec<SceneGraphAudioSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExportPlanError {
    #[error("the project has no scene to export")]
    MissingScene,
    #[error("the export range must have a non-negative start and a positive duration")]
    InvalidRange,
    #[error("export dimensions must be between 16 and 16384 pixels")]
    InvalidDimensions,
    #[error("export frame rate must be between 1 and 240 fps")]
    InvalidFrameRate,
    #[error("the selected scene has no enabled media for this export format")]
    MissingMedia,
    #[error("the selected scene needs the full scene-graph renderer: {0}")]
    NeedsSceneGraphRenderer(String),
    #[error("the selected media does not cover the complete export range")]
    RangeNotCovered,
    #[error("asset {0} is missing from the project")]
    MissingAsset(String),
    #[error("asset {0} is not backed by managed content")]
    UnmanagedAsset(String),
    #[error("the full scene-graph renderer only produces visual delivery formats")]
    UnsupportedSceneGraphFormat,
    #[error("the timeline audio renderer only produces WAV or MP3")]
    UnsupportedTimelineAudioFormat,
    #[error("the selected scene has no enabled timeline content")]
    EmptyTimeline,
}

/// Build a conservative FFmpeg fast-path plan. This path deliberately accepts
/// only one source. Rejecting complex timelines is safer than producing an
/// export that quietly omits authored content while the Chromium renderer is
/// unavailable.
pub fn build_basic_export_plan(
    document: &ProjectDocument,
    format: ExportFormat,
    range: Option<ExportRange>,
    dimensions: Option<(u32, u32)>,
    fps: Option<FrameRate>,
) -> Result<BasicExportPlan, ExportPlanError> {
    if matches!(
        format,
        ExportFormat::Png | ExportFormat::PngSequence | ExportFormat::ProRes4444
    ) {
        return Err(ExportPlanError::NeedsSceneGraphRenderer(
            "PNG and alpha-capable exports require the exact scene-graph renderer".to_owned(),
        ));
    }
    let scene = selected_scene(document).ok_or(ExportPlanError::MissingScene)?;
    let visual_format = format.has_video();
    if visual_format && document.settings.background != Background::default() {
        return Err(ExportPlanError::NeedsSceneGraphRenderer(
            "authored project backgrounds must be rendered".to_owned(),
        ));
    }
    let candidates = source_candidates(document, scene, visual_format)?;
    let item = match candidates.as_slice() {
        [] => return Err(ExportPlanError::MissingMedia),
        [item] => *item,
        _ => {
            return Err(ExportPlanError::NeedsSceneGraphRenderer(
                "multiple media layers or audio sources require mixing".to_owned(),
            ));
        }
    };
    let (asset_id, media_kind) = match &item.content {
        ItemContent::Media {
            asset_id,
            media_kind,
        } => (asset_id, *media_kind),
        _ => unreachable!("source_candidates returns only media items"),
    };
    let asset = document
        .assets
        .iter()
        .find(|asset| &asset.id == asset_id)
        .ok_or_else(|| ExportPlanError::MissingAsset(asset_id.to_string()))?;
    if !asset_has_local_content(asset) {
        return Err(ExportPlanError::UnmanagedAsset(asset_id.to_string()));
    }
    let track = scene
        .tracks
        .iter()
        .find(|track| track.items.iter().any(|candidate| candidate.id == item.id))
        .ok_or(ExportPlanError::MissingMedia)?;
    if visual_format && item_has_authored_visual_state(item) {
        return Err(ExportPlanError::NeedsSceneGraphRenderer(
            "visual transforms, opacity, masks, or effects must be rendered".to_owned(),
        ));
    }
    let audible = (media_kind == MediaKind::Audio || asset.has_audio)
        && !track.muted
        && item_audio_enabled(item);
    let (fade_in_ticks, fade_out_ticks) = item_story_crossfade(item);
    if (item_playback_rate(item) - 1.0).abs() > f64::EPSILON
        || (audible
            && ((item_audio_gain(item) - 1.0).abs() > f64::EPSILON
                || fade_in_ticks > 0
                || fade_out_ticks > 0))
    {
        return Err(ExportPlanError::NeedsSceneGraphRenderer(
            "retiming, gain, or fades require timeline rendering".to_owned(),
        ));
    }

    let item_end = item
        .end_ticks()
        .filter(|end| *end > item.start_ticks)
        .ok_or(ExportPlanError::InvalidRange)?;
    let range = range.unwrap_or(ExportRange {
        start_ticks: item.start_ticks.max(0),
        end_ticks: item_end,
    });
    if range.start_ticks < 0 || range.end_ticks <= range.start_ticks {
        return Err(ExportPlanError::InvalidRange);
    }
    if item.start_ticks > range.start_ticks || item_end < range.end_ticks {
        return Err(ExportPlanError::RangeNotCovered);
    }

    let (width, height) = dimensions.unwrap_or((
        document.settings.canvas_size.width,
        document.settings.canvas_size.height,
    ));
    validate_output_geometry(format, width, height)?;
    if visual_format
        && u64::from(width) * u64::from(document.settings.canvas_size.height)
            != u64::from(height) * u64::from(document.settings.canvas_size.width)
    {
        return Err(ExportPlanError::NeedsSceneGraphRenderer(
            "output aspect-ratio changes must be rendered from the scene graph".to_owned(),
        ));
    }
    let fps = fps.unwrap_or(document.settings.fps);
    validate_frame_rate(fps)?;
    let source_range_start = item.source_range.map(|source| source.in_ticks).unwrap_or(0);
    let source_start_ticks = source_range_start
        .checked_add(range.start_ticks - item.start_ticks)
        .ok_or(ExportPlanError::InvalidRange)?;
    let duration_ticks = if format == ExportFormat::Png {
        // A still export is pinned to one exact timeline frame.
        (TICKS_PER_SECOND as i128 * fps.denominator as i128 / fps.numerator as i128).max(1) as i64
    } else {
        range.end_ticks - range.start_ticks
    };

    Ok(BasicExportPlan {
        renderer: "ffmpeg-single-source-v1",
        format,
        width,
        height,
        fps,
        timeline_start_ticks: range.start_ticks,
        duration_ticks,
        ticks_per_second: TICKS_PER_SECOND,
        source: BasicExportSource {
            asset_id: asset.id.clone(),
            media_kind,
            source_start_ticks,
            duration_ticks,
            has_audio: audible,
        },
    })
}

/// Build a complete scene-graph render plan. Visual pixels are produced by the
/// same browser runtime as the editor while Rust remains authoritative for
/// range validation, managed-asset requirements, and deterministic audio
/// source timing.
pub fn build_scene_graph_export_plan(
    document: &ProjectDocument,
    format: ExportFormat,
    range: Option<ExportRange>,
    dimensions: Option<(u32, u32)>,
    fps: Option<FrameRate>,
) -> Result<SceneGraphExportPlan, ExportPlanError> {
    if !format.has_video() {
        return Err(ExportPlanError::UnsupportedSceneGraphFormat);
    }
    let scene = selected_scene(document).ok_or(ExportPlanError::MissingScene)?;
    let range = resolve_scene_export_range(scene, range)?;
    let (width, height) = dimensions.unwrap_or((
        document.settings.canvas_size.width,
        document.settings.canvas_size.height,
    ));
    validate_output_geometry(format, width, height)?;
    let fps = fps.unwrap_or(document.settings.fps);
    validate_frame_rate(fps)?;

    let audio_sources = collect_audio_sources(document, scene, range)?;

    Ok(SceneGraphExportPlan {
        renderer: "headless-scene-graph-v1",
        format,
        width,
        height,
        fps,
        timeline_start_ticks: range.start_ticks,
        duration_ticks: range.end_ticks - range.start_ticks,
        ticks_per_second: TICKS_PER_SECOND,
        audio_sources,
    })
}

/// Build a deterministic multi-source audio delivery plan. Unlike the basic
/// single-source path, this preserves clip placement, source trims, retiming,
/// gain, muted state, and speech-cut boundary fades without starting Chromium.
pub fn build_timeline_audio_export_plan(
    document: &ProjectDocument,
    format: ExportFormat,
    range: Option<ExportRange>,
) -> Result<TimelineAudioExportPlan, ExportPlanError> {
    if !matches!(format, ExportFormat::Wav | ExportFormat::Mp3) {
        return Err(ExportPlanError::UnsupportedTimelineAudioFormat);
    }
    let scene = selected_scene(document).ok_or(ExportPlanError::MissingScene)?;
    let range = resolve_scene_export_range(scene, range)?;
    let audio_sources = collect_audio_sources(document, scene, range)?;
    if audio_sources.is_empty() {
        return Err(ExportPlanError::MissingMedia);
    }
    Ok(TimelineAudioExportPlan {
        renderer: "ffmpeg-timeline-audio-v1",
        format,
        timeline_start_ticks: range.start_ticks,
        duration_ticks: range.end_ticks - range.start_ticks,
        ticks_per_second: TICKS_PER_SECOND,
        audio_sources,
    })
}

fn resolve_scene_export_range(
    scene: &Scene,
    requested: Option<ExportRange>,
) -> Result<ExportRange, ExportPlanError> {
    let scene_end = scene
        .tracks
        .iter()
        .filter(|track| !track.hidden)
        .flat_map(|track| track.items.iter())
        .filter(|item| item.enabled)
        .filter_map(TimelineItem::end_ticks)
        .max()
        .filter(|end| *end > 0)
        .ok_or(ExportPlanError::EmptyTimeline)?;
    let range = requested.unwrap_or(ExportRange {
        start_ticks: 0,
        end_ticks: scene_end,
    });
    if range.start_ticks < 0 || range.end_ticks <= range.start_ticks || range.end_ticks > scene_end
    {
        return Err(ExportPlanError::InvalidRange);
    }
    Ok(range)
}

fn collect_audio_sources(
    document: &ProjectDocument,
    scene: &Scene,
    range: ExportRange,
) -> Result<Vec<SceneGraphAudioSource>, ExportPlanError> {
    let mut audio_sources = Vec::new();
    for track in scene.tracks.iter().filter(|track| !track.hidden) {
        for item in track.items.iter().filter(|item| item.enabled) {
            let Some(item_end) = item.end_ticks() else {
                return Err(ExportPlanError::InvalidRange);
            };
            let overlap_start = item.start_ticks.max(range.start_ticks);
            let overlap_end = item_end.min(range.end_ticks);
            if overlap_end <= overlap_start {
                continue;
            }
            let ItemContent::Media {
                asset_id,
                media_kind,
            } = &item.content
            else {
                continue;
            };
            let asset = document
                .assets
                .iter()
                .find(|asset| &asset.id == asset_id)
                .ok_or_else(|| ExportPlanError::MissingAsset(asset_id.to_string()))?;
            if !asset_has_local_content(asset) {
                return Err(ExportPlanError::UnmanagedAsset(asset_id.to_string()));
            }
            let audible = (*media_kind == MediaKind::Audio || asset.has_audio)
                && !track.muted
                && item_audio_enabled(item);
            if !audible {
                continue;
            }
            let playback_rate = item_playback_rate(item);
            let source_range_start = item.source_range.map(|source| source.in_ticks).unwrap_or(0);
            let source_offset = ((overlap_start - item.start_ticks) as f64 * playback_rate).round();
            if !source_offset.is_finite() || source_offset > i64::MAX as f64 {
                return Err(ExportPlanError::InvalidRange);
            }
            audio_sources.push(SceneGraphAudioSource {
                asset_id: asset.id.clone(),
                timeline_start_ticks: overlap_start - range.start_ticks,
                source_start_ticks: source_range_start
                    .checked_add(source_offset as i64)
                    .ok_or(ExportPlanError::InvalidRange)?,
                duration_ticks: overlap_end - overlap_start,
                playback_rate,
                gain: item_audio_gain(item),
                fade_in_ticks: if overlap_start == item.start_ticks {
                    item_story_crossfade(item)
                        .0
                        .min(overlap_end - overlap_start)
                } else {
                    0
                },
                fade_out_ticks: if overlap_end == item_end {
                    item_story_crossfade(item)
                        .1
                        .min(overlap_end - overlap_start)
                } else {
                    0
                },
                fade_curve: "equalPower",
            });
        }
    }
    Ok(audio_sources)
}

fn asset_has_local_content(asset: &crate::Asset) -> bool {
    asset.content_hash.is_some()
        || asset
            .extensions
            .get("linkedFile")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|linked| {
                linked.get("version").and_then(serde_json::Value::as_u64) == Some(1)
                    && linked.get("portable").and_then(serde_json::Value::as_bool) == Some(false)
                    && linked
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|path| !path.is_empty())
            })
}

fn validate_output_geometry(
    format: ExportFormat,
    width: u32,
    height: u32,
) -> Result<(), ExportPlanError> {
    if !(16..=16_384).contains(&width) || !(16..=16_384).contains(&height) {
        return Err(ExportPlanError::InvalidDimensions);
    }
    if matches!(format, ExportFormat::Mp4 | ExportFormat::Webm)
        && (width % 2 != 0 || height % 2 != 0)
    {
        return Err(ExportPlanError::InvalidDimensions);
    }
    Ok(())
}

fn validate_frame_rate(fps: FrameRate) -> Result<(), ExportPlanError> {
    if fps.denominator == 0
        || fps.numerator == 0
        || fps.numerator > fps.denominator.saturating_mul(240)
    {
        return Err(ExportPlanError::InvalidFrameRate);
    }
    Ok(())
}

fn classic_element(item: &TimelineItem) -> Option<&serde_json::Map<String, serde_json::Value>> {
    item.extensions
        .get("classicElement")
        .and_then(serde_json::Value::as_object)
}

fn item_audio_enabled(item: &TimelineItem) -> bool {
    let classic = classic_element(item);
    if classic
        .and_then(|value| value.get("isSourceAudioEnabled"))
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return false;
    }
    classic
        .and_then(|value| value.get("params"))
        .and_then(serde_json::Value::as_object)
        .and_then(|params| params.get("muted"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
}

fn item_has_authored_visual_state(item: &TimelineItem) -> bool {
    let Some(classic) = classic_element(item) else {
        return item.extensions.keys().any(|key| key != "storyCrossfade");
    };
    let has_visual_params = classic
        .get("params")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|params| {
            params
                .keys()
                .any(|key| !matches!(key.as_str(), "muted" | "volume"))
        });
    let has_visual_classic_fields = classic.keys().any(|key| {
        !matches!(
            key.as_str(),
            "params" | "retime" | "trimStart" | "trimEnd" | "isSourceAudioEnabled"
        )
    });
    let has_other_extensions = item
        .extensions
        .keys()
        .any(|key| !matches!(key.as_str(), "classicElement" | "storyCrossfade"));
    has_visual_params || has_visual_classic_fields || has_other_extensions
}

fn item_playback_rate(item: &TimelineItem) -> f64 {
    classic_element(item)
        .and_then(|value| value.get("retime"))
        .and_then(serde_json::Value::as_object)
        .and_then(|retime| retime.get("rate"))
        .and_then(serde_json::Value::as_f64)
        .filter(|rate| rate.is_finite() && (0.05..=16.0).contains(rate))
        .unwrap_or(1.0)
}

fn item_audio_gain(item: &TimelineItem) -> f64 {
    let decibels = classic_element(item)
        .and_then(|value| value.get("params"))
        .and_then(serde_json::Value::as_object)
        .and_then(|params| params.get("volume"))
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
        .clamp(-60.0, 20.0);
    10_f64.powf(decibels / 20.0)
}

fn item_story_crossfade(item: &TimelineItem) -> (i64, i64) {
    let Some(crossfade) = item
        .extensions
        .get("storyCrossfade")
        .and_then(serde_json::Value::as_object)
    else {
        return (0, 0);
    };
    if crossfade.get("version").and_then(serde_json::Value::as_u64) != Some(1)
        || crossfade.get("curve").and_then(serde_json::Value::as_str) != Some("equalPower")
    {
        return (0, 0);
    }
    let duration = item.duration_ticks.max(1);
    let maximum = duration / 2;
    let read = |key| {
        crossfade
            .get(key)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            .clamp(0, maximum)
    };
    (read("fadeInTicks"), read("fadeOutTicks"))
}

fn selected_scene(document: &ProjectDocument) -> Option<&Scene> {
    document
        .current_scene_id
        .as_ref()
        .and_then(|id| document.scenes.iter().find(|scene| &scene.id == id))
        .or_else(|| document.scenes.iter().find(|scene| scene.is_main))
        .or_else(|| document.scenes.first())
}

fn source_candidates<'a>(
    document: &ProjectDocument,
    scene: &'a Scene,
    visual_format: bool,
) -> Result<Vec<&'a TimelineItem>, ExportPlanError> {
    let mut candidates = Vec::new();
    for track in &scene.tracks {
        if track.hidden || (track.kind == TrackKind::Audio && track.muted) {
            continue;
        }
        for item in track.items.iter().filter(|item| item.enabled) {
            match &item.content {
                ItemContent::Media {
                    asset_id,
                    media_kind,
                } => {
                    let asset = document
                        .assets
                        .iter()
                        .find(|asset| &asset.id == asset_id)
                        .ok_or_else(|| ExportPlanError::MissingAsset(asset_id.to_string()))?;
                    let audible = (*media_kind == MediaKind::Audio || asset.has_audio)
                        && !track.muted
                        && item_audio_enabled(item);
                    let usable = if visual_format {
                        *media_kind == MediaKind::Video
                    } else {
                        audible
                    };
                    if usable {
                        candidates.push(item);
                    } else if visual_format && *media_kind == MediaKind::Audio {
                        return Err(ExportPlanError::NeedsSceneGraphRenderer(
                            "separate audio tracks require timeline mixing".to_owned(),
                        ));
                    } else if visual_format && *media_kind != MediaKind::Audio {
                        return Err(ExportPlanError::NeedsSceneGraphRenderer(
                            "images require the scene-graph renderer".to_owned(),
                        ));
                    }
                }
                _ if visual_format && track.kind != TrackKind::Audio => {
                    return Err(ExportPlanError::NeedsSceneGraphRenderer(
                        "text, captions, graphics, stickers, and effects must be rendered"
                            .to_owned(),
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use crate::{
        Asset, AssetKind, ItemId, ProjectId, SceneId, Sha256Digest, TimelineItem, Track, TrackId,
    };

    use super::*;

    fn fixture() -> ProjectDocument {
        let mut document =
            ProjectDocument::new(ProjectId::new("export-project").unwrap(), "Export");
        let mut asset = Asset::new(
            AssetId::new("asset-video").unwrap(),
            "source.mp4",
            AssetKind::Video,
        );
        asset.content_hash = Some(Sha256Digest::new("a".repeat(64)).unwrap());
        asset.has_audio = true;
        document.assets.push(asset);
        let mut scene = Scene::new(SceneId::new("scene-main").unwrap(), "Main");
        scene.is_main = true;
        let mut track = Track::new(
            TrackId::new("track-video").unwrap(),
            "Video",
            TrackKind::Video,
        );
        track.items.push(TimelineItem::new(
            ItemId::new("item-video").unwrap(),
            "Source",
            0,
            30 * TICKS_PER_SECOND,
            ItemContent::Media {
                asset_id: AssetId::new("asset-video").unwrap(),
                media_kind: MediaKind::Video,
            },
        ));
        scene.tracks.push(track);
        document.current_scene_id = Some(scene.id.clone());
        document.scenes.push(scene);
        document
    }

    #[test]
    fn builds_a_word_size_independent_pinned_single_source_plan() {
        let plan =
            build_basic_export_plan(&fixture(), ExportFormat::Mp4, None, None, None).unwrap();
        assert_eq!(plan.renderer, "ffmpeg-single-source-v1");
        assert_eq!(plan.duration_ticks, 30 * TICKS_PER_SECOND);
        assert_eq!(plan.source.source_start_ticks, 0);
        assert!(plan.source.has_audio);
    }

    #[test]
    fn rejects_authored_layers_instead_of_silently_omitting_them() {
        let mut document = fixture();
        document.scenes[0].tracks[0].items.push(TimelineItem::new(
            ItemId::new("title").unwrap(),
            "Title",
            0,
            TICKS_PER_SECOND,
            ItemContent::Text {
                text: "Hello".to_owned(),
            },
        ));
        let error =
            build_basic_export_plan(&document, ExportFormat::Mp4, None, None, None).unwrap_err();
        assert!(matches!(error, ExportPlanError::NeedsSceneGraphRenderer(_)));
    }

    #[test]
    fn alpha_formats_always_use_the_shared_scene_graph_renderer() {
        for format in [
            ExportFormat::Png,
            ExportFormat::PngSequence,
            ExportFormat::ProRes4444,
        ] {
            let error = build_basic_export_plan(&fixture(), format, None, None, None).unwrap_err();
            assert!(matches!(error, ExportPlanError::NeedsSceneGraphRenderer(_)));
        }
    }

    #[test]
    fn fast_path_rejects_background_visual_state_and_aspect_ratio_changes() {
        let mut background = fixture();
        background.settings.background = Background::Color {
            color: "#123456".to_owned(),
        };
        assert!(matches!(
            build_basic_export_plan(&background, ExportFormat::Mp4, None, None, None),
            Err(ExportPlanError::NeedsSceneGraphRenderer(_))
        ));

        let mut transformed = fixture();
        transformed.scenes[0].tracks[0].items[0].extensions.insert(
            "classicElement".to_owned(),
            serde_json::json!({ "params": { "opacity": 0.5 } }),
        );
        assert!(matches!(
            build_basic_export_plan(&transformed, ExportFormat::Mp4, None, None, None),
            Err(ExportPlanError::NeedsSceneGraphRenderer(_))
        ));

        assert!(matches!(
            build_basic_export_plan(
                &fixture(),
                ExportFormat::Mp4,
                None,
                Some((1080, 1080)),
                None
            ),
            Err(ExportPlanError::NeedsSceneGraphRenderer(_))
        ));
    }

    #[test]
    fn scene_graph_plan_keeps_layers_and_builds_revision_relative_audio_timing() {
        let mut document = fixture();
        document.scenes[0].tracks.push({
            let mut track = Track::new(
                TrackId::new("track-title").unwrap(),
                "Titles",
                TrackKind::Text,
            );
            track.items.push(TimelineItem::new(
                ItemId::new("title").unwrap(),
                "Title",
                TICKS_PER_SECOND,
                2 * TICKS_PER_SECOND,
                ItemContent::Text {
                    text: "Hello".to_owned(),
                },
            ));
            track
        });
        let plan = build_scene_graph_export_plan(
            &document,
            ExportFormat::Mp4,
            Some(ExportRange {
                start_ticks: TICKS_PER_SECOND,
                end_ticks: 3 * TICKS_PER_SECOND,
            }),
            Some((1280, 720)),
            None,
        )
        .unwrap();
        assert_eq!(plan.renderer, "headless-scene-graph-v1");
        assert_eq!(plan.timeline_start_ticks, TICKS_PER_SECOND);
        assert_eq!(plan.duration_ticks, 2 * TICKS_PER_SECOND);
        assert_eq!(plan.audio_sources.len(), 1);
        assert_eq!(plan.audio_sources[0].timeline_start_ticks, 0);
        assert_eq!(plan.audio_sources[0].source_start_ticks, TICKS_PER_SECOND);
    }

    #[test]
    fn selection_must_be_covered_by_the_source_clip() {
        let error = build_basic_export_plan(
            &fixture(),
            ExportFormat::Mp4,
            Some(ExportRange {
                start_ticks: 0,
                end_ticks: 31 * TICKS_PER_SECOND,
            }),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(error, ExportPlanError::RangeNotCovered);
    }

    #[test]
    fn audio_fast_path_refuses_to_drop_gain_or_retiming() {
        let mut document = fixture();
        document.scenes[0].tracks[0].items[0].extensions.insert(
            "classicElement".to_owned(),
            serde_json::json!({
                "params": { "volume": -6.0, "muted": false },
                "retime": { "rate": 1.25 },
                "isSourceAudioEnabled": true
            }),
        );
        let error =
            build_basic_export_plan(&document, ExportFormat::Wav, None, None, None).unwrap_err();
        assert!(matches!(error, ExportPlanError::NeedsSceneGraphRenderer(_)));
    }

    #[test]
    fn timeline_audio_plan_preserves_multi_clip_timing_gain_retime_and_fades() {
        let mut document = fixture();
        document.assets[0].has_audio = false;
        let mut asset = Asset::new(
            AssetId::new("asset-dialogue").unwrap(),
            "dialogue.wav",
            AssetKind::Audio,
        );
        asset.content_hash = Some(Sha256Digest::new("b".repeat(64)).unwrap());
        asset.has_audio = true;
        document.assets.push(asset);
        let mut track = Track::new(
            TrackId::new("track-dialogue").unwrap(),
            "Dialogue",
            TrackKind::Audio,
        );
        let mut first = TimelineItem::new(
            ItemId::new("dialogue-first").unwrap(),
            "First",
            0,
            2 * TICKS_PER_SECOND,
            ItemContent::Media {
                asset_id: AssetId::new("asset-dialogue").unwrap(),
                media_kind: MediaKind::Audio,
            },
        );
        first.source_range = Some(crate::SourceRange {
            in_ticks: TICKS_PER_SECOND,
            out_ticks: 5 * TICKS_PER_SECOND,
        });
        first.extensions.insert(
            "classicElement".to_owned(),
            serde_json::json!({
                "params": { "volume": -6.0, "muted": false },
                "retime": { "rate": 2.0 }
            }),
        );
        first.extensions.insert(
            "storyCrossfade".to_owned(),
            serde_json::json!({
                "version": 1,
                "fadeInTicks": 0,
                "fadeOutTicks": 6_000,
                "curve": "equalPower"
            }),
        );
        track.items.push(first);
        track.items.push(TimelineItem::new(
            ItemId::new("dialogue-second").unwrap(),
            "Second",
            2 * TICKS_PER_SECOND,
            2 * TICKS_PER_SECOND,
            ItemContent::Media {
                asset_id: AssetId::new("asset-dialogue").unwrap(),
                media_kind: MediaKind::Audio,
            },
        ));
        document.scenes[0].tracks.push(track);

        let plan = build_timeline_audio_export_plan(
            &document,
            ExportFormat::Wav,
            Some(ExportRange {
                start_ticks: 0,
                end_ticks: 4 * TICKS_PER_SECOND,
            }),
        )
        .unwrap();
        assert_eq!(plan.renderer, "ffmpeg-timeline-audio-v1");
        assert_eq!(plan.duration_ticks, 4 * TICKS_PER_SECOND);
        assert_eq!(plan.audio_sources.len(), 2);
        assert_eq!(plan.audio_sources[0].source_start_ticks, TICKS_PER_SECOND);
        assert_eq!(plan.audio_sources[0].playback_rate, 2.0);
        assert!((plan.audio_sources[0].gain - 10_f64.powf(-6.0 / 20.0)).abs() < 1e-9);
        assert_eq!(plan.audio_sources[0].fade_out_ticks, 6_000);
        assert_eq!(
            plan.audio_sources[1].timeline_start_ticks,
            2 * TICKS_PER_SECOND
        );
    }
}
