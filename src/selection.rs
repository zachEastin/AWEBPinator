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
        SelectionMode::Shift | SelectionMode::CtrlShift => {
            let Some(anchor_id) = current_anchor_id.filter(|id| ordered_ids.contains(id)) else {
                return SelectionState {
                    selection: BTreeSet::from([clicked_id]),
                    anchor_id: Some(clicked_id),
                };
            };
            let Some((start, end)) = range_bounds(ordered_ids, anchor_id, clicked_id) else {
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
    }
}

fn range_bounds(ordered_ids: &[u64], left_id: u64, right_id: u64) -> Option<(usize, usize)> {
    let left = ordered_ids.iter().position(|id| *id == left_id)?;
    let right = ordered_ids.iter().position(|id| *id == right_id)?;
    Some((left.min(right), left.max(right)))
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

    use super::{SelectionMode, apply_selection};

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
        let state = apply_selection(&[1, 2, 3, 4, 5], &set(&[1]), Some(1), 4, SelectionMode::Shift);
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
}
