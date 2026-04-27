use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, anyhow, bail};
use rayon::prelude::*;
use tempfile::TempDir;

use crate::mp4::{detect_dri_render_node, is_known_mp4_encoder, software_fallback_mp4_encoder};
use crate::thumbnail::render_frame_to_path;
use crate::types::{
    ExportFormat, ExportJob, ExportProfile, FrameItem, OriginalSizeReference, ResizeTarget,
};

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
            args.extend(build_mp4_args(profile)?);
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

pub fn resolved_export_size(frames: &[FrameItem], profile: &ExportProfile) -> Option<ResizeTarget> {
    match (profile.output_width, profile.output_height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(ResizeTarget { width, height })
        }
        _ => resolved_original_export_size(frames, profile.original_size_reference),
    }
}

pub fn resolved_original_export_size(
    frames: &[FrameItem],
    reference: OriginalSizeReference,
) -> Option<ResizeTarget> {
    let mut dimensions = frames
        .iter()
        .filter(|frame| frame.enabled)
        .filter_map(frame_effective_dimensions_for_export);
    let first = dimensions.next()?;
    let chosen = dimensions.fold(first, |current, candidate| {
        let current_area = u64::from(current.width) * u64::from(current.height);
        let candidate_area = u64::from(candidate.width) * u64::from(candidate.height);
        match reference {
            OriginalSizeReference::LargestFrame => {
                if candidate_area > current_area
                    || (candidate_area == current_area
                        && (candidate.width, candidate.height) > (current.width, current.height))
                {
                    candidate
                } else {
                    current
                }
            }
            OriginalSizeReference::SmallestFrame => {
                if candidate_area < current_area
                    || (candidate_area == current_area
                        && (candidate.width, candidate.height) < (current.width, current.height))
                {
                    candidate
                } else {
                    current
                }
            }
        }
    });
    Some(chosen)
}

fn frame_effective_dimensions_for_export(frame: &FrameItem) -> Option<ResizeTarget> {
    let (mut width, mut height) = frame.source_dimensions?;
    if frame.transform_spec.rotate_quarter_turns.rem_euclid(2) == 1 {
        (width, height) = (height, width);
    }
    if let Some(crop) = frame.transform_spec.crop {
        width = crop.width.max(1);
        height = crop.height.max(1);
    }
    Some(ResizeTarget { width, height })
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

    let resize_target = resolved_export_size(&enabled_frames, profile);
    let export_fit_mode = profile.fit_mode;

    on_progress(ExportProgress {
        phase: ExportPhase::PreparingFrames,
        fraction: 0.0,
        detail: format!("Preparing {} frame(s) for export...", enabled_frames.len()),
    });

    let total_frames = enabled_frames.len();
    let rendered_dir_for_thread = rendered_dir.clone();
    let enabled_frames_for_thread = enabled_frames.clone();
    let (progress_tx, progress_rx) = mpsc::channel();
    let render_thread = thread::Builder::new()
        .name("export-frame-render".to_string())
        .spawn(move || {
            enabled_frames_for_thread
                .par_iter()
                .enumerate()
                .map(|(index, frame)| -> anyhow::Result<(usize, PathBuf, u32)> {
                    let frame_path = rendered_dir_for_thread.join(format!("{index:05}.png"));
                    render_frame_to_path(frame, resize_target, export_fit_mode, &frame_path)?;
                    let _ = progress_tx.send((index, frame.file_name()));
                    Ok((index, frame_path, frame.duration_ms))
                })
                .collect::<Vec<_>>()
        })
        .context("spawn export frame render worker")?;

    let mut completed_frames = 0_usize;
    let mut last_prepare_bucket = None;
    while completed_frames < total_frames {
        let Ok((index, file_name)) = progress_rx.recv() else {
            break;
        };
        completed_frames += 1;
        let prepare_bucket = (completed_frames * PREPARE_PROGRESS_BUCKETS) / total_frames.max(1);
        let should_emit =
            last_prepare_bucket != Some(prepare_bucket) || completed_frames == total_frames;
        if should_emit {
            last_prepare_bucket = Some(prepare_bucket);
            on_progress(ExportProgress {
                phase: ExportPhase::PreparingFrames,
                fraction: 0.8 * (completed_frames as f64 / total_frames as f64),
                detail: format!(
                    "Rendered frame {} of {}: {}",
                    index + 1,
                    total_frames,
                    file_name
                ),
            });
        }
    }

    let rendered_results = render_thread
        .join()
        .map_err(|_| anyhow!("export frame render worker panicked"))?;
    let mut rendered_results = rendered_results
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;
    rendered_results.sort_unstable_by_key(|(index, _, _)| *index);
    let manifest_entries = rendered_results
        .into_iter()
        .map(|(_, frame_path, duration_ms)| (frame_path, duration_ms))
        .collect::<Vec<_>>();

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

fn build_mp4_args(profile: &ExportProfile) -> anyhow::Result<Vec<String>> {
    let selected_encoder = resolved_mp4_encoder_name(&profile.mp4_video_encoder);
    let selected_encoder_name = selected_encoder.as_str();
    let mut args = Vec::new();

    match selected_encoder_name {
        "hevc_nvenc" | "h264_nvenc" | "av1_nvenc" => {
            args.extend([
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-cq".to_string(),
                mp4_nvenc_cq_for_quality(profile.quality).to_string(),
                "-b:v".to_string(),
                "0".to_string(),
                "-preset".to_string(),
                "p5".to_string(),
                "-vf".to_string(),
                software_mp4_filter().to_string(),
            ]);
        }
        "hevc_qsv" | "h264_qsv" | "av1_qsv" => {
            let render_node = detect_dri_render_node()
                .ok_or_else(|| anyhow!("no /dev/dri render node available for Quick Sync"))?;
            args.extend([
                "-qsv_device".to_string(),
                render_node,
                "-vf".to_string(),
                hardware_mp4_filter().to_string(),
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-global_quality".to_string(),
                mp4_global_quality_for_quality(profile.quality).to_string(),
            ]);
        }
        "hevc_vaapi" | "h264_vaapi" | "av1_vaapi" => {
            let render_node = detect_dri_render_node()
                .ok_or_else(|| anyhow!("no /dev/dri render node available for VAAPI"))?;
            args.extend([
                "-vaapi_device".to_string(),
                render_node,
                "-vf".to_string(),
                hardware_mp4_filter().to_string(),
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-global_quality".to_string(),
                mp4_global_quality_for_quality(profile.quality).to_string(),
            ]);
        }
        "libsvtav1" => {
            args.extend([
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-crf".to_string(),
                mp4_crf_for_quality(profile.quality).to_string(),
                "-preset".to_string(),
                "6".to_string(),
                "-vf".to_string(),
                software_mp4_filter().to_string(),
            ]);
        }
        "libaom-av1" => {
            args.extend([
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-crf".to_string(),
                mp4_crf_for_quality(profile.quality).to_string(),
                "-b:v".to_string(),
                "0".to_string(),
                "-cpu-used".to_string(),
                "4".to_string(),
                "-vf".to_string(),
                software_mp4_filter().to_string(),
            ]);
        }
        "libx264" | "libx265" => {
            args.extend([
                "-c:v".to_string(),
                selected_encoder.clone(),
                "-crf".to_string(),
                mp4_crf_for_quality(profile.quality).to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-vf".to_string(),
                software_mp4_filter().to_string(),
            ]);
        }
        _ => {
            args.extend([
                "-c:v".to_string(),
                "libx265".to_string(),
                "-crf".to_string(),
                mp4_crf_for_quality(profile.quality).to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-vf".to_string(),
                software_mp4_filter().to_string(),
            ]);
        }
    }

    if matches!(
        selected_encoder_name,
        "hevc_nvenc" | "hevc_qsv" | "hevc_vaapi" | "libx265"
    ) {
        args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
    }

    args.extend(["-movflags".to_string(), "+faststart".to_string()]);
    Ok(args)
}

fn resolved_mp4_encoder_name(selected: &str) -> String {
    if is_known_mp4_encoder(selected) {
        match selected {
            "hevc_qsv" | "h264_qsv" | "av1_qsv" | "hevc_vaapi" | "h264_vaapi" | "av1_vaapi"
                if detect_dri_render_node().is_none() =>
            {
                software_fallback_mp4_encoder(selected).to_string()
            }
            _ => selected.to_string(),
        }
    } else {
        "libx265".to_string()
    }
}

fn software_mp4_filter() -> &'static str {
    "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"
}

fn hardware_mp4_filter() -> &'static str {
    "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=nv12,hwupload"
}

fn mp4_nvenc_cq_for_quality(quality: f32) -> u8 {
    let quality = quality.clamp(0.0, 100.0);
    (35.0 - (quality * 23.0 / 100.0)).round().clamp(10.0, 35.0) as u8
}

fn mp4_global_quality_for_quality(quality: f32) -> u8 {
    let quality = quality.clamp(0.0, 100.0);
    (35.0 - (quality * 23.0 / 100.0)).round().clamp(10.0, 35.0) as u8
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

    use crate::types::{EncoderPreset, ExportFormat, ExportPreset, FitMode, OriginalSizeReference};

    use super::{
        ExportPhase, build_effective_command, export_animation, export_animation_with_progress,
        normalized_output_path, resolved_export_size, resolved_original_export_size,
        write_concat_manifest,
    };

    fn tiny_png(path: &Path, color: [u8; 4]) {
        let image = RgbaImage::from_pixel(64, 64, Rgba(color));
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
            original_size_reference: OriginalSizeReference::LargestFrame,
            fit_mode: FitMode::Contain,
            quality: 80.0,
            lossless: false,
            encoder_preset: EncoderPreset::Photo,
            mp4_video_encoder: "libx265".to_string(),
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
            mp4_video_encoder: "libx265".to_string(),
            quality: 80.0,
            ..ExportProfile::default()
        };

        let args =
            build_effective_command(Path::new("frames.ffconcat"), Path::new("out.mp4"), &profile)
                .unwrap();

        assert!(args.contains(&"libx265".to_string()));
        assert!(args.contains(&"hvc1".to_string()));
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
            source_dimensions: Some((64, 64)),
        }];
        let profile = ExportProfile {
            format: ExportFormat::Mp4,
            mp4_video_encoder: "libx265".to_string(),
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
                source_dimensions: Some((64, 64)),
            },
            FrameItem {
                id: 2,
                source_path: second,
                duration_ms: 240,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((64, 64)),
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
            source_dimensions: Some((64, 64)),
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
                source_dimensions: Some((64, 64)),
            },
            FrameItem {
                id: 2,
                source_path: second,
                duration_ms: 240,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((64, 64)),
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

    #[test]
    fn resolved_original_export_size_uses_largest_or_smallest_frame() {
        let frames = vec![
            FrameItem {
                id: 1,
                source_path: Path::new("one.png").into(),
                duration_ms: 120,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((400, 300)),
            },
            FrameItem {
                id: 2,
                source_path: Path::new("two.png").into(),
                duration_ms: 120,
                transform_spec: TransformSpec {
                    rotate_quarter_turns: 1,
                    crop: Some(crate::types::CropRect {
                        x: 0,
                        y: 0,
                        width: 200,
                        height: 500,
                    }),
                    ..TransformSpec::default()
                },
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((900, 600)),
            },
        ];

        assert_eq!(
            resolved_original_export_size(&frames, OriginalSizeReference::LargestFrame),
            Some(crate::types::ResizeTarget {
                width: 400,
                height: 300,
            })
        );
        assert_eq!(
            resolved_original_export_size(&frames, OriginalSizeReference::SmallestFrame),
            Some(crate::types::ResizeTarget {
                width: 200,
                height: 500,
            })
        );
    }

    #[test]
    fn resolved_export_size_prefers_explicit_custom_dimensions() {
        let frames = vec![FrameItem {
            id: 1,
            source_path: Path::new("one.png").into(),
            duration_ms: 120,
            transform_spec: TransformSpec::default(),
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((400, 300)),
        }];
        let profile = ExportProfile {
            output_width: Some(1280),
            output_height: Some(720),
            ..ExportProfile::default()
        };

        assert_eq!(
            resolved_export_size(&frames, &profile),
            Some(crate::types::ResizeTarget {
                width: 1280,
                height: 720,
            })
        );
    }
}
