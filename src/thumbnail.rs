use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, imageops};

use crate::app::PreviewRenderSize;
use crate::types::{CropRect, FitMode, FrameItem, ResizeTarget, TransformSpec};

static PREVIEW_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn ensure_cache_dir() -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join("awebpinator-cache");
    fs::create_dir_all(&path)
        .with_context(|| format!("create thumbnail cache {}", path.display()))?;
    Ok(path)
}

pub fn populate_frame_metadata(frame: &mut FrameItem) {
    if let Ok(image) = image::open(&frame.source_path) {
        frame.source_dimensions = Some(image.dimensions());
    }
}

pub fn refresh_thumbnail(frame: &mut FrameItem, cache_dir: &Path) -> anyhow::Result<()> {
    let image = image::open(&frame.source_path)
        .with_context(|| format!("open frame {}", frame.source_path.display()))?;
    let transformed = apply_transform(image, &frame.transform_spec, None);
    let thumbnail = transformed.thumbnail(160, 160);
    let target_path = cache_dir.join(format!("frame-{}.png", frame.id));
    thumbnail
        .save(&target_path)
        .with_context(|| format!("save thumbnail {}", target_path.display()))?;
    frame.thumbnail_path = Some(target_path);
    Ok(())
}

pub fn preview_cache_path(
    frame: &FrameItem,
    cache_dir: &Path,
    render_size: PreviewRenderSize,
) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    frame.source_path.hash(&mut hasher);
    frame.transform_spec.rotate_quarter_turns.hash(&mut hasher);
    frame.transform_spec.flip_horizontal.hash(&mut hasher);
    frame.transform_spec.flip_vertical.hash(&mut hasher);
    frame
        .transform_spec
        .crop
        .map(|crop| (crop.x, crop.y, crop.width, crop.height))
        .hash(&mut hasher);
    frame
        .transform_spec
        .resize
        .map(|resize| (resize.width, resize.height))
        .hash(&mut hasher);
    frame.transform_spec.fit_mode.as_str().hash(&mut hasher);
    render_size.width.hash(&mut hasher);
    render_size.height.hash(&mut hasher);
    let fingerprint = hasher.finish();

    cache_dir.join(format!("preview-{}-{fingerprint:016x}.png", frame.id))
}

pub fn render_preview(
    frame: &FrameItem,
    cache_dir: &Path,
    render_size: PreviewRenderSize,
) -> anyhow::Result<PathBuf> {
    let target_path = preview_cache_path(frame, cache_dir, render_size);
    if target_path.is_file() {
        return Ok(target_path);
    }

    let image = image::open(&frame.source_path)
        .with_context(|| format!("open frame {}", frame.source_path.display()))?;
    let transformed = apply_transform(image, &frame.transform_spec, None);
    let preview = transformed.thumbnail(render_size.width, render_size.height);
    let temporary_path = temporary_preview_path(&target_path);
    preview
        .save(&temporary_path)
        .with_context(|| format!("save preview {}", temporary_path.display()))?;
    fs::rename(&temporary_path, &target_path)
        .with_context(|| format!("publish preview {}", target_path.display()))?;
    Ok(target_path)
}

fn temporary_preview_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("preview.png");
    let suffix = PREVIEW_TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    target_path.with_file_name(format!(".{file_name}.{suffix}.tmp"))
}

pub fn render_frame_to_path(
    frame: &FrameItem,
    export_size: Option<ResizeTarget>,
    export_fit_mode: FitMode,
    target_path: &Path,
) -> anyhow::Result<()> {
    let image = image::open(&frame.source_path)
        .with_context(|| format!("open frame {}", frame.source_path.display()))?;
    let transformed = apply_transform(
        image,
        &frame.transform_spec,
        export_size.map(|target| (target, export_fit_mode)),
    );
    transformed
        .save(target_path)
        .with_context(|| format!("save transformed frame {}", target_path.display()))?;
    Ok(())
}

fn apply_transform(
    mut image: DynamicImage,
    transform: &TransformSpec,
    export_size: Option<(ResizeTarget, FitMode)>,
) -> DynamicImage {
    let turns = transform.rotate_quarter_turns.rem_euclid(4);
    image = match turns {
        1 => image.rotate90(),
        2 => image.rotate180(),
        3 => image.rotate270(),
        _ => image,
    };

    if transform.flip_horizontal {
        image = image.fliph();
    }
    if transform.flip_vertical {
        image = image.flipv();
    }
    if let Some(crop) = transform.crop {
        image = crop_image(image, crop);
    }

    if let Some(resize) = transform.resize {
        image = fit_image(image, resize, transform.fit_mode);
    }

    if let Some((resize, fit_mode)) = export_size {
        image = fit_image(image, resize, fit_mode);
    }

    image
}

fn crop_image(image: DynamicImage, crop: CropRect) -> DynamicImage {
    let (width, height) = image.dimensions();
    let x = crop.x.min(width.saturating_sub(1));
    let y = crop.y.min(height.saturating_sub(1));
    let max_width = width.saturating_sub(x);
    let max_height = height.saturating_sub(y);
    let crop_width = crop.width.min(max_width).max(1);
    let crop_height = crop.height.min(max_height).max(1);
    image.crop_imm(x, y, crop_width, crop_height)
}

fn fit_image(image: DynamicImage, resize: ResizeTarget, fit_mode: FitMode) -> DynamicImage {
    match fit_mode {
        FitMode::Stretch => {
            image.resize_exact(resize.width, resize.height, imageops::FilterType::Lanczos3)
        }
        FitMode::Contain => {
            image.resize(resize.width, resize.height, imageops::FilterType::Lanczos3)
        }
        FitMode::Cover => cover_image(image, resize),
    }
}

fn cover_image(image: DynamicImage, resize: ResizeTarget) -> DynamicImage {
    let resized = image.resize_to_fill(resize.width, resize.height, imageops::FilterType::Lanczos3);
    let mut canvas = RgbaImage::from_pixel(resize.width, resize.height, Rgba([0, 0, 0, 0]));
    imageops::overlay(&mut canvas, &resized.to_rgba8(), 0, 0);
    DynamicImage::ImageRgba8(canvas)
}
