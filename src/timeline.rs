use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::types::FrameItem;

#[derive(Debug, Default)]
pub struct MirroredLoopResult {
    pub inserted_ids: Vec<u64>,
    pub updated_endpoint_ids: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    frames: Vec<FrameItem>,
    next_id: u64,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            next_id: 1,
        }
    }

    pub fn from_frames(frames: Vec<FrameItem>) -> Self {
        let next_id = frames.iter().map(|frame| frame.id).max().unwrap_or(0) + 1;
        Self { frames, next_id }
    }

    pub fn frames(&self) -> &[FrameItem] {
        &self.frames
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn frames_mut(&mut self) -> &mut [FrameItem] {
        &mut self.frames
    }

    pub fn clone_with_new_id(&mut self, frame: &FrameItem) -> FrameItem {
        let mut cloned = frame.clone();
        cloned.id = self.next_id;
        self.next_id += 1;
        cloned
    }

    pub fn import_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<u64> {
        let imported = self.build_imported_frames(paths);
        let imported_ids = imported.iter().map(|frame| frame.id).collect();
        self.frames.extend(imported);
        imported_ids
    }

    pub fn prepend_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<u64> {
        let imported = self.build_imported_frames(paths);
        let imported_ids = imported.iter().map(|frame| frame.id).collect();
        self.frames.splice(0..0, imported);
        imported_ids
    }

    pub fn replace_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<u64> {
        self.frames.clear();
        self.next_id = 1;
        self.import_paths(paths)
    }

    pub fn selected_indices(selection: &BTreeSet<u64>, frames: &[FrameItem]) -> Vec<usize> {
        frames
            .iter()
            .enumerate()
            .filter_map(|(index, frame)| selection.contains(&frame.id).then_some(index))
            .collect()
    }

    pub fn remove_selected(&mut self, selection: &BTreeSet<u64>) {
        self.frames.retain(|frame| !selection.contains(&frame.id));
    }

    pub fn duplicate_selected(&mut self, selection: &BTreeSet<u64>) -> Vec<u64> {
        let indices = Self::selected_indices(selection, &self.frames);
        let mut inserted_ids = Vec::new();
        for (offset, index) in indices.into_iter().enumerate() {
            let source = self.frames[index + offset].clone();
            let new_frame = self.clone_with_new_id(&source);
            let insert_at = index + offset + 1;
            inserted_ids.push(new_frame.id);
            self.frames.insert(insert_at, new_frame);
        }
        inserted_ids
    }

    pub fn paste_after_selection(
        &mut self,
        selection: &BTreeSet<u64>,
        clipboard: &[FrameItem],
    ) -> Vec<u64> {
        if clipboard.is_empty() {
            return Vec::new();
        }
        let after_index = Self::selected_indices(selection, &self.frames)
            .into_iter()
            .max()
            .map(|index| index + 1)
            .unwrap_or(self.frames.len());
        let mut inserted_ids = Vec::new();
        for (offset, frame) in clipboard.iter().enumerate() {
            let new_frame = self.clone_with_new_id(frame);
            inserted_ids.push(new_frame.id);
            self.frames.insert(after_index + offset, new_frame);
        }
        inserted_ids
    }

    pub fn move_selection_up(&mut self, selection: &BTreeSet<u64>) {
        let indices = Self::selected_indices(selection, &self.frames);
        for index in indices {
            if index > 0 && !selection.contains(&self.frames[index - 1].id) {
                self.frames.swap(index - 1, index);
            }
        }
    }

    pub fn move_selection_down(&mut self, selection: &BTreeSet<u64>) {
        let mut indices = Self::selected_indices(selection, &self.frames);
        indices.reverse();
        for index in indices {
            if index + 1 < self.frames.len() && !selection.contains(&self.frames[index + 1].id) {
                self.frames.swap(index, index + 1);
            }
        }
    }

    pub fn apply_duration(&mut self, selection: &BTreeSet<u64>, duration_ms: u32) {
        for frame in &mut self.frames {
            if selection.contains(&frame.id) {
                frame.duration_ms = duration_ms;
            }
        }
    }

    pub fn move_frame_to_index(&mut self, frame_id: u64, target_index: usize) -> bool {
        let Some(current_index) = self.frames.iter().position(|frame| frame.id == frame_id) else {
            return false;
        };
        if self.frames.is_empty() {
            return false;
        }
        let clamped_target = target_index.min(self.frames.len());
        if current_index == clamped_target || current_index + 1 == clamped_target {
            return false;
        }
        let frame = self.frames.remove(current_index);
        let adjusted_target = if current_index < clamped_target {
            clamped_target.saturating_sub(1)
        } else {
            clamped_target
        };
        self.frames
            .insert(adjusted_target.min(self.frames.len()), frame);
        true
    }

    pub fn mirrored_loop_source(&self, selection: &BTreeSet<u64>) -> Vec<FrameItem> {
        let mut source = self.selection_or_all(selection);
        if source.len() > 1 {
            source.pop();
            source.remove(0);
        } else {
            source.clear();
        }
        source.reverse();
        source
    }

    pub fn append_copies(&mut self, source: &[FrameItem], repeats: u32) -> Vec<u64> {
        let mut inserted = Vec::new();
        for _ in 0..repeats.max(1) {
            for frame in source {
                let new_frame = self.clone_with_new_id(frame);
                inserted.push(new_frame.id);
                self.frames.push(new_frame);
            }
        }
        inserted
    }

    pub fn create_mirrored_loop(
        &mut self,
        selection: &BTreeSet<u64>,
        repeats: u32,
    ) -> MirroredLoopResult {
        let source_indices = self.selection_or_all_indices(selection);
        if source_indices.is_empty() {
            return MirroredLoopResult::default();
        }

        let updated_endpoint_ids = self.double_source_endpoint_durations(&source_indices);
        let source = self.mirrored_loop_source(selection);
        let inserted_ids = self.append_copies(&source, repeats);

        MirroredLoopResult {
            inserted_ids,
            updated_endpoint_ids,
        }
    }

    fn selection_or_all(&self, selection: &BTreeSet<u64>) -> Vec<FrameItem> {
        self.frames
            .iter()
            .filter(|frame| selection.is_empty() || selection.contains(&frame.id))
            .cloned()
            .collect()
    }

    fn selection_or_all_indices(&self, selection: &BTreeSet<u64>) -> Vec<usize> {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(index, frame)| {
                (selection.is_empty() || selection.contains(&frame.id)).then_some(index)
            })
            .collect()
    }

    fn double_source_endpoint_durations(&mut self, source_indices: &[usize]) -> Vec<u64> {
        let Some(&first_index) = source_indices.first() else {
            return Vec::new();
        };
        let last_index = *source_indices.last().unwrap_or(&first_index);

        let mut updated = Vec::new();
        {
            let first = &mut self.frames[first_index];
            first.duration_ms = first.duration_ms.saturating_mul(2);
            updated.push(first.id);
        }

        if last_index != first_index {
            let last = &mut self.frames[last_index];
            last.duration_ms = last.duration_ms.saturating_mul(2);
            updated.push(last.id);
        }

        updated
    }

    fn build_imported_frames(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<FrameItem> {
        let mut imported = Vec::new();
        for path in paths {
            let id = self.next_id;
            self.next_id += 1;
            imported.push(FrameItem {
                id,
                source_path: path,
                duration_ms: 100,
                transform_spec: Default::default(),
                thumbnail_path: None,
                enabled: true,
                source_dimensions: None,
            });
        }
        imported
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::Timeline;

    fn ids(set: &[u64]) -> BTreeSet<u64> {
        set.iter().copied().collect()
    }

    #[test]
    fn duplicate_and_paste_preserve_order() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);

        let selection = ids(&[imported[1]]);
        let inserted = timeline.duplicate_selected(&selection);
        assert_eq!(timeline.frames().len(), 4);
        assert_eq!(timeline.frames()[2].source_path, PathBuf::from("b.png"));

        let clipboard = vec![timeline.frames()[0].clone(), timeline.frames()[2].clone()];
        let pasted = timeline.paste_after_selection(&ids(&inserted), &clipboard);
        assert_eq!(pasted.len(), 2);
        assert_eq!(timeline.frames().len(), 6);
        assert_eq!(timeline.frames()[3].source_path, PathBuf::from("a.png"));
    }

    #[test]
    fn moving_and_duration_work_on_selection() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);
        let selection = ids(&[imported[2]]);
        timeline.move_selection_up(&selection);
        assert_eq!(timeline.frames()[1].source_path, PathBuf::from("c.png"));
        timeline.apply_duration(&selection, 250);
        assert_eq!(timeline.frames()[1].duration_ms, 250);
        timeline.move_selection_down(&selection);
        assert_eq!(timeline.frames()[2].source_path, PathBuf::from("c.png"));
    }

    #[test]
    fn mirrored_loop_uses_reversed_interior_only() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);
        let source = timeline.mirrored_loop_source(&ids(&imported));
        let names: Vec<_> = source.iter().map(|frame| frame.file_name()).collect();
        assert_eq!(names, vec!["b.png".to_string()]);
    }

    #[test]
    fn mirrored_loop_doubles_original_endpoints_before_appending() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);

        let result = timeline.create_mirrored_loop(&ids(&imported), 1);

        assert_eq!(result.inserted_ids.len(), 1);
        assert_eq!(result.updated_endpoint_ids, vec![imported[0], imported[2]]);
        assert_eq!(timeline.frames()[0].duration_ms, 200);
        assert_eq!(timeline.frames()[1].duration_ms, 100);
        assert_eq!(timeline.frames()[2].duration_ms, 200);
        assert_eq!(timeline.frames()[3].file_name(), "b.png");
    }

    #[test]
    fn move_frame_to_index_reorders_by_id() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            PathBuf::from("c.png"),
        ]);
        assert!(timeline.move_frame_to_index(imported[0], 3));
        let names: Vec<_> = timeline
            .frames()
            .iter()
            .map(|frame| frame.file_name())
            .collect();
        assert_eq!(names, vec!["b.png", "c.png", "a.png"]);
    }

    #[test]
    fn prepend_paths_keeps_new_images_at_front_in_order() {
        let mut timeline = Timeline::new();
        timeline.import_paths([PathBuf::from("c.png"), PathBuf::from("d.png")]);

        let imported = timeline.prepend_paths([PathBuf::from("a.png"), PathBuf::from("b.png")]);

        assert_eq!(imported.len(), 2);
        let names: Vec<_> = timeline
            .frames()
            .iter()
            .map(|frame| frame.file_name())
            .collect();
        assert_eq!(names, vec!["a.png", "b.png", "c.png", "d.png"]);
    }

    #[test]
    fn replace_paths_resets_existing_frames() {
        let mut timeline = Timeline::new();
        timeline.import_paths([PathBuf::from("old.png")]);

        let imported =
            timeline.replace_paths([PathBuf::from("new-a.png"), PathBuf::from("new-b.png")]);

        assert_eq!(imported, vec![1, 2]);
        let names: Vec<_> = timeline
            .frames()
            .iter()
            .map(|frame| frame.file_name())
            .collect();
        assert_eq!(names, vec!["new-a.png", "new-b.png"]);
    }

    #[test]
    fn append_copies_repeats_source_order() {
        let mut timeline = Timeline::new();
        let imported = timeline.import_paths([PathBuf::from("a.png"), PathBuf::from("b.png")]);

        let source = timeline.selection_or_all(&ids(&imported));
        let inserted = timeline.append_copies(&source, 3);

        assert_eq!(inserted.len(), 6);
        let names: Vec<_> = timeline
            .frames()
            .iter()
            .skip(2)
            .map(|frame| frame.file_name())
            .collect();
        assert_eq!(
            names,
            vec!["a.png", "b.png", "a.png", "b.png", "a.png", "b.png"]
        );
    }
}
