//! Godot-style undo/redo history (EditorInspector → History dock).
//!
//! Every undoable action pushes a FULL scene snapshot with the exact Godot
//! undo/redo action name (e.g. "Add forward lane", "Add RoadContainer").
//! `entries[0]` is the seeded "Initial state"; the cursor points at the
//! entry that is currently LIVE — entries above the cursor are the redo
//! tail (already undone in the History dock, dimmed like Godot's editor).
//!
//! Snapshots are coarse on purpose (the whole `SceneState` clone) — scenes
//! here are a few hundred points, so a full clone per action is cheap and
//! gives exact Godot semantics (jump to ANY entry, truncate redo on push).

use crate::bridge::SceneState;

/// Selection snapshot: (point sel (ci, pi), container sel, junction sel).
pub type HistorySel = (Option<(usize, usize)>, Option<usize>, Option<usize>);

/// One undo step: label + full scene + (optional) global state.
#[derive(Clone)]
pub struct HistoryEntry {
    /// exact Godot undo/redo action name ("Add next RoadPoint", …)
    pub label: String,
    pub scene: SceneState,
    /// [yaw, pitch, dist, target.x, target.z] when `history_global` is on
    pub cam: Option<[f64; 5]>,
    /// selection snapshot (points, containers, junctions)
    pub sel: Option<HistorySel>,
}

/// Bounded undo/redo stack with a moving cursor.
pub struct History {
    pub entries: Vec<HistoryEntry>,
    pub cursor: usize,
    /// Godot caps editor history at 100 steps too
    pub max: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// Seeds with one "Initial state" entry (the app overwrites its scene
    /// right after the demo script runs in `RoadkitApp::core`).
    pub fn new() -> Self {
        Self {
            entries: vec![HistoryEntry {
                label: "Initial state".to_string(),
                scene: SceneState::default(),
                cam: None,
                sel: None,
            }],
            cursor: 0,
            max: 100,
        }
    }

    /// Commit a new state: truncate the redo tail, push, cap at `max`.
    pub fn push(
        &mut self,
        label: impl Into<String>,
        scene: SceneState,
        cam: Option<[f64; 5]>,
        sel: Option<HistorySel>,
    ) {
        self.entries.truncate(self.cursor + 1);
        self.entries.push(HistoryEntry { label: label.into(), scene, cam, sel });
        if self.entries.len() > self.max {
            let drop_n = self.entries.len() - self.max;
            self.entries.drain(0..drop_n);
        }
        self.cursor = self.entries.len() - 1;
    }

    /// Step back one entry. Returns the entry that becomes live.
    pub fn undo(&mut self) -> Option<&HistoryEntry> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor)
    }

    /// Step forward one entry (only while a redo tail exists).
    pub fn redo(&mut self) -> Option<&HistoryEntry> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor)
    }

    /// Jump to any entry (the History dock rows).
    pub fn jump(&mut self, index: usize) -> Option<&HistoryEntry> {
        if index >= self.entries.len() {
            return None;
        }
        self.cursor = index;
        self.entries.get(index)
    }
}

/* ------------------------------------------------------------------ */
/* tests                                                               */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(n_curves: usize) -> SceneState {
        let mut sc = SceneState::default();
        for i in 0..n_curves {
            sc.curves
                .push(roads::model::RoadCurve::new(format!("curve {i}")));
        }
        sc
    }

    #[test]
    fn push_undo_redo_jump_semantics() {
        let mut h = History::new();
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.cursor, 0);
        assert_eq!(h.entries[0].label, "Initial state");

        // undo on a fresh history is a no-op
        assert!(h.undo().is_none());

        h.push("Add RoadContainer", scene(1), None, None);
        h.push("Add next RoadPoint", scene(2), Some([1.0, 0.5, 90.0, 3.0, 4.0]), None);
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.cursor, 2);

        // undo: back to "Add RoadContainer"
        let e = h.undo().expect("undo available");
        assert_eq!(e.label, "Add RoadContainer");
        assert_eq!(e.scene.curves.len(), 1);
        assert_eq!(h.cursor, 1);
        // undo again: back to the seed
        let e = h.undo().expect("undo available");
        assert_eq!(e.label, "Initial state");
        assert!(h.undo().is_none()); // bottomed out

        // redo twice replays the actions
        let e = h.redo().expect("redo available");
        assert_eq!(e.label, "Add RoadContainer");
        let e = h.redo().expect("redo available");
        assert_eq!(e.label, "Add next RoadPoint");
        assert_eq!(e.scene.curves.len(), 2);
        assert!(h.redo().is_none()); // topped out

        // jump to any row (History dock click)
        let e = h.jump(0).expect("jump valid");
        assert_eq!(e.label, "Initial state");
        assert_eq!(h.cursor, 0);
        assert!(h.jump(99).is_none(), "out-of-range jump refused");
        assert_eq!(h.cursor, 0, "failed jump must not move the cursor");
    }

    #[test]
    fn redo_tail_truncated_on_push() {
        let mut h = History::new();
        h.push("a", scene(1), None, None);
        h.push("b", scene(2), None, None);
        h.push("c", scene(3), None, None);
        assert_eq!(h.cursor, 3);
        // undo twice → entries 2.. are the redo tail
        h.undo();
        h.undo();
        assert_eq!(h.cursor, 1);
        // a new push must TRUNCATE the redo tail ("b", "c" are gone)
        h.push("d", scene(4), None, None);
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.entries[0].label, "Initial state");
        assert_eq!(h.entries[1].label, "a");
        assert_eq!(h.entries[2].label, "d");
        assert_eq!(h.cursor, 2);
        assert!(h.redo().is_none(), "redo tail was truncated");
    }

    #[test]
    fn cap_drops_oldest_and_keeps_cursor_last() {
        let mut h = History::new();
        h.max = 4;
        for i in 0..10 {
            h.push(format!("step {i}"), scene(i), None, None);
        }
        assert_eq!(h.entries.len(), 4, "history capped at max");
        // oldest 7 entries dropped: only "step 6..9" + the newest remain
        assert_eq!(h.entries[0].label, "step 6");
        assert_eq!(h.entries[3].label, "step 9");
        assert_eq!(h.cursor, 3, "cursor follows the newest entry");
        // undo chain walks the surviving window only
        assert_eq!(h.undo().unwrap().label, "step 8");
        assert_eq!(h.undo().unwrap().label, "step 7");
    }
}
