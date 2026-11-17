//! Every id that is not scoped to the ui it lives in. egui keeps state
//! (animations, window places, scratch values) under these, and two things
//! drawn under one id in the same frame make egui paint a red warning. Keeping
//! them in one list makes such a clash easy to see, and the test below
//! catches two names written the same.
//!
//! Ids scoped to a parent ui (`id_salt`, `ui.id().with(..)`) stay where they
//! are used: they cannot clash outside that ui.

use eframe::egui::Id;
use std::fmt::Debug;
use std::hash::Hash;

macro_rules! fixed {
    ($($(#[$doc:meta])* $name:ident = $key:literal;)*) => {
        $(
            $(#[$doc])*
            pub fn $name() -> Id {
                Id::unique($key)
            }
        )*
        #[cfg(test)]
        const FIXED: &[&str] = &[$($key),*];
    };
}

macro_rules! keyed {
    ($($(#[$doc:meta])* $name:ident = $key:literal;)*) => {
        $(
            $(#[$doc])*
            pub fn $name(key: impl Hash + Debug) -> Id {
                Id::unique(($key, key))
            }
        )*
        #[cfg(test)]
        const KEYED: &[&str] = &[$($key),*];
    };
}

fixed! {
    // ---- windows and areas ----
    /// the window a new block is made in. On purpose it is also the frame
    /// window when a new block turns into a frame, so the window stays put
    create_popup = "create_popup";
    frame_editor = "frame_editor";
    event_editor = "event_editor";
    task_editor = "task_editor";
    new_task = "newtask";
    command = "command";
    /// test aid: a right-click menu shown in a window
    demo_menu = "menu";
    toast = "toast";
    quick_nav_area = "quicknav_area";
    panel_bottom = "panel_bottom";
    /// the Save, Cancel and Delete row of an editor window
    editor_buttons = "editor_buttons";
    /// the narrow strip of list icons shown while the task panel is closed
    rail = "rail";
    rail_bottom = "rail_bottom";
    /// the task panel while it grows out of the rail or shrinks back
    morph_panel = "morph_panel";
    morph = "morph";
    /// a list hanging beside the rail
    flyer = "flyer";
    list_edit = "list_edit";
    /// the layer the dragged block's ghost is painted on
    ghost = "ghost";

    // ---- state kept between frames ----
    quick_nav = "quicknav";
    undo_button = "undo_btn";
    settings_find = "settings_find";
    /// how wide the controls of the settings page are, for this frame
    control_w = "control_w";
    /// the text in the quick add page's try-it field
    quick_try = "quick_try";
    /// the tags typed on the frames page that get frame time first
    frame_tags = "frame_tags";
    /// a dragged task would not fit where it was dropped
    drag_misfit = "drag_misfit";
    /// the size and first day the time grid had last frame
    grid_shape = "grid_shape";
    /// which edge a dragged block is held at, and since when
    drag_edge = "drag_edge";
}

keyed! {
    /// a day in the quick-nav strip
    quick_nav_day = "qnd";
    /// a section heading of the task panel
    section = "sec";
    section_chevron = "chev";
    section_open = "open";
    /// a list heading lit while a task is dragged over it
    section_drop = "drop";
    list_dots = "listdots";
    list_add = "listadd";
    /// a list's icon on the rail
    rail_icon = "railicon";
    /// a task row of the panel
    task_row = "row";
    /// the small name field under a "+" beside a dropdown
    add_text = "add_text";
    due_glow = "due";
    /// a block on one day of the grid, for its settling motion
    block = "block";
    block_glow = "glow";
    task_ghost = "task_ghost";
    /// a colour in the block colour menu
    colour_dot = "dot";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_names_are_the_same() {
        let mut all: Vec<&str> = FIXED.iter().chain(KEYED).copied().collect();
        all.sort();
        let n = all.len();
        all.dedup();
        assert_eq!(all.len(), n, "two ids share a name");
    }
}
