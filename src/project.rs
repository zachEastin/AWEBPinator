use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::types::ProjectDocument;

pub fn save_project(path: &Path, document: &ProjectDocument) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create project directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(document).context("serialize project")?;
    fs::write(path, json).with_context(|| format!("write project {}", path.display()))?;
    Ok(())
}

pub fn load_project(path: &Path) -> anyhow::Result<ProjectDocument> {
    let json =
        fs::read_to_string(path).with_context(|| format!("read project {}", path.display()))?;
    let document = serde_json::from_str(&json).context("parse project json")?;
    Ok(document)
}

pub fn save_autosave_project(document: &ProjectDocument) -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = autosave_project_path() else {
        return Ok(None);
    };
    save_project(&path, document)?;
    Ok(Some(path))
}

pub fn load_autosave_project() -> anyhow::Result<Option<ProjectDocument>> {
    let Some(path) = autosave_project_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    load_project(&path).map(Some)
}

fn autosave_project_path() -> Option<PathBuf> {
    state_root().map(|root| root.join("awebpinator").join("autosave.awebp.json"))
}

fn state_root() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::types::{ExportProfile, FrameItem, ProjectDocument, TransformSpec};

    use super::{load_project, save_project};

    #[test]
    fn project_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.awebp.json");
        let doc = ProjectDocument {
            frames: vec![FrameItem {
                id: 4,
                source_path: "demo.png".into(),
                duration_ms: 125,
                transform_spec: TransformSpec::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: Some((800, 600)),
            }],
            export_profile: ExportProfile::default(),
            last_output_path: Some("out.webp".into()),
        };

        save_project(&path, &doc).unwrap();
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded, doc);
    }
}
