use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    pub ffmpeg_ok: bool,
    pub ffprobe_ok: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_version: Option<String>,
    pub issues: Vec<String>,
}

impl Diagnostics {
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "ffmpeg: {}",
            self.ffmpeg_version
                .as_deref()
                .unwrap_or(if self.ffmpeg_ok { "available" } else { "missing" })
        ));
        lines.push(format!(
            "ffprobe: {}",
            self.ffprobe_version
                .as_deref()
                .unwrap_or(if self.ffprobe_ok { "available" } else { "missing" })
        ));
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

    if !diagnostics.ffmpeg_ok {
        diagnostics
            .issues
            .push("ffmpeg not found in PATH; export will be unavailable".to_string());
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
    let line = String::from_utf8_lossy(&output.stdout).lines().next()?.to_string();
    Some(line)
}
