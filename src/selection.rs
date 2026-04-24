use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Plain,
    Ctrl,
    Shift,
    CtrlShift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionState {
    pub selection: BTreeSet<u64>,
    pub anchor_id: Option<u64>,
}

pub fn select_all(ordered_ids: &[u64], current_anchor_id: Option<u64>) -> SelectionState {
    let selection: BTreeSet<_> = ordered_ids.iter().copied().collect();
    let anchor_id = current_anchor_id
        .filter(|id| selection.contains(id))
        .or_else(|| ordered_ids.first().copied());
    SelectionState {
        selection,
        anchor_id,
    }
}

pub fn clear_selection() -> SelectionState {
    SelectionState {
        selection: BTreeSet::new(),
        anchor_id: None,
    }
}

pub fn invert_selection(
    ordered_ids: &[u64],
    current_selection: &BTreeSet<u64>,
    current_anchor_id: Option<u64>,
) -> SelectionState {
    let selection: BTreeSet<_> = ordered_ids
        .iter()
        .copied()
        .filter(|id| !current_selection.contains(id))
        .collect();
    let anchor_id = if selection.is_empty() {
        None
    } else {
        current_anchor_id
            .filter(|id| selection.contains(id))
            .or_else(|| {
                ordered_ids
                    .iter()
                    .copied()
                    .find(|id| selection.contains(id))
            })
    };
    SelectionState {
        selection,
        anchor_id,
    }
}

pub fn extend_selection_to(
    ordered_ids: &[u64],
    current_selection: &BTreeSet<u64>,
    current_anchor_id: Option<u64>,
    target_id: u64,
) -> SelectionState {
    let target_exists = ordered_ids.contains(&target_id);
    if !target_exists {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    }

    let Some(anchor_id) = current_anchor_id.filter(|id| ordered_ids.contains(id)) else {
        return SelectionState {
            selection: BTreeSet::from([target_id]),
            anchor_id: Some(target_id),
        };
    };
    let Some((start, end)) = range_bounds(ordered_ids, anchor_id, target_id) else {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    };
    let mut selection = current_selection.clone();
    selection.extend_from_slice(&ordered_ids[start..=end]);
    SelectionState {
        selection,
        anchor_id: Some(anchor_id),
    }
}

pub fn extend_selection_by_step(
    ordered_ids: &[u64],
    current_selection: &BTreeSet<u64>,
    current_anchor_id: Option<u64>,
    offset: isize,
) -> SelectionState {
    let Some(direction) = direction_step(offset) else {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    };

    if ordered_ids.is_empty() {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    }

    let anchor_id = current_anchor_id
        .filter(|id| ordered_ids.contains(id))
        .or_else(|| ordered_ids.first().copied());
    let Some(anchor_id) = anchor_id else {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    };
    let Some(anchor_index) = ordered_ids.iter().position(|id| *id == anchor_id) else {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    };

    let target =
        first_unselected_in_direction(ordered_ids, current_selection, anchor_index, direction)
            .unwrap_or(anchor_id);

    let mut selection = current_selection.clone();
    selection.insert(target);
    SelectionState {
        selection,
        anchor_id: Some(target),
    }
}

pub fn apply_selection(
    ordered_ids: &[u64],
    current_selection: &BTreeSet<u64>,
    current_anchor_id: Option<u64>,
    clicked_id: u64,
    mode: SelectionMode,
) -> SelectionState {
    let clicked_exists = ordered_ids.contains(&clicked_id);
    if !clicked_exists {
        return SelectionState {
            selection: current_selection.clone(),
            anchor_id: current_anchor_id,
        };
    }

    match mode {
        SelectionMode::Plain => SelectionState {
            selection: BTreeSet::from([clicked_id]),
            anchor_id: Some(clicked_id),
        },
        SelectionMode::Ctrl => {
            let mut selection = current_selection.clone();
            if !selection.insert(clicked_id) {
                selection.remove(&clicked_id);
            }
            SelectionState {
                selection,
                anchor_id: Some(clicked_id),
            }
        }
        SelectionMode::Shift | SelectionMode::CtrlShift => extend_selection_to(
            ordered_ids,
            current_selection,
            current_anchor_id,
            clicked_id,
        ),
    }
}

fn range_bounds(ordered_ids: &[u64], left_id: u64, right_id: u64) -> Option<(usize, usize)> {
    let left = ordered_ids.iter().position(|id| *id == left_id)?;
    let right = ordered_ids.iter().position(|id| *id == right_id)?;
    Some((left.min(right), left.max(right)))
}

fn direction_step(offset: isize) -> Option<isize> {
    if offset < 0 {
        Some(-1)
    } else if offset > 0 {
        Some(1)
    } else {
        None
    }
}

fn first_unselected_in_direction(
    ordered_ids: &[u64],
    current_selection: &BTreeSet<u64>,
    start_index: usize,
    direction: isize,
) -> Option<u64> {
    let mut index = start_index as isize + direction;
    while index >= 0 && (index as usize) < ordered_ids.len() {
        let candidate = ordered_ids[index as usize];
        if !current_selection.contains(&candidate) {
            return Some(candidate);
        }
        index += direction;
    }
    None
}

trait ExtendFromSlice {
    fn extend_from_slice(&mut self, slice: &[u64]);
}

impl ExtendFromSlice for BTreeSet<u64> {
    fn extend_from_slice(&mut self, slice: &[u64]) {
        self.extend(slice.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        SelectionMode, apply_selection, clear_selection, extend_selection_by_step,
        extend_selection_to, invert_selection, select_all,
    };

    fn set(ids: &[u64]) -> BTreeSet<u64> {
        ids.iter().copied().collect()
    }

    #[test]
    fn plain_click_selects_one_frame() {
        let state = apply_selection(&[1, 2, 3], &set(&[1, 2]), Some(1), 3, SelectionMode::Plain);
        assert_eq!(state.selection, set(&[3]));
        assert_eq!(state.anchor_id, Some(3));
    }

    #[test]
    fn ctrl_click_adds_unselected_frame() {
        let state = apply_selection(&[1, 2, 3], &set(&[1]), Some(1), 3, SelectionMode::Ctrl);
        assert_eq!(state.selection, set(&[1, 3]));
        assert_eq!(state.anchor_id, Some(3));
    }

    #[test]
    fn ctrl_click_removes_selected_frame() {
        let state = apply_selection(&[1, 2, 3], &set(&[1, 3]), Some(1), 3, SelectionMode::Ctrl);
        assert_eq!(state.selection, set(&[1]));
        assert_eq!(state.anchor_id, Some(3));
    }

    #[test]
    fn shift_click_adds_inclusive_range() {
        let state = apply_selection(
            &[1, 2, 3, 4, 5],
            &set(&[1]),
            Some(1),
            4,
            SelectionMode::Shift,
        );
        assert_eq!(state.selection, set(&[1, 2, 3, 4]));
        assert_eq!(state.anchor_id, Some(1));
    }

    #[test]
    fn ctrl_shift_click_is_also_additive_range() {
        let state = apply_selection(
            &[1, 2, 3, 4, 5],
            &set(&[1, 5]),
            Some(1),
            4,
            SelectionMode::CtrlShift,
        );
        assert_eq!(state.selection, set(&[1, 2, 3, 4, 5]));
        assert_eq!(state.anchor_id, Some(1));
    }

    #[test]
    fn shift_without_anchor_falls_back_to_single_selection() {
        let state = apply_selection(&[1, 2, 3], &set(&[1]), None, 2, SelectionMode::Shift);
        assert_eq!(state.selection, set(&[2]));
        assert_eq!(state.anchor_id, Some(2));
    }

    #[test]
    fn shift_uses_current_anchor_even_if_selection_differs() {
        let state = apply_selection(&[1, 2, 3, 4], &set(&[2]), Some(4), 1, SelectionMode::Shift);
        assert_eq!(state.selection, set(&[1, 2, 3, 4]));
        assert_eq!(state.anchor_id, Some(4));
    }

    #[test]
    fn keyboard_range_extension_is_additive() {
        let state = extend_selection_to(&[1, 2, 3, 4, 5], &set(&[1, 5]), Some(3), 4);
        assert_eq!(state.selection, set(&[1, 3, 4, 5]));
        assert_eq!(state.anchor_id, Some(3));
    }

    #[test]
    fn keyboard_range_extension_without_anchor_falls_back_to_single_selection() {
        let state = extend_selection_to(&[1, 2, 3], &set(&[1]), None, 2);
        assert_eq!(state.selection, set(&[2]));
        assert_eq!(state.anchor_id, Some(2));
    }

    #[test]
    fn keyboard_step_extension_selects_first_unselected_to_the_right() {
        let state = extend_selection_by_step(&[1, 2, 3, 4, 5], &set(&[3, 4]), Some(4), 1);
        assert_eq!(state.selection, set(&[3, 4, 5]));
        assert_eq!(state.anchor_id, Some(5));
    }

    #[test]
    fn keyboard_step_extension_selects_first_unselected_to_the_left() {
        let state = extend_selection_by_step(&[1, 2, 3, 4, 5], &set(&[3, 4, 5]), Some(5), -1);
        assert_eq!(state.selection, set(&[2, 3, 4, 5]));
        assert_eq!(state.anchor_id, Some(2));
    }

    #[test]
    fn keyboard_step_extension_skips_already_selected_frames_in_direction() {
        let state = extend_selection_by_step(&[1, 2, 3, 4, 5], &set(&[2, 3, 5]), Some(3), 1);
        assert_eq!(state.selection, set(&[2, 3, 4, 5]));
        assert_eq!(state.anchor_id, Some(4));
    }

    #[test]
    fn keyboard_step_extension_keeps_anchor_when_no_unselected_frame_exists() {
        let state = extend_selection_by_step(&[1, 2, 3], &set(&[1, 2, 3]), Some(3), 1);
        assert_eq!(state.selection, set(&[1, 2, 3]));
        assert_eq!(state.anchor_id, Some(3));
    }

    #[test]
    fn select_all_keeps_existing_anchor_when_possible() {
        let state = select_all(&[1, 2, 3], Some(2));
        assert_eq!(state.selection, set(&[1, 2, 3]));
        assert_eq!(state.anchor_id, Some(2));
    }

    #[test]
    fn select_all_uses_first_frame_when_anchor_missing() {
        let state = select_all(&[1, 2, 3], Some(9));
        assert_eq!(state.selection, set(&[1, 2, 3]));
        assert_eq!(state.anchor_id, Some(1));
    }

    #[test]
    fn clear_selection_resets_anchor() {
        let state = clear_selection();
        assert!(state.selection.is_empty());
        assert_eq!(state.anchor_id, None);
    }

    #[test]
    fn invert_selection_keeps_anchor_when_still_selected() {
        let state = invert_selection(&[1, 2, 3, 4], &set(&[2, 4]), Some(1));
        assert_eq!(state.selection, set(&[1, 3]));
        assert_eq!(state.anchor_id, Some(1));
    }

    #[test]
    fn invert_selection_uses_first_selected_frame_when_anchor_drops_out() {
        let state = invert_selection(&[1, 2, 3, 4], &set(&[2, 4]), Some(2));
        assert_eq!(state.selection, set(&[1, 3]));
        assert_eq!(state.anchor_id, Some(1));
    }

    #[test]
    fn invert_selection_of_full_selection_clears_anchor() {
        let state = invert_selection(&[1, 2, 3], &set(&[1, 2, 3]), Some(2));
        assert!(state.selection.is_empty());
        assert_eq!(state.anchor_id, None);
    }
}
