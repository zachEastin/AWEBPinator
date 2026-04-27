use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp4CodecFamily {
    Hevc,
    H264,
    Av1,
}

impl Mp4CodecFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hevc => "H.265 / HEVC",
            Self::H264 => "H.264 / AVC",
            Self::Av1 => "AV1",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4EncoderChoice {
    pub ffmpeg_name: String,
    pub label: String,
    pub codec_family: Mp4CodecFamily,
    pub hardware: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Mp4Capabilities {
    pub available_encoders: BTreeSet<String>,
    pub hardware_accels: BTreeSet<String>,
    pub encoder_choices: Vec<Mp4EncoderChoice>,
    pub preferred_encoder: Option<String>,
    pub dri_render_node: Option<String>,
    pub nvidia_device_available: bool,
}

#[derive(Debug, Clone, Copy)]
struct KnownMp4Encoder {
    name: &'static str,
    label: &'static str,
    codec_family: Mp4CodecFamily,
    hardware: bool,
    backend: Option<&'static str>,
}

const KNOWN_MP4_ENCODERS: &[KnownMp4Encoder] = &[
    KnownMp4Encoder {
        name: "hevc_nvenc",
        label: "H.265 / HEVC (NVIDIA NVENC)",
        codec_family: Mp4CodecFamily::Hevc,
        hardware: true,
        backend: Some("cuda"),
    },
    KnownMp4Encoder {
        name: "hevc_qsv",
        label: "H.265 / HEVC (Intel Quick Sync)",
        codec_family: Mp4CodecFamily::Hevc,
        hardware: true,
        backend: Some("qsv"),
    },
    KnownMp4Encoder {
        name: "hevc_vaapi",
        label: "H.265 / HEVC (VAAPI)",
        codec_family: Mp4CodecFamily::Hevc,
        hardware: true,
        backend: Some("vaapi"),
    },
    KnownMp4Encoder {
        name: "libx265",
        label: "H.265 / HEVC (Software x265)",
        codec_family: Mp4CodecFamily::Hevc,
        hardware: false,
        backend: None,
    },
    KnownMp4Encoder {
        name: "h264_nvenc",
        label: "H.264 / AVC (NVIDIA NVENC)",
        codec_family: Mp4CodecFamily::H264,
        hardware: true,
        backend: Some("cuda"),
    },
    KnownMp4Encoder {
        name: "h264_qsv",
        label: "H.264 / AVC (Intel Quick Sync)",
        codec_family: Mp4CodecFamily::H264,
        hardware: true,
        backend: Some("qsv"),
    },
    KnownMp4Encoder {
        name: "h264_vaapi",
        label: "H.264 / AVC (VAAPI)",
        codec_family: Mp4CodecFamily::H264,
        hardware: true,
        backend: Some("vaapi"),
    },
    KnownMp4Encoder {
        name: "libx264",
        label: "H.264 / AVC (Software x264)",
        codec_family: Mp4CodecFamily::H264,
        hardware: false,
        backend: None,
    },
    KnownMp4Encoder {
        name: "av1_nvenc",
        label: "AV1 (NVIDIA NVENC)",
        codec_family: Mp4CodecFamily::Av1,
        hardware: true,
        backend: Some("cuda"),
    },
    KnownMp4Encoder {
        name: "av1_qsv",
        label: "AV1 (Intel Quick Sync)",
        codec_family: Mp4CodecFamily::Av1,
        hardware: true,
        backend: Some("qsv"),
    },
    KnownMp4Encoder {
        name: "av1_vaapi",
        label: "AV1 (VAAPI)",
        codec_family: Mp4CodecFamily::Av1,
        hardware: true,
        backend: Some("vaapi"),
    },
    KnownMp4Encoder {
        name: "libsvtav1",
        label: "AV1 (Software SVT-AV1)",
        codec_family: Mp4CodecFamily::Av1,
        hardware: false,
        backend: None,
    },
    KnownMp4Encoder {
        name: "libaom-av1",
        label: "AV1 (Software libaom)",
        codec_family: Mp4CodecFamily::Av1,
        hardware: false,
        backend: None,
    },
];

pub fn default_mp4_encoder_name() -> String {
    "libx265".to_string()
}

pub fn collect_mp4_capabilities(
    available_encoders: BTreeSet<String>,
    hardware_accels: BTreeSet<String>,
) -> Mp4Capabilities {
    let dri_render_node = detect_dri_render_node();
    let nvidia_device_available = Path::new("/dev/nvidia0").exists();
    collect_mp4_capabilities_with_detection(
        available_encoders,
        hardware_accels,
        dri_render_node,
        nvidia_device_available,
    )
}

fn collect_mp4_capabilities_with_detection(
    available_encoders: BTreeSet<String>,
    hardware_accels: BTreeSet<String>,
    dri_render_node: Option<String>,
    nvidia_device_available: bool,
) -> Mp4Capabilities {
    let encoder_choices = supported_mp4_encoder_choices(
        &available_encoders,
        &hardware_accels,
        dri_render_node.as_deref(),
        nvidia_device_available,
    );
    let preferred_encoder = encoder_choices
        .first()
        .map(|choice| choice.ffmpeg_name.clone());

    Mp4Capabilities {
        available_encoders,
        hardware_accels,
        encoder_choices,
        preferred_encoder,
        dri_render_node,
        nvidia_device_available,
    }
}

pub fn normalized_mp4_encoder(selected: &str, capabilities: &Mp4Capabilities) -> Option<String> {
    if capabilities
        .encoder_choices
        .iter()
        .any(|choice| choice.ffmpeg_name == selected)
    {
        return Some(selected.to_string());
    }

    capabilities.preferred_encoder.clone().or_else(|| {
        capabilities
            .encoder_choices
            .first()
            .map(|choice| choice.ffmpeg_name.clone())
    })
}

pub fn known_mp4_encoder_label(name: &str) -> Option<&'static str> {
    known_mp4_encoder(name).map(|encoder| encoder.label)
}

pub fn is_known_mp4_encoder(name: &str) -> bool {
    known_mp4_encoder(name).is_some()
}

pub fn software_fallback_mp4_encoder(name: &str) -> &'static str {
    match known_mp4_encoder(name).map(|encoder| encoder.codec_family) {
        Some(Mp4CodecFamily::H264) => "libx264",
        Some(Mp4CodecFamily::Av1) => "libsvtav1",
        _ => "libx265",
    }
}

pub fn detect_dri_render_node() -> Option<String> {
    let dri_dir = Path::new("/dev/dri");
    let entries = fs::read_dir(dri_dir).ok()?;
    let mut render_nodes: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            file_name
                .starts_with("renderD")
                .then_some(path.display().to_string())
        })
        .collect();
    render_nodes.sort();
    render_nodes.into_iter().next()
}

fn supported_mp4_encoder_choices(
    available_encoders: &BTreeSet<String>,
    hardware_accels: &BTreeSet<String>,
    dri_render_node: Option<&str>,
    nvidia_device_available: bool,
) -> Vec<Mp4EncoderChoice> {
    KNOWN_MP4_ENCODERS
        .iter()
        .filter(|encoder| {
            encoder_supported(
                encoder,
                available_encoders,
                hardware_accels,
                dri_render_node,
                nvidia_device_available,
            )
        })
        .map(|encoder| Mp4EncoderChoice {
            ffmpeg_name: encoder.name.to_string(),
            label: encoder.label.to_string(),
            codec_family: encoder.codec_family,
            hardware: encoder.hardware,
        })
        .collect()
}

fn encoder_supported(
    encoder: &KnownMp4Encoder,
    available_encoders: &BTreeSet<String>,
    hardware_accels: &BTreeSet<String>,
    dri_render_node: Option<&str>,
    nvidia_device_available: bool,
) -> bool {
    if !available_encoders.contains(encoder.name) {
        return false;
    }

    match encoder.backend {
        Some("cuda") => nvidia_device_available && hardware_accels.contains("cuda"),
        Some("qsv") => dri_render_node.is_some() && hardware_accels.contains("qsv"),
        Some("vaapi") => dri_render_node.is_some() && hardware_accels.contains("vaapi"),
        Some(_) => false,
        None => true,
    }
}

fn known_mp4_encoder(name: &str) -> Option<&'static KnownMp4Encoder> {
    KNOWN_MP4_ENCODERS
        .iter()
        .find(|encoder| encoder.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Mp4CodecFamily, collect_mp4_capabilities_with_detection, normalized_mp4_encoder,
        software_fallback_mp4_encoder,
    };

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn prefers_hevc_gpu_encoder_when_available() {
        let capabilities = collect_mp4_capabilities_with_detection(
            set(&["hevc_nvenc", "libx265", "libx264"]),
            set(&["cuda"]),
            None,
            true,
        );

        assert_eq!(
            capabilities.preferred_encoder.as_deref(),
            Some("hevc_nvenc")
        );
        assert_eq!(
            capabilities.encoder_choices[0].codec_family,
            Mp4CodecFamily::Hevc
        );
        assert!(capabilities.encoder_choices[0].hardware);
    }

    #[test]
    fn falls_back_to_software_when_selected_encoder_missing() {
        let capabilities = collect_mp4_capabilities_with_detection(
            set(&["libx265", "libx264"]),
            set(&[]),
            None,
            false,
        );

        assert_eq!(
            normalized_mp4_encoder("hevc_nvenc", &capabilities).as_deref(),
            Some("libx265")
        );
        assert_eq!(software_fallback_mp4_encoder("h264_nvenc"), "libx264");
    }
}
