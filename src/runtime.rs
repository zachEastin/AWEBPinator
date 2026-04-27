use std::collections::BTreeSet;
use std::process::Command;

use crate::mp4::{Mp4Capabilities, collect_mp4_capabilities};

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    pub ffmpeg_ok: bool,
    pub ffprobe_ok: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_version: Option<String>,
    pub mp4_capabilities: Mp4Capabilities,
    pub issues: Vec<String>,
}

impl Diagnostics {
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "ffmpeg: {}",
            self.ffmpeg_version.as_deref().unwrap_or(if self.ffmpeg_ok {
                "available"
            } else {
                "missing"
            })
        ));
        lines.push(format!(
            "ffprobe: {}",
            self.ffprobe_version
                .as_deref()
                .unwrap_or(if self.ffprobe_ok {
                    "available"
                } else {
                    "missing"
                })
        ));
        if let Some(preferred) = self
            .mp4_capabilities
            .preferred_encoder
            .as_deref()
            .and_then(crate::mp4::known_mp4_encoder_label)
        {
            lines.push(format!("Preferred MP4 encoder: {preferred}"));
        }
        if !self.mp4_capabilities.encoder_choices.is_empty() {
            let labels = self
                .mp4_capabilities
                .encoder_choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("Available MP4 encoders: {labels}"));
        }
        if !self.mp4_capabilities.hardware_accels.is_empty() {
            let hardware_accels = self
                .mp4_capabilities
                .hardware_accels
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("ffmpeg hardware acceleration: {hardware_accels}"));
        }
        if !self.issues.is_empty() {
            lines.push(format!("Issues: {}", self.issues.join(" | ")));
        }
        lines.join("\n")
    }
}

pub fn collect_diagnostics() -> Diagnostics {
    let mut diagnostics = Diagnostics::default();
    diagnostics.ffmpeg_version = command_version("ffmpeg");
    diagnostics.ffprobe_version = command_version("ffprobe");
    diagnostics.ffmpeg_ok = diagnostics.ffmpeg_version.is_some();
    diagnostics.ffprobe_ok = diagnostics.ffprobe_version.is_some();

    if diagnostics.ffmpeg_ok {
        diagnostics.mp4_capabilities =
            collect_mp4_capabilities(ffmpeg_encoder_names(), ffmpeg_hardware_accels());
    }

    if !diagnostics.ffmpeg_ok {
        diagnostics
            .issues
            .push("ffmpeg not found in PATH; export will be unavailable".to_string());
    } else if diagnostics.mp4_capabilities.encoder_choices.is_empty() {
        diagnostics
            .issues
            .push("No supported MP4 encoders were detected in this ffmpeg build.".to_string());
    }
    if !diagnostics.ffprobe_ok {
        diagnostics
            .issues
            .push("ffprobe not found in PATH; probe-based checks will be unavailable".to_string());
    }
    diagnostics
}

fn command_version(binary: &str) -> Option<String> {
    let output = Command::new(binary).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .to_string();
    Some(line)
}

fn ffmpeg_encoder_names() -> BTreeSet<String> {
    let Ok(output) = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return BTreeSet::new();
    };

    if !output.status.success() {
        return BTreeSet::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn ffmpeg_hardware_accels() -> BTreeSet<String> {
    let Ok(output) = Command::new("ffmpeg")
        .args(["-hide_banner", "-hwaccels"])
        .output()
    else {
        return BTreeSet::new();
    };

    if !output.status.success() {
        return BTreeSet::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip_while(|line| !line.trim().starts_with("Hardware acceleration methods"))
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
