use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::mp4::default_mp4_encoder_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FitMode {
    #[default]
    Contain,
    Cover,
    Stretch,
}

impl FitMode {
    pub const ALL: [Self; 3] = [Self::Contain, Self::Cover, Self::Stretch];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contain => "Contain",
            Self::Cover => "Cover",
            Self::Stretch => "Stretch",
        }
    }
}

impl fmt::Display for FitMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncoderPreset {
    #[default]
    Default,
    Picture,
    Photo,
    Drawing,
    Icon,
    Text,
}

impl EncoderPreset {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Picture,
        Self::Photo,
        Self::Drawing,
        Self::Icon,
        Self::Text,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Picture => "Picture",
            Self::Photo => "Photo",
            Self::Drawing => "Drawing",
            Self::Icon => "Icon",
            Self::Text => "Text",
        }
    }

    pub fn ffmpeg_value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Picture => "picture",
            Self::Photo => "photo",
            Self::Drawing => "drawing",
            Self::Icon => "icon",
            Self::Text => "text",
        }
    }
}

impl fmt::Display for EncoderPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportFormat {
    #[default]
    WebP,
    Mp4,
}

impl ExportFormat {
    pub const ALL: [Self; 2] = [Self::WebP, Self::Mp4];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebP => "Animated WebP",
            Self::Mp4 => "MP4 Video",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::WebP => "webp",
            Self::Mp4 => "mp4",
        }
    }
}

impl fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportPreset {
    FastPreview,
    #[default]
    Balanced,
    HighQuality,
    Lossless,
}

impl ExportPreset {
    pub const ALL: [Self; 4] = [
        Self::FastPreview,
        Self::Balanced,
        Self::HighQuality,
        Self::Lossless,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FastPreview => "Fast Preview",
            Self::Balanced => "Balanced",
            Self::HighQuality => "High Quality",
            Self::Lossless => "Lossless",
        }
    }
}

impl fmt::Display for ExportPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeTarget {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OriginalSizeReference {
    SmallestFrame,
    #[default]
    LargestFrame,
}

impl OriginalSizeReference {
    pub const ALL: [Self; 2] = [Self::LargestFrame, Self::SmallestFrame];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LargestFrame => "Largest Frame",
            Self::SmallestFrame => "Smallest Frame",
        }
    }
}

impl fmt::Display for OriginalSizeReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TransformSpec {
    pub rotate_quarter_turns: i32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub crop: Option<CropRect>,
}

#[derive(Deserialize)]
struct TransformSpecSerde {
    #[serde(default)]
    rotate_quarter_turns: i32,
    #[serde(default)]
    flip_horizontal: bool,
    #[serde(default)]
    flip_vertical: bool,
    #[serde(default)]
    crop: Option<CropRect>,
    #[allow(dead_code)]
    #[serde(default)]
    resize: Option<ResizeTarget>,
    #[allow(dead_code)]
    #[serde(default)]
    fit_mode: FitMode,
}

impl<'de> Deserialize<'de> for TransformSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = TransformSpecSerde::deserialize(deserializer)?;
        Ok(Self {
            rotate_quarter_turns: value.rotate_quarter_turns,
            flip_horizontal: value.flip_horizontal,
            flip_vertical: value.flip_vertical,
            crop: value.crop,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameItem {
    pub id: u64,
    pub source_path: PathBuf,
    pub duration_ms: u32,
    pub transform_spec: TransformSpec,
    pub thumbnail_path: Option<PathBuf>,
    pub enabled: bool,
    pub source_dimensions: Option<(u32, u32)>,
}

impl FrameItem {
    pub fn file_name(&self) -> String {
        self.source_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.source_path.display().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportProfile {
    #[serde(default)]
    pub format: ExportFormat,
    pub preset: ExportPreset,
    pub output_width: Option<u32>,
    pub output_height: Option<u32>,
    #[serde(default)]
    pub original_size_reference: OriginalSizeReference,
    pub fit_mode: FitMode,
    pub quality: f32,
    pub lossless: bool,
    pub encoder_preset: EncoderPreset,
    #[serde(default = "default_mp4_encoder_name")]
    pub mp4_video_encoder: String,
    pub cr_threshold: u32,
    pub cr_size: u32,
    pub loop_count: u32,
    pub overwrite: bool,
    pub raw_args: String,
}

impl Default for ExportProfile {
    fn default() -> Self {
        Self::from_preset(ExportPreset::Balanced)
    }
}

impl ExportProfile {
    pub fn from_preset(preset: ExportPreset) -> Self {
        match preset {
            ExportPreset::FastPreview => Self {
                format: ExportFormat::WebP,
                preset,
                output_width: None,
                output_height: None,
                original_size_reference: OriginalSizeReference::LargestFrame,
                fit_mode: FitMode::Contain,
                quality: 45.0,
                lossless: false,
                encoder_preset: EncoderPreset::Default,
                mp4_video_encoder: default_mp4_encoder_name(),
                cr_threshold: 0,
                cr_size: 16,
                loop_count: 0,
                overwrite: true,
                raw_args: String::new(),
            },
            ExportPreset::Balanced => Self {
                format: ExportFormat::WebP,
                preset,
                output_width: None,
                output_height: None,
                original_size_reference: OriginalSizeReference::LargestFrame,
                fit_mode: FitMode::Contain,
                quality: 75.0,
                lossless: false,
                encoder_preset: EncoderPreset::Default,
                mp4_video_encoder: default_mp4_encoder_name(),
                cr_threshold: 0,
                cr_size: 16,
                loop_count: 0,
                overwrite: true,
                raw_args: String::new(),
            },
            ExportPreset::HighQuality => Self {
                format: ExportFormat::WebP,
                preset,
                output_width: None,
                output_height: None,
                original_size_reference: OriginalSizeReference::LargestFrame,
                fit_mode: FitMode::Contain,
                quality: 92.0,
                lossless: false,
                encoder_preset: EncoderPreset::Photo,
                mp4_video_encoder: default_mp4_encoder_name(),
                cr_threshold: 0,
                cr_size: 16,
                loop_count: 0,
                overwrite: true,
                raw_args: String::new(),
            },
            ExportPreset::Lossless => Self {
                format: ExportFormat::WebP,
                preset,
                output_width: None,
                output_height: None,
                original_size_reference: OriginalSizeReference::LargestFrame,
                fit_mode: FitMode::Contain,
                quality: 100.0,
                lossless: true,
                encoder_preset: EncoderPreset::Drawing,
                mp4_video_encoder: default_mp4_encoder_name(),
                cr_threshold: 0,
                cr_size: 16,
                loop_count: 0,
                overwrite: true,
                raw_args: String::new(),
            },
        }
    }

    pub fn apply_preset(&mut self, preset: ExportPreset) {
        let format = self.format;
        let raw_args = self.raw_args.clone();
        let output_width = self.output_width;
        let output_height = self.output_height;
        let original_size_reference = self.original_size_reference;
        let loop_count = self.loop_count;
        let overwrite = self.overwrite;
        let mp4_video_encoder = self.mp4_video_encoder.clone();
        *self = Self::from_preset(preset);
        self.format = format;
        self.raw_args = raw_args;
        self.output_width = output_width;
        self.output_height = output_height;
        self.original_size_reference = original_size_reference;
        self.loop_count = loop_count;
        self.overwrite = overwrite;
        self.mp4_video_encoder = mp4_video_encoder;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportJob {
    pub temp_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub output_path: PathBuf,
    pub effective_command: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectDocument {
    pub frames: Vec<FrameItem>,
    pub export_profile: ExportProfile,
    pub last_output_path: Option<PathBuf>,
}
