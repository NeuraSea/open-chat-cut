use std::collections::{BTreeMap, BTreeSet};

use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Asset, AssetKind, FrameRate, ItemContent, MediaKind, ProjectDocument, Scene, TICKS_PER_SECOND,
    TimelineItem, Track, TrackKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NleFormat {
    #[serde(rename = "premiere-xml")]
    PremiereXml,
    #[serde(rename = "resolve-xml")]
    ResolveXml,
}

impl NleFormat {
    pub const fn renderer(self) -> &'static str {
        match self {
            Self::PremiereXml => "premiere-fcp7-xml-v1",
            Self::ResolveXml => "resolve-fcpxml-v1",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NleExport {
    pub content: String,
    /// Semantic content cannot be represented as editable NLE media clips.
    /// Callers surface these IDs rather than silently claiming full fidelity.
    pub unsupported_item_ids: Vec<String>,
    pub media_clip_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NleXmlError {
    #[error("the project has no scene to export")]
    MissingScene,
    #[error("the selected scene has no enabled media clips")]
    MissingMedia,
    #[error("timeline item {0} has invalid timing")]
    InvalidTiming(String),
    #[error("timeline item {item_id} references missing asset {asset_id}")]
    MissingAsset { item_id: String, asset_id: String },
    #[error("managed file URI is missing for asset {0}")]
    MissingAssetUri(String),
    #[error("NLE XML serialization failed: {0}")]
    Serialization(String),
}

struct MediaClip<'a> {
    track: &'a Track,
    track_index: usize,
    item: &'a TimelineItem,
    asset: &'a Asset,
    media_kind: MediaKind,
    uri: &'a str,
}

pub fn export_nle_xml(
    document: &ProjectDocument,
    format: NleFormat,
    asset_file_uris: &BTreeMap<String, String>,
) -> Result<NleExport, NleXmlError> {
    let scene = selected_scene(document).ok_or(NleXmlError::MissingScene)?;
    let mut clips = Vec::new();
    let mut unsupported = Vec::new();
    for (track_index, track) in scene.tracks.iter().enumerate() {
        if track.hidden {
            continue;
        }
        for item in track.items.iter().filter(|item| item.enabled) {
            let ItemContent::Media {
                asset_id,
                media_kind,
            } = &item.content
            else {
                unsupported.push(item.id.to_string());
                continue;
            };
            validate_item_timing(item)?;
            let asset = document
                .assets
                .iter()
                .find(|asset| &asset.id == asset_id)
                .ok_or_else(|| NleXmlError::MissingAsset {
                    item_id: item.id.to_string(),
                    asset_id: asset_id.to_string(),
                })?;
            let uri = asset_file_uris
                .get(asset_id.as_str())
                .ok_or_else(|| NleXmlError::MissingAssetUri(asset_id.to_string()))?;
            clips.push(MediaClip {
                track,
                track_index,
                item,
                asset,
                media_kind: *media_kind,
                uri,
            });
        }
    }
    if clips.is_empty() {
        return Err(NleXmlError::MissingMedia);
    }
    unsupported.sort();
    unsupported.dedup();
    let content = match format {
        NleFormat::PremiereXml => premiere_xml(document, scene, &clips)?,
        NleFormat::ResolveXml => resolve_xml(document, scene, &clips)?,
    };
    Ok(NleExport {
        content,
        unsupported_item_ids: unsupported,
        media_clip_count: clips.len(),
    })
}

fn selected_scene(document: &ProjectDocument) -> Option<&Scene> {
    document
        .current_scene_id
        .as_ref()
        .and_then(|id| document.scenes.iter().find(|scene| &scene.id == id))
        .or_else(|| document.scenes.iter().find(|scene| scene.is_main))
        .or_else(|| document.scenes.first())
}

fn validate_item_timing(item: &TimelineItem) -> Result<(), NleXmlError> {
    if item.start_ticks < 0
        || item.duration_ticks <= 0
        || item.end_ticks().is_none()
        || item
            .source_range
            .is_some_and(|range| range.in_ticks < 0 || range.out_ticks <= range.in_ticks)
    {
        return Err(NleXmlError::InvalidTiming(item.id.to_string()));
    }
    Ok(())
}

fn premiere_xml(
    document: &ProjectDocument,
    scene: &Scene,
    clips: &[MediaClip<'_>],
) -> Result<String, NleXmlError> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    write(
        &mut writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;
    write(&mut writer, Event::DocType(BytesText::new("xmeml")))?;
    start(&mut writer, "xmeml", &[("version", "5")])?;
    start(&mut writer, "sequence", &[("id", "openchatcut-sequence")])?;
    text(&mut writer, "name", &scene.name)?;
    let fps = document.settings.fps;
    rate(&mut writer, fps)?;
    text(
        &mut writer,
        "duration",
        &timeline_frames(clips, fps)?.to_string(),
    )?;
    start(&mut writer, "media", &[])?;
    write_premiere_tracks(&mut writer, document, clips, true)?;
    write_premiere_tracks(&mut writer, document, clips, false)?;
    end(&mut writer, "media")?;
    end(&mut writer, "sequence")?;
    end(&mut writer, "xmeml")?;
    finish(writer)
}

fn write_premiere_tracks(
    writer: &mut Writer<Vec<u8>>,
    document: &ProjectDocument,
    clips: &[MediaClip<'_>],
    video: bool,
) -> Result<(), NleXmlError> {
    start(writer, if video { "video" } else { "audio" }, &[])?;
    if video {
        start(writer, "format", &[])?;
        start(writer, "samplecharacteristics", &[])?;
        rate(writer, document.settings.fps)?;
        text(
            writer,
            "width",
            &document.settings.canvas_size.width.to_string(),
        )?;
        text(
            writer,
            "height",
            &document.settings.canvas_size.height.to_string(),
        )?;
        text(writer, "pixelaspectratio", "square")?;
        end(writer, "samplecharacteristics")?;
        end(writer, "format")?;
    }
    let track_ids = clips
        .iter()
        .filter(|clip| clip_is_video(clip) == video)
        .map(|clip| clip.track.id.to_string())
        .collect::<BTreeSet<_>>();
    for track_id in track_ids {
        start(writer, "track", &[])?;
        for clip in clips
            .iter()
            .filter(|clip| clip.track.id.as_str() == track_id && clip_is_video(clip) == video)
        {
            premiere_clip(writer, document.settings.fps, clip, video)?;
        }
        text(writer, "enabled", "TRUE")?;
        text(writer, "locked", "FALSE")?;
        end(writer, "track")?;
    }
    end(writer, if video { "video" } else { "audio" })
}

fn premiere_clip(
    writer: &mut Writer<Vec<u8>>,
    fps: FrameRate,
    clip: &MediaClip<'_>,
    video: bool,
) -> Result<(), NleXmlError> {
    let clip_id = format!("clip-{}-{}", if video { "v" } else { "a" }, clip.item.id);
    start(writer, "clipitem", &[("id", &clip_id)])?;
    text(writer, "name", &clip.item.name)?;
    text(writer, "enabled", "TRUE")?;
    text(
        writer,
        "start",
        &ticks_to_floor_frames(clip.item.start_ticks, fps).to_string(),
    )?;
    text(
        writer,
        "end",
        &ticks_to_ceil_frames(
            clip.item
                .end_ticks()
                .ok_or_else(|| NleXmlError::InvalidTiming(clip.item.id.to_string()))?,
            fps,
        )
        .to_string(),
    )?;
    let source_start = clip.item.source_range.map_or(0, |range| range.in_ticks);
    let source_end = source_start
        .checked_add(clip.item.duration_ticks)
        .ok_or_else(|| NleXmlError::InvalidTiming(clip.item.id.to_string()))?;
    text(
        writer,
        "in",
        &ticks_to_floor_frames(source_start, fps).to_string(),
    )?;
    text(
        writer,
        "out",
        &ticks_to_ceil_frames(source_end, fps).to_string(),
    )?;
    let file_id = format!("file-{}-{}", clip.asset.id, clip.item.id);
    start(writer, "file", &[("id", &file_id)])?;
    text(writer, "name", &clip.asset.name)?;
    text(writer, "pathurl", clip.uri)?;
    rate(writer, fps)?;
    let asset_duration = clip
        .asset
        .duration_ticks
        .or(clip.item.source_duration_ticks)
        .unwrap_or(source_end)
        .max(source_end);
    text(
        writer,
        "duration",
        &ticks_to_ceil_frames(asset_duration, fps).to_string(),
    )?;
    start(writer, "media", &[])?;
    empty(writer, if video { "video" } else { "audio" }, &[])?;
    end(writer, "media")?;
    end(writer, "file")?;
    end(writer, "clipitem")
}

fn resolve_xml(
    document: &ProjectDocument,
    scene: &Scene,
    clips: &[MediaClip<'_>],
) -> Result<String, NleXmlError> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    write(
        &mut writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;
    write(&mut writer, Event::DocType(BytesText::new("fcpxml")))?;
    start(&mut writer, "fcpxml", &[("version", "1.10")])?;
    start(&mut writer, "resources", &[])?;
    let frame_duration = format!(
        "{}/{}s",
        document.settings.fps.denominator, document.settings.fps.numerator
    );
    empty(
        &mut writer,
        "format",
        &[
            ("id", "r0"),
            ("name", "OpenChatCutFormat"),
            ("frameDuration", &frame_duration),
            ("width", &document.settings.canvas_size.width.to_string()),
            ("height", &document.settings.canvas_size.height.to_string()),
            ("colorSpace", "1-1-1 (Rec. 709)"),
        ],
    )?;
    let mut resource_ids = BTreeMap::new();
    for clip in clips {
        if resource_ids.contains_key(clip.asset.id.as_str()) {
            continue;
        }
        let id = format!("r{}", resource_ids.len() + 1);
        resource_ids.insert(clip.asset.id.to_string(), id.clone());
        let duration = ticks_time(
            clip.asset
                .duration_ticks
                .or(clip.item.source_duration_ticks)
                .unwrap_or(clip.item.duration_ticks),
        );
        empty(
            &mut writer,
            "asset",
            &[
                ("id", &id),
                ("name", &clip.asset.name),
                ("src", clip.uri),
                ("start", "0s"),
                ("duration", &duration),
                (
                    "hasVideo",
                    if clip.asset.kind == AssetKind::Audio {
                        "0"
                    } else {
                        "1"
                    },
                ),
                (
                    "hasAudio",
                    if clip.asset.has_audio || clip.asset.kind == AssetKind::Audio {
                        "1"
                    } else {
                        "0"
                    },
                ),
                ("format", "r0"),
            ],
        )?;
    }
    end(&mut writer, "resources")?;
    start(&mut writer, "library", &[])?;
    start(&mut writer, "event", &[("name", "OpenChatCut")])?;
    start(&mut writer, "project", &[("name", &document.name)])?;
    let duration_ticks = clips
        .iter()
        .filter_map(|clip| clip.item.end_ticks())
        .max()
        .unwrap_or(0);
    start(
        &mut writer,
        "sequence",
        &[
            ("duration", &ticks_time(duration_ticks)),
            ("format", "r0"),
            ("tcStart", "0s"),
            ("tcFormat", "NDF"),
            ("audioLayout", "stereo"),
            ("audioRate", "48k"),
        ],
    )?;
    start(&mut writer, "spine", &[])?;
    start(
        &mut writer,
        "gap",
        &[
            ("name", &scene.name),
            ("offset", "0s"),
            ("start", "0s"),
            ("duration", &ticks_time(duration_ticks)),
        ],
    )?;
    for clip in clips {
        let reference = resource_ids
            .get(clip.asset.id.as_str())
            .expect("resources were collected from the same clips");
        let source_start = clip.item.source_range.map_or(0, |range| range.in_ticks);
        let lane = if clip_is_video(clip) {
            (clip.track_index + 1) as i64
        } else {
            -((clip.track_index + 1) as i64)
        };
        empty(
            &mut writer,
            "asset-clip",
            &[
                ("name", &clip.item.name),
                ("ref", reference),
                ("lane", &lane.to_string()),
                ("offset", &ticks_time(clip.item.start_ticks)),
                ("start", &ticks_time(source_start)),
                ("duration", &ticks_time(clip.item.duration_ticks)),
            ],
        )?;
    }
    end(&mut writer, "gap")?;
    end(&mut writer, "spine")?;
    end(&mut writer, "sequence")?;
    end(&mut writer, "project")?;
    end(&mut writer, "event")?;
    end(&mut writer, "library")?;
    end(&mut writer, "fcpxml")?;
    finish(writer)
}

fn clip_is_video(clip: &MediaClip<'_>) -> bool {
    match clip.track.kind {
        TrackKind::Audio => false,
        TrackKind::Video => clip.media_kind != MediaKind::Audio,
        _ => clip.media_kind != MediaKind::Audio,
    }
}

fn timeline_frames(clips: &[MediaClip<'_>], fps: FrameRate) -> Result<i64, NleXmlError> {
    let ticks = clips
        .iter()
        .filter_map(|clip| clip.item.end_ticks())
        .max()
        .ok_or(NleXmlError::MissingMedia)?;
    Ok(ticks_to_ceil_frames(ticks, fps))
}

fn ticks_to_floor_frames(ticks: i64, fps: FrameRate) -> i64 {
    ((ticks as i128 * fps.numerator as i128) / (TICKS_PER_SECOND as i128 * fps.denominator as i128))
        as i64
}

fn ticks_to_ceil_frames(ticks: i64, fps: FrameRate) -> i64 {
    let numerator = ticks as i128 * fps.numerator as i128;
    let denominator = TICKS_PER_SECOND as i128 * fps.denominator as i128;
    ((numerator + denominator - 1) / denominator) as i64
}

fn ticks_time(ticks: i64) -> String {
    if ticks == 0 {
        "0s".to_owned()
    } else {
        format!("{ticks}/{TICKS_PER_SECOND}s")
    }
}

fn rate(writer: &mut Writer<Vec<u8>>, fps: FrameRate) -> Result<(), NleXmlError> {
    start(writer, "rate", &[])?;
    text(
        writer,
        "timebase",
        &((fps.numerator + fps.denominator / 2) / fps.denominator).to_string(),
    )?;
    text(
        writer,
        "ntsc",
        if fps.denominator == 1001 {
            "TRUE"
        } else {
            "FALSE"
        },
    )?;
    end(writer, "rate")
}

fn write(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<(), NleXmlError> {
    writer
        .write_event(event)
        .map_err(|error| NleXmlError::Serialization(error.to_string()))
}

fn start(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    attributes: &[(&str, &str)],
) -> Result<(), NleXmlError> {
    let mut element = BytesStart::new(name);
    for &(key, value) in attributes {
        element.push_attribute((key, value));
    }
    write(writer, Event::Start(element))
}

fn empty(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    attributes: &[(&str, &str)],
) -> Result<(), NleXmlError> {
    let mut element = BytesStart::new(name);
    for &(key, value) in attributes {
        element.push_attribute((key, value));
    }
    write(writer, Event::Empty(element))
}

fn end(writer: &mut Writer<Vec<u8>>, name: &str) -> Result<(), NleXmlError> {
    write(writer, Event::End(BytesEnd::new(name)))
}

fn text(writer: &mut Writer<Vec<u8>>, name: &str, value: &str) -> Result<(), NleXmlError> {
    start(writer, name, &[])?;
    write(writer, Event::Text(BytesText::new(value)))?;
    end(writer, name)
}

fn finish(writer: Writer<Vec<u8>>) -> Result<String, NleXmlError> {
    String::from_utf8(writer.into_inner())
        .map_err(|error| NleXmlError::Serialization(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetId, ItemId, SceneId, TrackId};

    fn fixture() -> (ProjectDocument, BTreeMap<String, String>) {
        let mut document = ProjectDocument::new("nle-project".parse().unwrap(), "A & B");
        let mut asset = Asset::new(
            AssetId::new("asset:source").unwrap(),
            "Source <One> & Two",
            AssetKind::Video,
        );
        asset.has_audio = true;
        asset.duration_ticks = Some(240_000);
        document.assets.push(asset);
        let mut scene = Scene::new(SceneId::new("scene:main").unwrap(), "Main & Cut");
        scene.is_main = true;
        let mut track = Track::new(TrackId::new("track:v1").unwrap(), "V1", TrackKind::Video);
        let mut text_track =
            Track::new(TrackId::new("track:text").unwrap(), "Text", TrackKind::Text);
        track.items.push(TimelineItem::new(
            ItemId::new("item:one").unwrap(),
            "Clip <1>",
            0,
            120_000,
            ItemContent::Media {
                asset_id: AssetId::new("asset:source").unwrap(),
                media_kind: MediaKind::Video,
            },
        ));
        text_track.items.push(TimelineItem::new(
            ItemId::new("item:title").unwrap(),
            "Title",
            0,
            120_000,
            ItemContent::Text {
                text: "Hello".into(),
            },
        ));
        scene.tracks.push(track);
        scene.tracks.push(text_track);
        document.current_scene_id = Some(scene.id.clone());
        document.scenes.push(scene);
        (
            document,
            BTreeMap::from([(
                "asset:source".to_owned(),
                "file:///tmp/source%20video.mp4".to_owned(),
            )]),
        )
    }

    #[test]
    fn premiere_and_resolve_xml_are_escaped_and_report_unsupported_items() {
        let (document, uris) = fixture();
        for format in [NleFormat::PremiereXml, NleFormat::ResolveXml] {
            let exported = export_nle_xml(&document, format, &uris).unwrap();
            assert_eq!(exported.media_clip_count, 1);
            assert_eq!(exported.unsupported_item_ids, vec!["item:title"]);
            assert!(exported.content.contains("Source &lt;One&gt; &amp; Two"));
            assert!(exported.content.contains("file:///tmp/source%20video.mp4"));
            quick_xml::Reader::from_str(&exported.content)
                .read_event()
                .unwrap();
        }
    }
}
