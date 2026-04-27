use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow, bail};
use tempfile::TempDir;

use crate::thumbnail::render_frame_to_path;
use crate::types::{ExportFormat, ExportJob, ExportProfile, FrameItem, ResizeTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPhase {
    PreparingFrames,
    Encoding,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportProgress {
    pub phase: ExportPhase,
    pub fraction: f64,
    pub detail: String,
}

const PREPARE_PROGRESS_BUCKETS: usize = 24;

pub fn normalized_output_path(path: &Path) -> PathBuf {
    normalized_output_path_for_format(path, ExportFormat::WebP)
}

pub fn normalized_output_path_for_format(path: &Path, format: ExportFormat) -> PathBuf {
    if path.extension().is_some() {
        return path.to_path_buf();
    }
    path.with_extension(format.extension())
}

pub fn build_effective_command(
    manifest_path: &Path,
    output_path: &Path,
    profile: &ExportProfile,
) -> anyhow::Result<Vec<String>> {
    let mut args = vec![
        if profile.overwrite {
            "-y".to_string()
        } else {
            "-n".to_string()
        },
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-nostdin".to_string(),
        "-f".to_string(),
        "concat".to_string(),
        "-safe".to_string(),
        "0".to_string(),
        "-i".to_string(),
        manifest_path.display().to_string(),
    ];

    match profile.format {
        ExportFormat::WebP => {
            args.extend([
                "-c:v".to_string(),
                "libwebp_anim".to_string(),
                "-quality".to_string(),
                format!("{:.2}", profile.quality),
                "-preset".to_string(),
                profile.encoder_preset.ffmpeg_value().to_string(),
                "-loop".to_string(),
                profile.loop_count.to_string(),
                "-cr_threshold".to_string(),
                profile.cr_threshold.to_string(),
                "-cr_size".to_string(),
                profile.cr_size.to_string(),
            ]);

            if profile.lossless {
                args.push("-lossless".to_string());
                args.push("1".to_string());
            }
        }
        ExportFormat::Mp4 => {
            args.extend([
                "-c:v".to_string(),
                "libx264".to_string(),
                "-crf".to_string(),
                mp4_crf_for_quality(profile.quality).to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-vf".to_string(),
                "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p".to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
            ]);
        }
    }

    if !profile.raw_args.trim().is_empty() {
        let raw = shlex::split(&profile.raw_args)
            .ok_or_else(|| anyhow!("invalid raw ffmpeg arguments"))?;
        args.extend(raw);
    }

    args.push(output_path.display().to_string());
    Ok(args)
}

pub fn build_command_preview(
    manifest_path: &Path,
    output_path: &Path,
    profile: &ExportProfile,
) -> String {
    match build_effective_command(manifest_path, output_path, profile) {
        Ok(args) => format!("ffmpeg {}", shell_join(&args)),
        Err(err) => format!("Invalid advanced args: {err}"),
    }
}

pub fn export_animation(
    frames: &[FrameItem],
    profile: &ExportProfile,
    output_path: &Path,
) -> anyhow::Result<ExportJob> {
    export_animation_with_progress(frames, profile, output_path, |_| {})
}

pub fn export_animation_with_progress<F>(
    frames: &[FrameItem],
    profile: &ExportProfile,
    output_path: &Path,
    mut on_progress: F,
) -> anyhow::Result<ExportJob>
where
    F: FnMut(ExportProgress),
{
    let output_path = normalized_output_path_for_format(output_path, profile.format);
    let enabled_frames: Vec<_> = frames
        .iter()
        .filter(|frame| frame.enabled)
        .cloned()
        .collect();
    if enabled_frames.is_empty() {
        bail!("no enabled frames to export");
    }

    let temp_dir = TempDir::new().context("create export temp dir")?;
    let rendered_dir = temp_dir.path().join("frames");
    fs::create_dir_all(&rendered_dir).context("create rendered frame dir")?;

    let resize_target = match (profile.output_width, profile.output_height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(ResizeTarget { width, height })
        }
        _ => None,
    };

    on_progress(ExportProgress {
        phase: ExportPhase::PreparingFrames,
        fraction: 0.0,
        detail: format!("Preparing {} frame(s) for export...", enabled_frames.len()),
    });

    let total_frames = enabled_frames.len();
    let mut last_prepare_bucket = None;
    let mut manifest_entries = Vec::new();
    for (index, frame) in enabled_frames.iter().enumerate() {
        let frame_path = rendered_dir.join(format!("{index:05}.png"));
        render_frame_to_path(frame, resize_target, profile.fit_mode, &frame_path)?;
        manifest_entries.push((frame_path, frame.duration_ms));
        let prepare_bucket = ((index + 1) * PREPARE_PROGRESS_BUCKETS) / total_frames.max(1);
        let should_emit = last_prepare_bucket != Some(prepare_bucket) || index + 1 == total_frames;
        if should_emit {
            last_prepare_bucket = Some(prepare_bucket);
            on_progress(ExportProgress {
                phase: ExportPhase::PreparingFrames,
                fraction: 0.8 * ((index + 1) as f64 / total_frames as f64),
                detail: format!(
                    "Rendering frame {} of {}: {}",
                    index + 1,
                    total_frames,
                    frame.file_name()
                ),
            });
        }
    }

    let manifest_path = temp_dir.path().join("frames.ffconcat");
    write_concat_manifest(&manifest_path, &manifest_entries)?;
    let args = build_effective_command(&manifest_path, &output_path, profile)?;
    let effective_command = format!("ffmpeg {}", shell_join(&args));

    on_progress(ExportProgress {
        phase: ExportPhase::Encoding,
        fraction: 0.9,
        detail: format!("Encoding {}...", profile.format),
    });

    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .context("spawn ffmpeg")?;
    if !output.status.success() {
        let stderr_text = String::from_utf8_lossy(&output.stderr);
        let stderr_text = stderr_text.trim();
        if stderr_text.is_empty() {
            bail!("ffmpeg failed with status {}", output.status);
        }
        bail!("ffmpeg failed: {stderr_text}");
    }

    on_progress(ExportProgress {
        phase: ExportPhase::Encoding,
        fraction: 1.0,
        detail: "Export finished.".to_string(),
    });

    Ok(ExportJob {
        temp_dir: temp_dir.keep(),
        manifest_path,
        output_path,
        effective_command,
        status: "Export finished".to_string(),
    })
}

fn mp4_crf_for_quality(quality: f32) -> u8 {
    let quality = quality.clamp(0.0, 100.0);
    (35.0 - (quality * 23.0 / 100.0)).round().clamp(12.0, 35.0) as u8
}

pub fn write_concat_manifest(path: &Path, entries: &[(PathBuf, u32)]) -> anyhow::Result<()> {
    if entries.is_empty() {
        bail!("cannot write an empty concat manifest");
    }

    let mut manifest = String::from("ffconcat version 1.0\n");
    for (path_entry, duration_ms) in entries {
        writeln!(&mut manifest, "file '{}'", escape_manifest_path(path_entry)).unwrap();
        writeln!(
            &mut manifest,
            "duration {:.3}",
            *duration_ms as f32 / 1000.0
        )
        .unwrap();
    }
    writeln!(
        &mut manifest,
        "file '{}'",
        escape_manifest_path(&entries.last().unwrap().0)
    )
    .unwrap();
    fs::write(path, manifest)
        .with_context(|| format!("write concat manifest {}", path.display()))?;
    Ok(())
}

fn escape_manifest_path(path: &Path) -> String {
    path.display().to_string().replace('\'', "'\\''")
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_./=:".contains(ch))
            {
                arg.clone()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{Rgba, RgbaImage};
    use tempfile::tempdir;

    use crate::types::{EncoderPreset, ExportFormat, ExportPreset, FitMode};

    use super::{
        ExportPhase, build_effective_command, export_animation, export_animation_with_progress,
        normalized_output_path, write_concat_manifest,
    };

    fn tiny_png(path: &Path, color: [u8; 4]) {
        let image = RgbaImage::from_pixel(4, 4, Rgba(color));
        image.save(path).unwrap();
    }

    use crate::types::{ExportProfile, FrameItem, TransformSpec};
    use std::path::Path;

    #[test]
    fn manifest_contains_last_file_twice() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("frames.ffconcat");
        write_concat_manifest(
            &path,
            &[
                (dir.path().join("a.png"), 100),
                (dir.path().join("b.png"), 250),
            ],
        )
        .unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert_eq!(text.matches("file '").count(), 3);
        assert!(text.contains("duration 0.250"));
    }

    #[test]
    fn command_builder_includes_raw_args() {
        let profile = ExportProfile {
            format: ExportFormat::WebP,
            preset: ExportPreset::Balanced,
            output_width: Some(320),
            output_height: Some(240),
            fit_mode: FitMode::Contain,
            quality: 80.0,
            lossless: false,
            encoder_preset: EncoderPreset::Photo,
            cr_threshold: 0,
            cr_size: 16,
            loop_count: 0,
            overwrite: true,
            raw_args: "-metadata title=test".to_string(),
        };

        let args = build_effective_command(
            Path::new("frames.ffconcat"),
            Path::new("out.webp"),
            &profile,
        )
        .unwrap();
        assert!(args.contains(&"libwebp_anim".to_string()));
        assert!(args.contains(&"title=test".to_string()));
    }

    #[test]
    fn command_builder_can_target_mp4() {
        let profile = ExportProfile {
            format: ExportFormat::Mp4,
            quality: 80.0,
            ..ExportProfile::default()
        };

        let args =
            build_effective_command(Path::new("frames.ffconcat"), Path::new("out.mp4"), &profile)
                .unwrap();

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"+faststart".to_string()));
        assert!(!args.contains(&"libwebp_anim".to_string()));
    }

    #[test]
    fn output_path_adds_webp_extension_when_missing() {
        assert_eq!(
            normalized_output_path(Path::new("/tmp/demo")),
            Path::new("/tmp/demo.webp")
        );
        assert_eq!(
            normalized_output_path(Path::new("/tmp/demo.webp")),
            Path::new("/tmp/demo.webp")
        );
    }

    #[test]
    fn export_adds_mp4_extension_when_output_has_no_extension() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("one.png");
        tiny_png(&first, [255, 0, 0, 255]);

        let frames = vec![FrameItem {
            id: 1,
            source_path: first,
            duration_ms: 120,
            transform_spec: TransformSpec::default(),
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((4, 4)),
        }];
        let profile = ExportProfile {
            format: ExportFormat::Mp4,
            ..ExportProfile::default()
        };
        let output = dir.path().join("animation");
        let job = export_animation(&frames, &profile, &output).unwrap();
        assert_eq!(job.output_path, dir.path().join("animation.mp4"));
        assert!(job.output_path.exists());
    }

    #[test]
    fn export_generates_webp() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("one.png");
        let second = dir.path().join("two.png");
        tiny_png(&first, [255, 0, 0, 255]);
        tiny_png(&second, [0, 255, 0, 255]);

        let frames = vec![
            FrameItem {
                id: 1,
                source_path: first,
                duration_ms: 120,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((4, 4)),
            },
            FrameItem {
                id: 2,
                source_path: second,
                duration_ms: 240,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((4, 4)),
            },
        ];
        let output = dir.path().join("animation.webp");
        let job = export_animation(&frames, &ExportProfile::default(), &output).unwrap();
        assert!(job.output_path.exists());
    }

    #[test]
    fn export_adds_webp_extension_when_output_has_no_extension() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("one.png");
        tiny_png(&first, [255, 0, 0, 255]);

        let frames = vec![FrameItem {
            id: 1,
            source_path: first,
            duration_ms: 120,
            transform_spec: TransformSpec::default(),
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((4, 4)),
        }];
        let output = dir.path().join("animation");
        let job = export_animation(&frames, &ExportProfile::default(), &output).unwrap();
        assert_eq!(job.output_path, dir.path().join("animation.webp"));
        assert!(job.output_path.exists());
    }

    #[test]
    fn export_reports_progress_updates() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("one.png");
        let second = dir.path().join("two.png");
        tiny_png(&first, [255, 0, 0, 255]);
        tiny_png(&second, [0, 255, 0, 255]);

        let frames = vec![
            FrameItem {
                id: 1,
                source_path: first,
                duration_ms: 120,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((4, 4)),
            },
            FrameItem {
                id: 2,
                source_path: second,
                duration_ms: 240,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((4, 4)),
            },
        ];
        let output = dir.path().join("progress.webp");
        let mut progress_updates = Vec::new();

        let job = export_animation_with_progress(
            &frames,
            &ExportProfile::default(),
            &output,
            |progress| progress_updates.push(progress),
        )
        .unwrap();

        assert!(job.output_path.exists());
        assert!(!progress_updates.is_empty());
        assert_eq!(
            progress_updates.first().unwrap().phase,
            ExportPhase::PreparingFrames
        );
        assert!(
            progress_updates
                .iter()
                .any(|update| update.phase == ExportPhase::Encoding)
        );
        assert_eq!(progress_updates.last().unwrap().fraction, 1.0);
    }
}
