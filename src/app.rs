use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::export::{build_command_preview, export_animation};
use crate::project::{load_project, save_project};
use crate::runtime::{Diagnostics, collect_diagnostics};
use crate::selection::{SelectionMode, apply_selection};
use crate::thumbnail::{
    ensure_cache_dir, populate_frame_metadata, preview_cache_path, refresh_thumbnail,
    render_preview,
};
use crate::timeline::Timeline;
use crate::types::{
    CropRect, EncoderPreset, ExportJob, ExportPreset, ExportProfile, FitMode, FrameItem,
    ProjectDocument, ResizeTarget, TransformSpec,
};
use gtk::glib::clone;
use gtk::prelude::*;
use gtk::{gdk, gio};
use relm4::{Component, ComponentParts, ComponentSender, RelmApp};

const DEFAULT_PREVIEW_LOGICAL_WIDTH: i32 = 720;
const DEFAULT_PREVIEW_LOGICAL_HEIGHT: i32 = 360;
const MAX_PREVIEW_RENDER_EDGE: u32 = 4096;

pub fn run() {
    let app = RelmApp::new("dev.truevfx.awebpinator");
    app.run::<AppModel>(());
}

#[derive(Debug)]
pub enum AppMsg {
    ImportPaths(Vec<PathBuf>),
    ImportPathsWithMode {
        paths: Vec<PathBuf>,
        mode: ImportMode,
    },
    PreviewLayoutChanged(PreviewRenderSize),
    GoToBeginning,
    StepBackward,
    TogglePlayback,
    StepForward,
    GoToEnd,
    PlaybackAdvance {
        generation: u64,
    },
    SelectFrame {
        id: u64,
        mode: SelectionMode,
    },
    ToggleEnabled(u64, bool),
    SetFrameDuration(u64, u32),
    ApplyBatchDuration(u32),
    MoveSelectionUp,
    MoveSelectionDown,
    DropFrameAt {
        dragged_id: u64,
        target_index: usize,
    },
    DuplicateSelection,
    CopySelection,
    PasteClipboard,
    RemoveSelection,
    AppendDuplicateLoop,
    AppendReverseLoop(bool),
    RotateSelection(i32),
    ApplyInspectorTransform(InspectorValues),
    SetExportPreset(ExportPreset),
    SetOutputPath(String),
    SetOutputWidth(u32),
    SetOutputHeight(u32),
    SetQuality(f32),
    SetLossless(bool),
    SetEncoderPreset(EncoderPreset),
    SetCrThreshold(u32),
    SetCrSize(u32),
    SetLoopCount(u32),
    SetOverwrite(bool),
    SetExportFitMode(FitMode),
    SetRawArgs(String),
    SaveProject(PathBuf),
    OpenProject(PathBuf),
    ChooseOutputPath(PathBuf),
    ExportNow,
}

#[derive(Debug, Clone)]
pub enum CommandMsg {
    ThumbnailReady {
        frame_id: u64,
        thumbnail_path: Option<PathBuf>,
        dimensions: Option<(u32, u32)>,
        error: Option<String>,
    },
    PreviewReady {
        frame_id: u64,
        render_size: PreviewRenderSize,
        preview_path: Option<PathBuf>,
        error: Option<String>,
    },
    ExportFinished {
        result: Result<ExportJob, String>,
    },
}

#[derive(Debug, Clone)]
pub struct InspectorValues {
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub crop: Option<CropRect>,
    pub resize: Option<ResizeTarget>,
    pub fit_mode: FitMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Append,
    Prepend,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreviewRenderSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PreviewRenderSize {
    fn covers(self, other: Self) -> bool {
        self.width >= other.width && self.height >= other.height
    }
}

pub struct AppModel {
    timeline: Timeline,
    selection: BTreeSet<u64>,
    selection_anchor_id: Option<u64>,
    clipboard: Vec<FrameItem>,
    export_profile: ExportProfile,
    diagnostics: Diagnostics,
    status: String,
    cache_dir: PathBuf,
    last_output_path: Option<PathBuf>,
    command_preview: String,
    preview_path: Option<PathBuf>,
    preview_frame_id: Option<u64>,
    preview_target_size: PreviewRenderSize,
    preview_rendered_size: Option<PreviewRenderSize>,
    playback_active: bool,
    playback_generation: u64,
    thumbnails_pending: usize,
    export_in_progress: bool,
}

pub struct AppWidgets {
    timeline_strip: gtk::Box,
    nav_first_button: gtk::Button,
    nav_prev_button: gtk::Button,
    nav_play_button: gtk::Button,
    nav_next_button: gtk::Button,
    nav_last_button: gtk::Button,
    diagnostics_label: gtk::Label,
    selection_label: gtk::Label,
    status_label: gtk::Label,
    preview_picture: gtk::Picture,
    preview_meta: gtk::Label,
    output_entry: gtk::Entry,
    preset_combo: gtk::ComboBoxText,
    quality_spin: gtk::SpinButton,
    width_spin: gtk::SpinButton,
    height_spin: gtk::SpinButton,
    lossless_check: gtk::CheckButton,
    encoder_combo: gtk::ComboBoxText,
    cr_threshold_spin: gtk::SpinButton,
    cr_size_spin: gtk::SpinButton,
    loop_spin: gtk::SpinButton,
    overwrite_check: gtk::CheckButton,
    fit_mode_combo: gtk::ComboBoxText,
    raw_args_entry: gtk::Entry,
    command_preview_label: gtk::Label,
    flip_h_check: gtk::CheckButton,
    flip_v_check: gtk::CheckButton,
    crop_x: gtk::SpinButton,
    crop_y: gtk::SpinButton,
    crop_w: gtk::SpinButton,
    crop_h: gtk::SpinButton,
    resize_w: gtk::SpinButton,
    resize_h: gtk::SpinButton,
    inspector_fit_combo: gtk::ComboBoxText,
}

impl Component for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = CommandMsg;
    type Root = gtk::Window;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        gtk::Window::builder()
            .title("AWEBPinator")
            .default_width(1480)
            .default_height(820)
            .resizable(true)
            .build()
    }

    fn init(
        _init: Self::Init,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        install_app_css(&window);
        let cache_dir = ensure_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
        let diagnostics = collect_diagnostics();
        let mut model = AppModel {
            timeline: Timeline::new(),
            selection: BTreeSet::new(),
            selection_anchor_id: None,
            clipboard: Vec::new(),
            export_profile: ExportProfile::default(),
            diagnostics,
            status: "Import images to begin building an animated WebP.".to_string(),
            cache_dir,
            last_output_path: None,
            command_preview: String::new(),
            preview_path: None,
            preview_frame_id: None,
            preview_target_size: PreviewRenderSize {
                width: DEFAULT_PREVIEW_LOGICAL_WIDTH as u32,
                height: DEFAULT_PREVIEW_LOGICAL_HEIGHT as u32,
            },
            preview_rendered_size: None,
            playback_active: false,
            playback_generation: 0,
            thumbnails_pending: 0,
            export_in_progress: false,
        };
        model.recompute_command_preview();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        window.set_child(Some(&root));

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let import_button = gtk::Button::with_label("Import Images");
        let open_project_button = gtk::Button::with_label("Open Project");
        let save_project_button = gtk::Button::with_label("Save Project");
        let duplicate_button = gtk::Button::with_label("Duplicate");
        let copy_button = gtk::Button::with_label("Copy");
        let paste_button = gtk::Button::with_label("Paste");
        let remove_button = gtk::Button::with_label("Remove");
        let move_up_button = gtk::Button::with_label("Move Up");
        let move_down_button = gtk::Button::with_label("Move Down");
        let loop_dup_button = gtk::Button::with_label("Loop Duplicate");
        let loop_reverse_button = gtk::Button::with_label("Loop Reverse");
        let loop_ping_pong_button = gtk::Button::with_label("Ping-Pong");

        for button in [
            &import_button,
            &open_project_button,
            &save_project_button,
            &duplicate_button,
            &copy_button,
            &paste_button,
            &remove_button,
            &move_up_button,
            &move_down_button,
            &loop_dup_button,
            &loop_reverse_button,
            &loop_ping_pong_button,
        ] {
            header.append(button);
        }
        root.append(&header);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .wide_handle(true)
            .build();
        paned.set_vexpand(true);
        root.append(&paned);

        let left_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .build();
        paned.set_start_child(Some(&left_box));

        let selection_label = gtk::Label::new(Some("No frames selected"));
        selection_label.set_xalign(0.0);
        left_box.append(&selection_label);

        let batch_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let batch_duration_label = gtk::Label::new(Some("Batch duration (ms)"));
        let batch_duration_spin = gtk::SpinButton::with_range(10.0, 30_000.0, 5.0);
        batch_duration_spin.set_value(100.0);
        let batch_duration_button = gtk::Button::with_label("Apply");
        batch_box.append(&batch_duration_label);
        batch_box.append(&batch_duration_spin);
        batch_box.append(&batch_duration_button);
        left_box.append(&batch_box);

        let right_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_start(8)
            .width_request(450)
            .build();
        let right_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_width(420)
            .child(&right_box)
            .build();
        paned.set_end_child(Some(&right_scroll));

        let diagnostics_label = gtk::Label::new(None);
        diagnostics_label.set_xalign(0.0);
        diagnostics_label.set_selectable(true);
        diagnostics_label.set_wrap(true);
        right_box.append(&section("Diagnostics", &diagnostics_label));

        let preview_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .vexpand(true)
            .build();
        let preview_picture = gtk::Picture::new();
        preview_picture.set_size_request(720, 360);
        preview_picture.set_can_shrink(true);
        preview_picture.set_hexpand(true);
        preview_picture.set_vexpand(true);
        model.preview_target_size = preview_render_size_for_widget(&preview_picture);
        install_preview_layout_watch(&preview_picture, sender.clone());
        let preview_meta = gtk::Label::new(Some("Select a frame to inspect it."));
        preview_meta.set_xalign(0.0);
        preview_meta.set_wrap(true);
        preview_box.append(&preview_picture);
        preview_box.append(&preview_meta);
        left_box.append(&section("Selected Frame Preview", &preview_box));

        let transform_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        let rotate_left_button = gtk::Button::with_label("Rotate Left");
        let rotate_right_button = gtk::Button::with_label("Rotate Right");
        let flip_h_check = gtk::CheckButton::with_label("Flip H");
        let flip_v_check = gtk::CheckButton::with_label("Flip V");
        let crop_x = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_y = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_w = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_h = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let resize_w = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let resize_h = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let inspector_fit_combo = combo_for_fit_mode();
        let apply_transform_button = gtk::Button::with_label("Apply To Selection");
        let clear_transform_button = gtk::Button::with_label("Clear Crop/Resize");

        transform_grid.attach(&rotate_left_button, 0, 0, 1, 1);
        transform_grid.attach(&rotate_right_button, 1, 0, 1, 1);
        transform_grid.attach(&flip_h_check, 0, 1, 1, 1);
        transform_grid.attach(&flip_v_check, 1, 1, 1, 1);
        attach_labeled_spin(&transform_grid, "Crop X", &crop_x, 0, 2);
        attach_labeled_spin(&transform_grid, "Crop Y", &crop_y, 1, 2);
        attach_labeled_spin(&transform_grid, "Crop W", &crop_w, 0, 4);
        attach_labeled_spin(&transform_grid, "Crop H", &crop_h, 1, 4);
        attach_labeled_spin(&transform_grid, "Resize W", &resize_w, 0, 6);
        attach_labeled_spin(&transform_grid, "Resize H", &resize_h, 1, 6);
        transform_grid.attach(&gtk::Label::new(Some("Fit mode")), 0, 8, 1, 1);
        transform_grid.attach(&inspector_fit_combo, 1, 8, 1, 1);
        transform_grid.attach(&apply_transform_button, 0, 9, 1, 1);
        transform_grid.attach(&clear_transform_button, 1, 9, 1, 1);
        right_box.append(&section("Selection Edit", &transform_grid));

        let export_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        let output_entry = gtk::Entry::new();
        output_entry.set_placeholder_text(Some("/path/to/output.webp"));
        let browse_output_button = gtk::Button::with_label("Browse");
        let preset_combo = combo_for_export_preset();
        let width_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let height_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let quality_spin = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
        quality_spin.set_value(75.0);
        let lossless_check = gtk::CheckButton::with_label("Lossless");
        let encoder_combo = combo_for_encoder_preset();
        let cr_threshold_spin = gtk::SpinButton::with_range(0.0, 1024.0, 1.0);
        let cr_size_spin = gtk::SpinButton::with_range(0.0, 256.0, 1.0);
        cr_size_spin.set_value(16.0);
        let loop_spin = gtk::SpinButton::with_range(0.0, 9999.0, 1.0);
        let overwrite_check = gtk::CheckButton::with_label("Overwrite");
        overwrite_check.set_active(true);
        let fit_mode_combo = combo_for_fit_mode();
        let raw_args_entry = gtk::Entry::new();
        raw_args_entry.set_placeholder_text(Some("-metadata title='Animated export'"));
        let export_button = gtk::Button::with_label("Export Animated WebP");

        export_grid.attach(&gtk::Label::new(Some("Output path")), 0, 0, 1, 1);
        export_grid.attach(&output_entry, 0, 1, 1, 1);
        export_grid.attach(&browse_output_button, 1, 1, 1, 1);
        export_grid.attach(&gtk::Label::new(Some("Preset")), 0, 2, 1, 1);
        export_grid.attach(&preset_combo, 1, 2, 1, 1);
        attach_labeled_spin(&export_grid, "Width", &width_spin, 0, 3);
        attach_labeled_spin(&export_grid, "Height", &height_spin, 1, 3);
        attach_labeled_spin(&export_grid, "Quality", &quality_spin, 0, 5);
        export_grid.attach(&lossless_check, 1, 5, 1, 1);
        export_grid.attach(&gtk::Label::new(Some("Encoder preset")), 0, 6, 1, 1);
        export_grid.attach(&encoder_combo, 1, 6, 1, 1);
        attach_labeled_spin(&export_grid, "CR threshold", &cr_threshold_spin, 0, 7);
        attach_labeled_spin(&export_grid, "CR size", &cr_size_spin, 1, 7);
        attach_labeled_spin(&export_grid, "Loop count", &loop_spin, 0, 9);
        export_grid.attach(&overwrite_check, 1, 9, 1, 1);
        export_grid.attach(&gtk::Label::new(Some("Export fit mode")), 0, 10, 1, 1);
        export_grid.attach(&fit_mode_combo, 1, 10, 1, 1);
        export_grid.attach(&gtk::Label::new(Some("Advanced ffmpeg args")), 0, 11, 1, 1);
        export_grid.attach(&raw_args_entry, 0, 12, 2, 1);
        export_grid.attach(&export_button, 0, 13, 2, 1);
        right_box.append(&section("Export", &export_grid));

        let command_preview_label = gtk::Label::new(None);
        command_preview_label.set_xalign(0.0);
        command_preview_label.set_wrap(true);
        command_preview_label.set_selectable(true);
        right_box.append(&section("Effective Command", &command_preview_label));

        let timeline_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let transport_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();
        let nav_first_button = build_icon_button(
            "media-skip-backward-symbolic",
            "Go to beginning (Ctrl+Left)",
        );
        let nav_prev_button =
            build_icon_button("media-seek-backward-symbolic", "Go back one frame (Left)");
        let nav_play_button = build_icon_button(
            "media-playback-start-symbolic",
            "Play or pause preview playback (Space)",
        );
        let nav_next_button = build_icon_button(
            "media-seek-forward-symbolic",
            "Go forward one frame (Right)",
        );
        let nav_last_button =
            build_icon_button("media-skip-forward-symbolic", "Go to end (Ctrl+Right)");
        for button in [
            &nav_first_button,
            &nav_prev_button,
            &nav_play_button,
            &nav_next_button,
            &nav_last_button,
        ] {
            transport_box.append(button);
        }
        let timeline_hint = gtk::Label::new(Some(
            "Timeline: drag thumbnails to reorder. Drop image files anywhere in the window to import.",
        ));
        timeline_hint.set_xalign(0.0);
        let timeline_strip = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .hexpand(true)
            .build();
        let frame_scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .min_content_height(150)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&timeline_strip)
            .build();
        timeline_box.append(&transport_box);
        timeline_box.append(&timeline_hint);
        timeline_box.append(&frame_scroll);
        root.append(&timeline_box);

        let status_label = gtk::Label::new(None);
        status_label.set_xalign(0.0);
        status_label.set_wrap(true);
        root.append(&status_label);

        let key_controller = gtk::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        key_controller.connect_key_pressed(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_, key, _, state| {
                if !should_handle_timeline_shortcuts(&window) {
                    return false.into();
                }

                let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
                let handled = match (ctrl, key) {
                    (true, gdk::Key::c) | (true, gdk::Key::C) => {
                        sender.input(AppMsg::CopySelection);
                        true
                    }
                    (true, gdk::Key::v) | (true, gdk::Key::V) => {
                        sender.input(AppMsg::PasteClipboard);
                        true
                    }
                    (_, gdk::Key::Delete) | (_, gdk::Key::KP_Delete) => {
                        sender.input(AppMsg::RemoveSelection);
                        true
                    }
                    (true, gdk::Key::Left) | (true, gdk::Key::KP_Left) => {
                        sender.input(AppMsg::GoToBeginning);
                        true
                    }
                    (false, gdk::Key::Left) | (false, gdk::Key::KP_Left) => {
                        sender.input(AppMsg::StepBackward);
                        true
                    }
                    (false, gdk::Key::space) | (false, gdk::Key::KP_Space) => {
                        sender.input(AppMsg::TogglePlayback);
                        true
                    }
                    (false, gdk::Key::Right) | (false, gdk::Key::KP_Right) => {
                        sender.input(AppMsg::StepForward);
                        true
                    }
                    (true, gdk::Key::Right) | (true, gdk::Key::KP_Right) => {
                        sender.input(AppMsg::GoToEnd);
                        true
                    }
                    _ => false,
                };

                handled.into()
            }
        ));
        window.add_controller(key_controller);

        import_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| open_image_dialog(&window, sender.clone())
        ));
        open_project_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| open_project_dialog(&window, sender.clone())
        ));
        save_project_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| save_project_dialog(&window, sender.clone())
        ));
        browse_output_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| choose_export_dialog(&window, sender.clone())
        ));

        duplicate_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::DuplicateSelection)
        ));
        copy_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::CopySelection)
        ));
        paste_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::PasteClipboard)
        ));
        remove_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::RemoveSelection)
        ));
        move_up_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::MoveSelectionUp)
        ));
        move_down_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::MoveSelectionDown)
        ));
        loop_dup_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::AppendDuplicateLoop)
        ));
        loop_reverse_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::AppendReverseLoop(true))
        ));
        loop_ping_pong_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::AppendReverseLoop(false))
        ));
        nav_first_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::GoToBeginning)
        ));
        nav_prev_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::StepBackward)
        ));
        nav_play_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::TogglePlayback)
        ));
        nav_next_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::StepForward)
        ));
        nav_last_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::GoToEnd)
        ));
        batch_duration_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            batch_duration_spin,
            move |_| sender.input(AppMsg::ApplyBatchDuration(
                batch_duration_spin.value() as u32
            ))
        ));
        rotate_left_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::RotateSelection(-1))
        ));
        rotate_right_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::RotateSelection(1))
        ));
        apply_transform_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            flip_h_check,
            #[strong]
            flip_v_check,
            #[strong]
            crop_x,
            #[strong]
            crop_y,
            #[strong]
            crop_w,
            #[strong]
            crop_h,
            #[strong]
            resize_w,
            #[strong]
            resize_h,
            #[strong]
            inspector_fit_combo,
            move |_| {
                sender.input(AppMsg::ApplyInspectorTransform(InspectorValues {
                    flip_horizontal: flip_h_check.is_active(),
                    flip_vertical: flip_v_check.is_active(),
                    crop: crop_from_widgets(&crop_x, &crop_y, &crop_w, &crop_h),
                    resize: resize_from_widgets(&resize_w, &resize_h),
                    fit_mode: fit_mode_from_combo(&inspector_fit_combo),
                }));
            }
        ));
        clear_transform_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            crop_x,
            #[strong]
            crop_y,
            #[strong]
            crop_w,
            #[strong]
            crop_h,
            #[strong]
            resize_w,
            #[strong]
            resize_h,
            #[strong]
            flip_h_check,
            #[strong]
            flip_v_check,
            move |_| {
                set_spin_if_needed(&crop_x, 0.0);
                set_spin_if_needed(&crop_y, 0.0);
                set_spin_if_needed(&crop_w, 0.0);
                set_spin_if_needed(&crop_h, 0.0);
                set_spin_if_needed(&resize_w, 0.0);
                set_spin_if_needed(&resize_h, 0.0);
                set_check_if_needed(&flip_h_check, false);
                set_check_if_needed(&flip_v_check, false);
                sender.input(AppMsg::ApplyInspectorTransform(InspectorValues {
                    flip_horizontal: false,
                    flip_vertical: false,
                    crop: None,
                    resize: None,
                    fit_mode: FitMode::Contain,
                }));
            }
        ));

        preset_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| sender.input(AppMsg::SetExportPreset(export_preset_from_combo(combo)))
        ));
        output_entry.connect_changed(clone!(
            #[strong]
            sender,
            move |entry| sender.input(AppMsg::SetOutputPath(entry.text().to_string()))
        ));
        width_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetOutputWidth(spin.value() as u32))
        ));
        height_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetOutputHeight(spin.value() as u32))
        ));
        quality_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetQuality(spin.value() as f32))
        ));
        lossless_check.connect_toggled(clone!(
            #[strong]
            sender,
            move |check| sender.input(AppMsg::SetLossless(check.is_active()))
        ));
        encoder_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| sender.input(AppMsg::SetEncoderPreset(encoder_from_combo(combo)))
        ));
        cr_threshold_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetCrThreshold(spin.value() as u32))
        ));
        cr_size_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetCrSize(spin.value() as u32))
        ));
        loop_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetLoopCount(spin.value() as u32))
        ));
        overwrite_check.connect_toggled(clone!(
            #[strong]
            sender,
            move |check| sender.input(AppMsg::SetOverwrite(check.is_active()))
        ));
        fit_mode_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| sender.input(AppMsg::SetExportFitMode(fit_mode_from_combo(combo)))
        ));
        raw_args_entry.connect_changed(clone!(
            #[strong]
            sender,
            move |entry| sender.input(AppMsg::SetRawArgs(entry.text().to_string()))
        ));
        export_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ExportNow)
        ));

        install_import_drop_targets(&root, sender.clone());

        let widgets = AppWidgets {
            timeline_strip,
            nav_first_button,
            nav_prev_button,
            nav_play_button,
            nav_next_button,
            nav_last_button,
            diagnostics_label,
            selection_label,
            status_label,
            preview_picture,
            preview_meta,
            output_entry,
            preset_combo,
            quality_spin,
            width_spin,
            height_spin,
            lossless_check,
            encoder_combo,
            cr_threshold_spin,
            cr_size_spin,
            loop_spin,
            overwrite_check,
            fit_mode_combo,
            raw_args_entry,
            command_preview_label,
            flip_h_check,
            flip_v_check,
            crop_x,
            crop_y,
            crop_w,
            crop_h,
            resize_w,
            resize_h,
            inspector_fit_combo,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::ImportPaths(paths) => {
                let valid = filter_image_paths(paths);
                if valid.is_empty() {
                    self.status = "No supported image files were provided.".to_string();
                } else if self.timeline.is_empty() {
                    self.import_paths(valid, ImportMode::Append, &sender);
                } else {
                    choose_import_mode(root, sender.clone(), valid);
                }
            }
            AppMsg::ImportPathsWithMode { paths, mode } => {
                self.import_paths(paths, mode, &sender);
            }
            AppMsg::PreviewLayoutChanged(size) => {
                if size != self.preview_target_size {
                    self.preview_target_size = size;
                    if self.primary_selected_id().is_some()
                        && should_refresh_preview(
                            self.preview_rendered_size,
                            self.preview_target_size,
                        )
                    {
                        self.queue_preview_for_primary_selection(&sender);
                    }
                }
            }
            AppMsg::GoToBeginning => self.navigate_to_boundary(false, &sender),
            AppMsg::StepBackward => self.navigate_by_step(-1, &sender),
            AppMsg::TogglePlayback => self.toggle_playback(&sender),
            AppMsg::StepForward => self.navigate_by_step(1, &sender),
            AppMsg::GoToEnd => self.navigate_to_boundary(true, &sender),
            AppMsg::PlaybackAdvance { generation } => {
                if generation == self.playback_generation && self.playback_active {
                    if let Some(next_id) = self.following_frame_id() {
                        self.select_single_frame(next_id, &sender);
                        self.schedule_playback_advance(generation, &sender);
                    } else {
                        self.playback_active = false;
                        self.playback_generation = self.playback_generation.wrapping_add(1);
                        self.status = "Playback finished.".to_string();
                    }
                }
            }
            AppMsg::SelectFrame { id, mode } => {
                let ordered_ids: Vec<_> = self
                    .timeline
                    .frames()
                    .iter()
                    .map(|frame| frame.id)
                    .collect();
                let next = apply_selection(
                    &ordered_ids,
                    &self.selection,
                    self.selection_anchor_id,
                    id,
                    mode,
                );
                self.selection = next.selection;
                self.selection_anchor_id = next.anchor_id;
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::ToggleEnabled(id, enabled) => {
                if let Some(frame) = self.frame_mut(id) {
                    frame.enabled = enabled;
                }
            }
            AppMsg::SetFrameDuration(id, duration) => {
                if let Some(frame) = self.frame_mut(id) {
                    frame.duration_ms = duration.max(10);
                }
            }
            AppMsg::ApplyBatchDuration(duration) => {
                self.timeline
                    .apply_duration(&self.selection, duration.max(10));
                self.status = format!("Applied {} ms to selected frames.", duration.max(10));
            }
            AppMsg::MoveSelectionUp => {
                self.stop_playback(None);
                self.timeline.move_selection_up(&self.selection)
            }
            AppMsg::MoveSelectionDown => {
                self.stop_playback(None);
                self.timeline.move_selection_down(&self.selection)
            }
            AppMsg::DropFrameAt {
                dragged_id,
                target_index,
            } => {
                self.stop_playback(None);
                if self.timeline.move_frame_to_index(dragged_id, target_index) {
                    self.status = "Reordered frame.".to_string();
                }
            }
            AppMsg::DuplicateSelection => {
                self.stop_playback(None);
                let inserted = self.timeline.duplicate_selected(&self.selection);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.status = format!("Duplicated {} frame(s).", inserted.len());
                self.refresh_frame_jobs(inserted, &sender);
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::CopySelection => {
                self.clipboard = self
                    .timeline
                    .frames()
                    .iter()
                    .filter(|frame| self.selection.contains(&frame.id))
                    .cloned()
                    .collect();
                self.status = format!("Copied {} frame(s).", self.clipboard.len());
            }
            AppMsg::PasteClipboard => {
                self.stop_playback(None);
                let inserted = self
                    .timeline
                    .paste_after_selection(&self.selection, &self.clipboard);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.status = format!("Pasted {} frame(s).", inserted.len());
                self.refresh_frame_jobs(inserted, &sender);
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::RemoveSelection => {
                self.stop_playback(None);
                let removed = self.selection.len();
                self.timeline.remove_selected(&self.selection);
                self.selection.clear();
                self.selection_anchor_id = None;
                self.preview_path = None;
                self.preview_frame_id = None;
                self.status = format!("Removed {removed} frame(s).");
            }
            AppMsg::AppendDuplicateLoop => {
                self.stop_playback(None);
                let inserted = self.timeline.append_duplicate_loop(&self.selection);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.status = format!("Appended duplicate loop with {} frame(s).", inserted.len());
                self.refresh_frame_jobs(inserted, &sender);
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::AppendReverseLoop(repeat_edges) => {
                self.stop_playback(None);
                let inserted = self
                    .timeline
                    .append_reverse_loop(&self.selection, repeat_edges);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.status = format!("Appended reverse loop with {} frame(s).", inserted.len());
                self.refresh_frame_jobs(inserted, &sender);
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::RotateSelection(delta) => {
                self.apply_to_selection(|frame| {
                    frame.transform_spec.rotate_quarter_turns += delta;
                });
                self.status = "Updated rotation for selected frames.".to_string();
                self.refresh_selection_jobs(&sender);
            }
            AppMsg::ApplyInspectorTransform(values) => {
                self.apply_to_selection(|frame| {
                    frame.transform_spec.flip_horizontal = values.flip_horizontal;
                    frame.transform_spec.flip_vertical = values.flip_vertical;
                    frame.transform_spec.crop = values.crop;
                    frame.transform_spec.resize = values.resize;
                    frame.transform_spec.fit_mode = values.fit_mode;
                });
                self.status = "Applied edit values to selected frames.".to_string();
                self.refresh_selection_jobs(&sender);
            }
            AppMsg::SetExportPreset(preset) => self.export_profile.apply_preset(preset),
            AppMsg::SetOutputPath(path) => {
                self.last_output_path =
                    (!path.trim().is_empty()).then_some(PathBuf::from(path.trim()));
            }
            AppMsg::SetOutputWidth(width) => {
                self.export_profile.output_width = if width == 0 { None } else { Some(width) };
            }
            AppMsg::SetOutputHeight(height) => {
                self.export_profile.output_height = if height == 0 { None } else { Some(height) };
            }
            AppMsg::SetQuality(quality) => self.export_profile.quality = quality.clamp(0.0, 100.0),
            AppMsg::SetLossless(lossless) => self.export_profile.lossless = lossless,
            AppMsg::SetEncoderPreset(preset) => self.export_profile.encoder_preset = preset,
            AppMsg::SetCrThreshold(value) => self.export_profile.cr_threshold = value,
            AppMsg::SetCrSize(value) => self.export_profile.cr_size = value,
            AppMsg::SetLoopCount(value) => self.export_profile.loop_count = value,
            AppMsg::SetOverwrite(value) => self.export_profile.overwrite = value,
            AppMsg::SetExportFitMode(value) => self.export_profile.fit_mode = value,
            AppMsg::SetRawArgs(args) => self.export_profile.raw_args = args,
            AppMsg::SaveProject(path) => {
                let document = ProjectDocument {
                    frames: self.timeline.frames().to_vec(),
                    export_profile: self.export_profile.clone(),
                    last_output_path: self.last_output_path.clone(),
                };
                match save_project(&path, &document) {
                    Ok(_) => self.status = format!("Saved project to {}", path.display()),
                    Err(err) => self.status = format!("Failed to save project: {err}"),
                }
            }
            AppMsg::OpenProject(path) => {
                self.stop_playback(None);
                match load_project(&path) {
                    Ok(document) => {
                        let ids: Vec<_> = document.frames.iter().map(|frame| frame.id).collect();
                        self.timeline = Timeline::from_frames(document.frames);
                        self.selection = ids.into_iter().collect();
                        self.selection_anchor_id =
                            self.timeline.frames().first().map(|frame| frame.id);
                        self.export_profile = document.export_profile;
                        self.last_output_path = document.last_output_path;
                        self.preview_path = None;
                        self.preview_frame_id = None;
                        self.status = format!(
                            "Loaded project {}. Refreshing thumbnails...",
                            path.display()
                        );
                        let frame_ids = self
                            .timeline
                            .frames()
                            .iter()
                            .map(|frame| frame.id)
                            .collect();
                        self.refresh_frame_jobs(frame_ids, &sender);
                        self.queue_preview_for_primary_selection(&sender);
                    }
                    Err(err) => self.status = format!("Failed to load project: {err}"),
                }
            }
            AppMsg::ChooseOutputPath(path) => {
                self.last_output_path = Some(path);
            }
            AppMsg::ExportNow => {
                let Some(output_path) = self.last_output_path.clone() else {
                    self.status = "Choose an output path first.".to_string();
                    self.recompute_command_preview();
                    return;
                };
                if self.export_in_progress {
                    self.status = "Export already running.".to_string();
                    self.recompute_command_preview();
                    return;
                }
                self.export_in_progress = true;
                self.status = format!("Exporting to {} ...", output_path.display());
                let frames = self.timeline.frames().to_vec();
                let profile = self.export_profile.clone();
                sender.spawn_oneshot_command(move || CommandMsg::ExportFinished {
                    result: export_animation(&frames, &profile, &output_path)
                        .map_err(|err| err.to_string()),
                });
            }
        }

        self.recompute_command_preview();
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            CommandMsg::ThumbnailReady {
                frame_id,
                thumbnail_path,
                dimensions,
                error,
            } => {
                if self.thumbnails_pending > 0 {
                    self.thumbnails_pending -= 1;
                }
                if let Some(frame) = self.frame_mut(frame_id) {
                    if let Some(path) = thumbnail_path {
                        frame.thumbnail_path = Some(path);
                    }
                    if let Some(dimensions) = dimensions {
                        frame.source_dimensions = Some(dimensions);
                    }
                }
                if let Some(error) = error {
                    self.status = format!("Thumbnail failed for frame {frame_id}: {error}");
                } else if self.thumbnails_pending == 0 {
                    self.status = "Timeline thumbnails ready.".to_string();
                }
            }
            CommandMsg::PreviewReady {
                frame_id,
                render_size,
                preview_path,
                error,
            } => {
                if preview_result_is_usable(
                    self.primary_selected_id(),
                    self.preview_target_size,
                    frame_id,
                    render_size,
                ) {
                    self.preview_frame_id = Some(frame_id);
                    self.preview_rendered_size = Some(render_size);
                    self.preview_path = preview_path;
                }
                if let Some(error) = error {
                    self.status = format!("Preview failed for frame {frame_id}: {error}");
                }
            }
            CommandMsg::ExportFinished { result } => {
                self.export_in_progress = false;
                match result {
                    Ok(job) => self.status = format!("Exported {}", job.output_path.display()),
                    Err(err) => self.status = format!("Export failed: {err}"),
                }
            }
        }

        self.recompute_command_preview();
        let _ = sender;
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        widgets
            .diagnostics_label
            .set_label(&self.diagnostics.summary());
        let mut selection_summary = format!(
            "{} selected / {} total",
            self.selection.len(),
            self.timeline.frames().len()
        );
        if self.playback_active {
            selection_summary.push_str(" | playing");
        }
        if self.thumbnails_pending > 0 {
            selection_summary.push_str(&format!(
                " | {} thumbnail job(s) running",
                self.thumbnails_pending
            ));
        }
        if self.export_in_progress {
            selection_summary.push_str(" | export running");
        }
        widgets.selection_label.set_label(&selection_summary);
        widgets.status_label.set_label(&self.status);
        widgets
            .command_preview_label
            .set_label(&self.command_preview);
        set_button_icon(
            &widgets.nav_play_button,
            if self.playback_active {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            },
        );

        let frame_ids = self.timeline_frame_ids();
        let has_frames = !frame_ids.is_empty();
        let current_index = self.current_frame_index();
        let last_index = frame_ids.len().checked_sub(1);
        widgets
            .nav_first_button
            .set_sensitive(has_frames && current_index != Some(0));
        widgets
            .nav_prev_button
            .set_sensitive(has_frames && current_index != Some(0));
        widgets.nav_play_button.set_sensitive(has_frames);
        widgets
            .nav_next_button
            .set_sensitive(has_frames && current_index != last_index);
        widgets
            .nav_last_button
            .set_sensitive(has_frames && current_index != last_index);

        if let Some(path) = self.preview_path.as_ref() {
            widgets
                .preview_picture
                .set_file(Some(&gio::File::for_path(path)));
        } else {
            widgets.preview_picture.set_file(None::<&gio::File>);
        }
        widgets.preview_meta.set_label(&self.preview_meta_text());

        let output_text = self
            .last_output_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if widgets.output_entry.text().as_str() != output_text {
            widgets.output_entry.set_text(&output_text);
        }
        sync_combo_active_export_preset(&widgets.preset_combo, self.export_profile.preset);
        sync_combo_active_encoder_preset(
            &widgets.encoder_combo,
            self.export_profile.encoder_preset,
        );
        sync_combo_active_fit_mode(&widgets.fit_mode_combo, self.export_profile.fit_mode);
        set_spin_if_needed(&widgets.quality_spin, self.export_profile.quality as f64);
        set_spin_if_needed(
            &widgets.width_spin,
            self.export_profile.output_width.unwrap_or_default() as f64,
        );
        set_spin_if_needed(
            &widgets.height_spin,
            self.export_profile.output_height.unwrap_or_default() as f64,
        );
        set_check_if_needed(&widgets.lossless_check, self.export_profile.lossless);
        set_spin_if_needed(
            &widgets.cr_threshold_spin,
            self.export_profile.cr_threshold as f64,
        );
        set_spin_if_needed(&widgets.cr_size_spin, self.export_profile.cr_size as f64);
        set_spin_if_needed(&widgets.loop_spin, self.export_profile.loop_count as f64);
        set_check_if_needed(&widgets.overwrite_check, self.export_profile.overwrite);
        if widgets.raw_args_entry.text().as_str() != self.export_profile.raw_args {
            widgets
                .raw_args_entry
                .set_text(&self.export_profile.raw_args);
        }

        self.sync_inspector_widgets(widgets);

        while let Some(child) = widgets.timeline_strip.first_child() {
            widgets.timeline_strip.remove(&child);
        }

        for (index, frame) in self.timeline.frames().iter().enumerate() {
            widgets.timeline_strip.append(&build_timeline_tile(
                frame,
                index,
                self.selection.contains(&frame.id),
                sender.clone(),
            ));
        }
    }
}

impl AppModel {
    fn import_paths(
        &mut self,
        paths: Vec<PathBuf>,
        mode: ImportMode,
        sender: &ComponentSender<Self>,
    ) {
        self.stop_playback(None);
        let imported_ids = match mode {
            ImportMode::Append => self.timeline.import_paths(paths),
            ImportMode::Prepend => self.timeline.prepend_paths(paths),
            ImportMode::Replace => self.timeline.replace_paths(paths),
        };
        self.selection = imported_ids.iter().copied().collect();
        self.selection_anchor_id = imported_ids.first().copied();
        self.status = format!(
            "Imported {} frame(s). Generating thumbnails...",
            imported_ids.len()
        );
        self.refresh_frame_jobs(imported_ids, sender);
        self.queue_preview_for_primary_selection(sender);
    }

    fn frame_mut(&mut self, id: u64) -> Option<&mut FrameItem> {
        self.timeline
            .frames_mut()
            .iter_mut()
            .find(|frame| frame.id == id)
    }

    fn primary_selected_id(&self) -> Option<u64> {
        self.timeline
            .frames()
            .iter()
            .find(|frame| self.selection.contains(&frame.id))
            .map(|frame| frame.id)
    }

    fn primary_selected_frame(&self) -> Option<&FrameItem> {
        let id = self.primary_selected_id()?;
        self.timeline.frames().iter().find(|frame| frame.id == id)
    }

    fn timeline_frame_ids(&self) -> Vec<u64> {
        self.timeline
            .frames()
            .iter()
            .map(|frame| frame.id)
            .collect()
    }

    fn current_frame_index(&self) -> Option<usize> {
        let current = self.primary_selected_id()?;
        self.timeline
            .frames()
            .iter()
            .position(|frame| frame.id == current)
    }

    fn select_single_frame(&mut self, frame_id: u64, sender: &ComponentSender<Self>) {
        if self.selection.len() == 1
            && self.selection.contains(&frame_id)
            && self.selection_anchor_id == Some(frame_id)
        {
            return;
        }
        self.selection.clear();
        self.selection.insert(frame_id);
        self.selection_anchor_id = Some(frame_id);
        self.queue_preview_for_primary_selection(sender);
    }

    fn navigate_to_boundary(&mut self, end: bool, sender: &ComponentSender<Self>) {
        self.stop_playback(None);
        let frame_ids = self.timeline_frame_ids();
        let target = if end {
            frame_ids.last().copied()
        } else {
            frame_ids.first().copied()
        };
        let Some(frame_id) = target else {
            self.status = "No frames in timeline.".to_string();
            return;
        };
        self.select_single_frame(frame_id, sender);
        self.status = if end {
            "Moved to last frame.".to_string()
        } else {
            "Moved to first frame.".to_string()
        };
    }

    fn navigate_by_step(&mut self, offset: isize, sender: &ComponentSender<Self>) {
        self.stop_playback(None);
        let frame_ids = self.timeline_frame_ids();
        let Some(frame_id) = step_frame_id(&frame_ids, self.primary_selected_id(), offset) else {
            self.status = "No frames in timeline.".to_string();
            return;
        };
        self.select_single_frame(frame_id, sender);
        self.status = if offset < 0 {
            "Moved back one frame.".to_string()
        } else {
            "Moved forward one frame.".to_string()
        };
    }

    fn toggle_playback(&mut self, sender: &ComponentSender<Self>) {
        if self.playback_active {
            self.stop_playback(Some("Playback paused."));
            return;
        }

        let frame_ids = self.timeline_frame_ids();
        let Some(frame_id) = playback_start_frame_id(&frame_ids, self.primary_selected_id()) else {
            self.status = "No frames in timeline.".to_string();
            return;
        };

        self.playback_generation = self.playback_generation.wrapping_add(1);
        self.playback_active = true;
        self.select_single_frame(frame_id, sender);
        self.status = "Playback started.".to_string();
        self.schedule_playback_advance(self.playback_generation, sender);
    }

    fn schedule_playback_advance(&self, generation: u64, sender: &ComponentSender<Self>) {
        if !self.playback_active {
            return;
        }
        let delay_ms = self
            .primary_selected_frame()
            .map(|frame| u64::from(frame.duration_ms.max(10)))
            .unwrap_or(100);
        let sender = sender.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
            sender.input(AppMsg::PlaybackAdvance { generation });
        });
    }

    fn following_frame_id(&self) -> Option<u64> {
        let frame_ids = self.timeline_frame_ids();
        following_frame_id(&frame_ids, self.primary_selected_id())
    }

    fn stop_playback(&mut self, status: Option<&str>) {
        if self.playback_active {
            self.playback_active = false;
            self.playback_generation = self.playback_generation.wrapping_add(1);
        }
        if let Some(status) = status {
            self.status = status.to_string();
        }
    }

    fn refresh_frame_jobs(&mut self, frame_ids: Vec<u64>, sender: &ComponentSender<Self>) {
        if frame_ids.is_empty() {
            return;
        }
        self.thumbnails_pending += frame_ids.len();
        for frame_id in frame_ids {
            let Some(frame) = self
                .timeline
                .frames()
                .iter()
                .find(|frame| frame.id == frame_id)
                .cloned()
            else {
                self.thumbnails_pending = self.thumbnails_pending.saturating_sub(1);
                continue;
            };
            let cache_dir = self.cache_dir.clone();
            sender.spawn_oneshot_command(move || {
                let mut frame = frame;
                populate_frame_metadata(&mut frame);
                let dimensions = frame.source_dimensions;
                let result = refresh_thumbnail(&mut frame, &cache_dir);
                CommandMsg::ThumbnailReady {
                    frame_id,
                    thumbnail_path: frame.thumbnail_path.clone(),
                    dimensions,
                    error: result.err().map(|err| err.to_string()),
                }
            });
        }
    }

    fn refresh_selection_jobs(&mut self, sender: &ComponentSender<Self>) {
        let ids: Vec<_> = self.selection.iter().copied().collect();
        self.refresh_frame_jobs(ids, sender);
        self.queue_preview_for_primary_selection(sender);
    }

    fn queue_preview_for_primary_selection(&mut self, sender: &ComponentSender<Self>) {
        let Some(frame) = self.primary_selected_frame().cloned() else {
            self.preview_frame_id = None;
            self.preview_path = None;
            self.preview_rendered_size = None;
            return;
        };
        let same_frame = self.preview_frame_id == Some(frame.id);
        let render_size = self.preview_target_size;
        let cached_preview_path = preview_cache_path(&frame, &self.cache_dir, render_size);
        let cached_preview_path = cached_preview_path.is_file().then_some(cached_preview_path);
        self.preview_frame_id = Some(frame.id);

        if let Some(cached_preview_path) = cached_preview_path {
            self.preview_path = Some(cached_preview_path);
            self.preview_rendered_size = Some(render_size);
        } else {
            let fallback_path = immediate_preview_path(
                &frame,
                None,
                self.preview_path.as_ref(),
                self.playback_active,
            );
            if !same_frame || self.preview_path.as_ref() != Some(&fallback_path) {
                self.preview_path = Some(fallback_path);
            }
            self.preview_rendered_size = None;

            let frame_id = frame.id;
            let cache_dir = self.cache_dir.clone();
            sender.spawn_oneshot_command(move || {
                let result = render_preview(&frame, &cache_dir, render_size);
                CommandMsg::PreviewReady {
                    frame_id,
                    render_size,
                    preview_path: result.as_ref().ok().cloned(),
                    error: result.err().map(|err| err.to_string()),
                }
            });
        }

        self.prewarm_upcoming_playback_previews();
    }

    fn prewarm_upcoming_playback_previews(&self) {
        if !self.playback_active {
            return;
        }

        let Some(current_index) = self.current_frame_index() else {
            return;
        };

        let render_size = self.preview_target_size;
        for frame in self
            .timeline
            .frames()
            .iter()
            .skip(current_index + 1)
            .take(2)
            .cloned()
        {
            let cache_dir = self.cache_dir.clone();
            if preview_cache_path(&frame, &cache_dir, render_size).is_file() {
                continue;
            }

            std::thread::spawn(move || {
                let _ = render_preview(&frame, &cache_dir, render_size);
            });
        }
    }

    fn apply_to_selection(&mut self, mut apply: impl FnMut(&mut FrameItem)) {
        for frame in self.timeline.frames_mut() {
            if self.selection.contains(&frame.id) {
                apply(frame);
            }
        }
    }

    fn recompute_command_preview(&mut self) {
        let output_path = self
            .last_output_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("output.webp"));
        self.command_preview = build_command_preview(
            Path::new("/tmp/awebpinator-preview.ffconcat"),
            &output_path,
            &self.export_profile,
        );
    }

    fn preview_meta_text(&self) -> String {
        let Some(frame) = self.primary_selected_frame() else {
            return "Select a frame to inspect it.".to_string();
        };
        let dims = frame
            .source_dimensions
            .map(|(w, h)| format!("{w}x{h}"))
            .unwrap_or_else(|| "unknown".to_string());
        let crop = frame
            .transform_spec
            .crop
            .map(|crop| format!("crop {}x{}+{},{}", crop.width, crop.height, crop.x, crop.y))
            .unwrap_or_else(|| "no crop".to_string());
        let resize = frame
            .transform_spec
            .resize
            .map(|resize| format!("resize {}x{}", resize.width, resize.height))
            .unwrap_or_else(|| "no resize".to_string());
        format!(
            "{}\n{} | {} ms | rotate {} quarter-turns\n{} | {} | fit {} | flip h:{} v:{}",
            frame.file_name(),
            dims,
            frame.duration_ms,
            frame.transform_spec.rotate_quarter_turns.rem_euclid(4),
            crop,
            resize,
            frame.transform_spec.fit_mode,
            frame.transform_spec.flip_horizontal,
            frame.transform_spec.flip_vertical
        )
    }

    fn sync_inspector_widgets(&self, widgets: &mut AppWidgets) {
        let Some(frame) = self.primary_selected_frame() else {
            set_check_if_needed(&widgets.flip_h_check, false);
            set_check_if_needed(&widgets.flip_v_check, false);
            set_spin_if_needed(&widgets.crop_x, 0.0);
            set_spin_if_needed(&widgets.crop_y, 0.0);
            set_spin_if_needed(&widgets.crop_w, 0.0);
            set_spin_if_needed(&widgets.crop_h, 0.0);
            set_spin_if_needed(&widgets.resize_w, 0.0);
            set_spin_if_needed(&widgets.resize_h, 0.0);
            sync_combo_active_fit_mode(&widgets.inspector_fit_combo, FitMode::Contain);
            return;
        };
        set_check_if_needed(&widgets.flip_h_check, frame.transform_spec.flip_horizontal);
        set_check_if_needed(&widgets.flip_v_check, frame.transform_spec.flip_vertical);
        let crop = frame.transform_spec.crop.unwrap_or(CropRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        set_spin_if_needed(&widgets.crop_x, crop.x as f64);
        set_spin_if_needed(&widgets.crop_y, crop.y as f64);
        set_spin_if_needed(&widgets.crop_w, crop.width as f64);
        set_spin_if_needed(&widgets.crop_h, crop.height as f64);
        let resize = frame.transform_spec.resize.unwrap_or(ResizeTarget {
            width: 0,
            height: 0,
        });
        set_spin_if_needed(&widgets.resize_w, resize.width as f64);
        set_spin_if_needed(&widgets.resize_h, resize.height as f64);
        sync_combo_active_fit_mode(&widgets.inspector_fit_combo, frame.transform_spec.fit_mode);
    }
}

fn build_timeline_tile(
    frame: &FrameItem,
    index: usize,
    selected: bool,
    sender: ComponentSender<AppModel>,
) -> gtk::Box {
    let frame_id = frame.id;
    let tile = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .width_request(132)
        .build();
    tile.add_css_class("timeline-tile");
    if selected {
        tile.add_css_class("timeline-tile-selected");
    }

    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    drag_source.set_content(Some(&gdk::ContentProvider::for_value(
        &frame_id.to_string().to_value(),
    )));
    tile.add_controller(drag_source);

    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.connect_released(clone!(
        #[strong]
        sender,
        move |gesture, _, _, _| {
            let state = gesture.current_event_state();
            let mode = match (
                state.contains(gdk::ModifierType::SHIFT_MASK),
                state.contains(gdk::ModifierType::CONTROL_MASK),
            ) {
                (true, true) => SelectionMode::CtrlShift,
                (true, false) => SelectionMode::Shift,
                (false, true) => SelectionMode::Ctrl,
                (false, false) => SelectionMode::Plain,
            };
            sender.input(AppMsg::SelectFrame { id: frame_id, mode });
        }
    ));
    tile.add_controller(click);

    let picture = if let Some(path) = frame.thumbnail_path.as_ref() {
        gtk::Picture::for_filename(path)
    } else {
        gtk::Picture::new()
    };
    picture.set_size_request(120, 120);
    picture.set_can_shrink(true);
    tile.append(&picture);

    let title = gtk::Label::new(Some(&format!("Frame {:03}", index + 1)));
    title.set_xalign(0.0);
    tile.append(&title);

    let subtitle = gtk::Label::new(Some(&frame.file_name()));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.set_max_width_chars(14);
    tile.append(&subtitle);

    let drop_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    let tile_for_drop = tile.clone();
    drop_target.connect_enter(clone!(move |_, x, _| {
        set_tile_drop_class(&tile_for_drop, tile_drop_side(tile_for_drop.width(), x));
        gdk::DragAction::MOVE
    }));
    let tile_for_motion = tile.clone();
    drop_target.connect_motion(clone!(move |_, x, _| {
        set_tile_drop_class(&tile_for_motion, tile_drop_side(tile_for_motion.width(), x));
        gdk::DragAction::MOVE
    }));
    let tile_for_leave = tile.clone();
    drop_target.connect_leave(clone!(move |_| {
        clear_tile_drop_class(&tile_for_leave);
    }));
    let tile_for_commit = tile.clone();
    drop_target.connect_drop(clone!(
        #[strong]
        sender,
        move |_, value, x, _| {
            let Ok(text) = value.get::<String>() else {
                return false;
            };
            let Ok(dragged_id) = text.parse::<u64>() else {
                return false;
            };
            clear_tile_drop_class(&tile_for_commit);
            sender.input(AppMsg::DropFrameAt {
                dragged_id,
                target_index: tile_drop_index(index, tile_for_commit.width(), x),
            });
            true
        }
    ));
    tile.add_controller(drop_target);

    tile
}

fn combo_for_fit_mode() -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for mode in FitMode::ALL {
        combo.append_text(mode.as_str());
    }
    combo.set_active(Some(0));
    combo
}

fn combo_for_export_preset() -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for preset in ExportPreset::ALL {
        combo.append_text(preset.as_str());
    }
    combo.set_active(Some(1));
    combo
}

fn combo_for_encoder_preset() -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for preset in EncoderPreset::ALL {
        combo.append_text(preset.as_str());
    }
    combo.set_active(Some(0));
    combo
}

fn fit_mode_from_combo(combo: &gtk::ComboBoxText) -> FitMode {
    match combo.active_text().as_deref() {
        Some("Cover") => FitMode::Cover,
        Some("Stretch") => FitMode::Stretch,
        _ => FitMode::Contain,
    }
}

fn export_preset_from_combo(combo: &gtk::ComboBoxText) -> ExportPreset {
    match combo.active_text().as_deref() {
        Some("Fast Preview") => ExportPreset::FastPreview,
        Some("High Quality") => ExportPreset::HighQuality,
        Some("Lossless") => ExportPreset::Lossless,
        _ => ExportPreset::Balanced,
    }
}

fn encoder_from_combo(combo: &gtk::ComboBoxText) -> EncoderPreset {
    match combo.active_text().as_deref() {
        Some("Picture") => EncoderPreset::Picture,
        Some("Photo") => EncoderPreset::Photo,
        Some("Drawing") => EncoderPreset::Drawing,
        Some("Icon") => EncoderPreset::Icon,
        Some("Text") => EncoderPreset::Text,
        _ => EncoderPreset::Default,
    }
}

fn sync_combo_active_fit_mode(combo: &gtk::ComboBoxText, mode: FitMode) {
    let target = match mode {
        FitMode::Contain => 0,
        FitMode::Cover => 1,
        FitMode::Stretch => 2,
    };
    if combo.active() != Some(target) {
        combo.set_active(Some(target));
    }
}

fn sync_combo_active_export_preset(combo: &gtk::ComboBoxText, preset: ExportPreset) {
    let target = match preset {
        ExportPreset::FastPreview => 0,
        ExportPreset::Balanced => 1,
        ExportPreset::HighQuality => 2,
        ExportPreset::Lossless => 3,
    };
    if combo.active() != Some(target) {
        combo.set_active(Some(target));
    }
}

fn sync_combo_active_encoder_preset(combo: &gtk::ComboBoxText, preset: EncoderPreset) {
    let target = match preset {
        EncoderPreset::Default => 0,
        EncoderPreset::Picture => 1,
        EncoderPreset::Photo => 2,
        EncoderPreset::Drawing => 3,
        EncoderPreset::Icon => 4,
        EncoderPreset::Text => 5,
    };
    if combo.active() != Some(target) {
        combo.set_active(Some(target));
    }
}

fn section<W: IsA<gtk::Widget>>(title: &str, child: &W) -> gtk::Frame {
    gtk::Frame::builder().label(title).child(child).build()
}

fn build_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_button_icon(&button, icon_name);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn install_preview_layout_watch(preview_picture: &gtk::Picture, sender: ComponentSender<AppModel>) {
    preview_picture.connect_map(clone!(
        #[strong]
        sender,
        move |picture| sender.input(AppMsg::PreviewLayoutChanged(
            preview_render_size_for_widget(picture)
        ))
    ));
    preview_picture.connect_notify_local(
        Some("width"),
        clone!(
            #[strong]
            sender,
            move |picture, _| sender.input(AppMsg::PreviewLayoutChanged(
                preview_render_size_for_widget(picture)
            ))
        ),
    );
    preview_picture.connect_notify_local(
        Some("height"),
        clone!(
            #[strong]
            sender,
            move |picture, _| sender.input(AppMsg::PreviewLayoutChanged(
                preview_render_size_for_widget(picture)
            ))
        ),
    );
    preview_picture.connect_notify_local(
        Some("scale-factor"),
        clone!(
            #[strong]
            sender,
            move |picture, _| sender.input(AppMsg::PreviewLayoutChanged(
                preview_render_size_for_widget(picture)
            ))
        ),
    );
}

fn set_button_icon(button: &gtk::Button, icon_name: &str) {
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
}

fn preview_render_size_for_widget(widget: &impl IsA<gtk::Widget>) -> PreviewRenderSize {
    preview_render_size_from_values(
        widget.as_ref().width(),
        widget.as_ref().height(),
        widget.as_ref().width_request(),
        widget.as_ref().height_request(),
        widget.as_ref().scale_factor(),
    )
}

fn preview_render_size_from_values(
    width: i32,
    height: i32,
    width_request: i32,
    height_request: i32,
    scale_factor: i32,
) -> PreviewRenderSize {
    let logical_width = width.max(width_request.max(0));
    let logical_height = height.max(height_request.max(0));
    let logical_width = if logical_width > 0 {
        logical_width
    } else {
        DEFAULT_PREVIEW_LOGICAL_WIDTH
    };
    let logical_height = if logical_height > 0 {
        logical_height
    } else {
        DEFAULT_PREVIEW_LOGICAL_HEIGHT
    };
    let scale_factor = scale_factor.max(1) as u32;
    PreviewRenderSize {
        width: (logical_width as u32)
            .saturating_mul(scale_factor)
            .clamp(1, MAX_PREVIEW_RENDER_EDGE),
        height: (logical_height as u32)
            .saturating_mul(scale_factor)
            .clamp(1, MAX_PREVIEW_RENDER_EDGE),
    }
}

fn install_app_css(window: &gtk::Window) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .timeline-tile {
            border-radius: 10px;
            padding: 6px;
            border: 2px solid transparent;
            background: transparent;
        }
        .timeline-tile-selected {
            background: #0b63ce;
            border-color: #7fb2ff;
            color: white;
        }
        .timeline-tile-selected label {
            color: white;
        }
        .timeline-drop-before {
            border-left-color: #7fb2ff;
            box-shadow: inset 4px 0 0 #0b63ce;
        }
        .timeline-drop-after {
            border-right-color: #7fb2ff;
            box-shadow: inset -4px 0 0 #0b63ce;
        }
        ",
    );

    let display = gtk::prelude::WidgetExt::display(window);
    #[allow(deprecated)]
    gtk::StyleContext::add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn attach_labeled_spin(
    grid: &gtk::Grid,
    label: &str,
    spin: &gtk::SpinButton,
    column: i32,
    row: i32,
) {
    grid.attach(&gtk::Label::new(Some(label)), column, row, 1, 1);
    grid.attach(spin, column, row + 1, 1, 1);
}

fn crop_from_widgets(
    crop_x: &gtk::SpinButton,
    crop_y: &gtk::SpinButton,
    crop_w: &gtk::SpinButton,
    crop_h: &gtk::SpinButton,
) -> Option<CropRect> {
    match (crop_w.value() as u32, crop_h.value() as u32) {
        (0, 0) => None,
        (width, height) => Some(CropRect {
            x: crop_x.value() as u32,
            y: crop_y.value() as u32,
            width,
            height,
        }),
    }
}

fn resize_from_widgets(
    resize_w: &gtk::SpinButton,
    resize_h: &gtk::SpinButton,
) -> Option<ResizeTarget> {
    match (resize_w.value() as u32, resize_h.value() as u32) {
        (0, 0) => None,
        (width, height) if width > 0 && height > 0 => Some(ResizeTarget { width, height }),
        _ => None,
    }
}

fn set_spin_if_needed(spin: &gtk::SpinButton, value: f64) {
    if (spin.value() - value).abs() > f64::EPSILON {
        spin.set_value(value);
    }
}

fn set_check_if_needed(check: &gtk::CheckButton, value: bool) {
    if check.is_active() != value {
        check.set_active(value);
    }
}

#[derive(Clone, Copy)]
enum TileDropSide {
    Before,
    After,
}

fn tile_drop_side(tile_width: i32, x: f64) -> TileDropSide {
    let split = if tile_width > 0 {
        f64::from(tile_width) / 2.0
    } else {
        66.0
    };
    if x < split {
        TileDropSide::Before
    } else {
        TileDropSide::After
    }
}

fn tile_drop_index(index: usize, tile_width: i32, x: f64) -> usize {
    index
        + match tile_drop_side(tile_width, x) {
            TileDropSide::Before => 0,
            TileDropSide::After => 1,
        }
}

fn set_tile_drop_class(tile: &gtk::Box, side: TileDropSide) {
    clear_tile_drop_class(tile);
    match side {
        TileDropSide::Before => tile.add_css_class("timeline-drop-before"),
        TileDropSide::After => tile.add_css_class("timeline-drop-after"),
    }
}

fn clear_tile_drop_class(tile: &gtk::Box) {
    tile.remove_css_class("timeline-drop-before");
    tile.remove_css_class("timeline-drop-after");
}

fn should_handle_timeline_shortcuts(window: &gtk::Window) -> bool {
    let Some(focus) = gtk::prelude::RootExt::focus(window) else {
        return true;
    };

    !(focus.is::<gtk::Entry>()
        || focus.is::<gtk::SpinButton>()
        || focus.is::<gtk::TextView>()
        || focus.is::<gtk::EditableLabel>())
}

fn open_image_dialog(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let dialog = gtk::FileChooserNative::builder()
        .title("Import Images")
        .transient_for(window)
        .accept_label("Import")
        .cancel_label("Cancel")
        .action(gtk::FileChooserAction::Open)
        .select_multiple(true)
        .build();
    add_image_filter(&dialog);
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let files = dialog.files();
            let mut paths = Vec::new();
            for index in 0..files.n_items() {
                if let Some(item) = files.item(index) {
                    if let Ok(file) = item.downcast::<gio::File>() {
                        if let Some(path) = file.path() {
                            paths.push(path);
                        }
                    }
                }
            }
            sender.input(AppMsg::ImportPaths(paths));
        }
        dialog.destroy();
    });
    dialog.show();
}

fn open_project_dialog(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let dialog = gtk::FileChooserNative::builder()
        .title("Open Project")
        .transient_for(window)
        .accept_label("Open")
        .cancel_label("Cancel")
        .action(gtk::FileChooserAction::Open)
        .build();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(file) = dialog.file().and_then(|file| file.path()) {
                sender.input(AppMsg::OpenProject(file));
            }
        }
        dialog.destroy();
    });
    dialog.show();
}

fn save_project_dialog(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let dialog = gtk::FileChooserNative::builder()
        .title("Save Project")
        .transient_for(window)
        .accept_label("Save")
        .cancel_label("Cancel")
        .action(gtk::FileChooserAction::Save)
        .build();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(file) = dialog.file().and_then(|file| file.path()) {
                sender.input(AppMsg::SaveProject(file));
            }
        }
        dialog.destroy();
    });
    dialog.show();
}

fn choose_export_dialog(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let dialog = gtk::FileChooserNative::builder()
        .title("Choose Export Output")
        .transient_for(window)
        .accept_label("Use Path")
        .cancel_label("Cancel")
        .action(gtk::FileChooserAction::Save)
        .build();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(file) = dialog.file().and_then(|file| file.path()) {
                sender.input(AppMsg::ChooseOutputPath(file));
            }
        }
        dialog.destroy();
    });
    dialog.show();
}

fn add_image_filter(dialog: &gtk::FileChooserNative) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for mime in [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/tiff",
    ] {
        filter.add_mime_type(mime);
    }
    dialog.add_filter(&filter);
}

fn install_import_drop_targets(widget: &impl IsA<gtk::Widget>, sender: ComponentSender<AppModel>) {
    let file_list_target =
        gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    file_list_target.connect_drop(clone!(
        #[strong]
        sender,
        move |_, value, _, _| {
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let paths: Vec<_> = files
                .files()
                .into_iter()
                .filter_map(|file| file.path())
                .collect();
            if paths.is_empty() {
                return false;
            }
            sender.input(AppMsg::ImportPaths(paths));
            true
        }
    ));
    widget.as_ref().add_controller(file_list_target);

    let file_target = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    file_target.connect_drop(clone!(
        #[strong]
        sender,
        move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            sender.input(AppMsg::ImportPaths(vec![path]));
            true
        }
    ));
    widget.as_ref().add_controller(file_target);

    let text_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::COPY);
    text_target.connect_drop(clone!(
        #[strong]
        sender,
        move |_, value, _, _| {
            let Ok(text) = value.get::<String>() else {
                return false;
            };
            let paths = parse_uri_list(&text);
            if paths.is_empty() {
                return false;
            }
            sender.input(AppMsg::ImportPaths(paths));
            true
        }
    ));
    widget.as_ref().add_controller(text_target);
}

fn choose_import_mode(
    window: &gtk::Window,
    sender: ComponentSender<AppModel>,
    paths: Vec<PathBuf>,
) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk::MessageType::Question)
        .text("Import into current timeline?")
        .secondary_text("Frames are already loaded. Choose whether to append, prepend, or replace them with the new images.")
        .build();
    dialog.add_buttons(&[
        ("Append", gtk::ResponseType::Other(0)),
        ("Prepend", gtk::ResponseType::Other(1)),
        ("Replace", gtk::ResponseType::Other(2)),
        ("Cancel", gtk::ResponseType::Cancel),
    ]);
    dialog.connect_response(move |dialog, response| {
        let mode = match response {
            gtk::ResponseType::Other(0) => Some(ImportMode::Append),
            gtk::ResponseType::Other(1) => Some(ImportMode::Prepend),
            gtk::ResponseType::Other(2) => Some(ImportMode::Replace),
            _ => None,
        };
        dialog.close();
        if let Some(mode) = mode {
            sender.input(AppMsg::ImportPathsWithMode {
                paths: paths.clone(),
                mode,
            });
        }
    });
    dialog.present();
}

fn filter_image_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"
                    )
                })
                .unwrap_or(false)
        })
        .collect()
}

fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            gio::File::for_uri(line)
                .path()
                .or_else(|| Path::new(line).is_absolute().then(|| PathBuf::from(line)))
        })
        .collect()
}

fn immediate_preview_path(
    frame: &FrameItem,
    cached_preview_path: Option<PathBuf>,
    current_preview_path: Option<&PathBuf>,
    playback_active: bool,
) -> PathBuf {
    if let Some(cached_preview_path) = cached_preview_path {
        return cached_preview_path;
    }

    if playback_active {
        if frame.transform_spec != TransformSpec::default() {
            if let Some(current_preview_path) = current_preview_path {
                return current_preview_path.clone();
            }
        }
        return frame.source_path.clone();
    }

    frame
        .thumbnail_path
        .clone()
        .unwrap_or_else(|| frame.source_path.clone())
}

fn should_refresh_preview(
    rendered_size: Option<PreviewRenderSize>,
    target_size: PreviewRenderSize,
) -> bool {
    rendered_size.is_none_or(|rendered_size| !rendered_size.covers(target_size))
}

fn preview_result_is_usable(
    current_frame_id: Option<u64>,
    target_size: PreviewRenderSize,
    frame_id: u64,
    render_size: PreviewRenderSize,
) -> bool {
    current_frame_id == Some(frame_id) && render_size.covers(target_size)
}

fn step_frame_id(frame_ids: &[u64], current: Option<u64>, offset: isize) -> Option<u64> {
    if frame_ids.is_empty() {
        return None;
    }

    let current_index = current
        .and_then(|frame_id| {
            frame_ids
                .iter()
                .position(|candidate| *candidate == frame_id)
        })
        .unwrap_or(0);

    let target_index = if offset < 0 {
        current_index.saturating_sub(offset.unsigned_abs())
    } else {
        current_index
            .saturating_add(offset as usize)
            .min(frame_ids.len().saturating_sub(1))
    };

    frame_ids.get(target_index).copied()
}

fn playback_start_frame_id(frame_ids: &[u64], current: Option<u64>) -> Option<u64> {
    if frame_ids.is_empty() {
        return None;
    }

    match current.and_then(|frame_id| {
        frame_ids
            .iter()
            .position(|candidate| *candidate == frame_id)
    }) {
        Some(index) if index + 1 < frame_ids.len() => frame_ids.get(index).copied(),
        _ => frame_ids.first().copied(),
    }
}

fn following_frame_id(frame_ids: &[u64], current: Option<u64>) -> Option<u64> {
    let current_index = current.and_then(|frame_id| {
        frame_ids
            .iter()
            .position(|candidate| *candidate == frame_id)
    })?;
    frame_ids.get(current_index + 1).copied()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        PreviewRenderSize, following_frame_id, immediate_preview_path, playback_start_frame_id,
        preview_render_size_from_values, preview_result_is_usable, should_refresh_preview,
        step_frame_id,
    };
    use crate::types::{FitMode, FrameItem, TransformSpec};

    #[test]
    fn step_frame_navigation_clamps_to_timeline_bounds() {
        let frame_ids = vec![10, 20, 30];

        assert_eq!(step_frame_id(&frame_ids, Some(10), -1), Some(10));
        assert_eq!(step_frame_id(&frame_ids, Some(20), -1), Some(10));
        assert_eq!(step_frame_id(&frame_ids, Some(20), 1), Some(30));
        assert_eq!(step_frame_id(&frame_ids, Some(30), 1), Some(30));
    }

    #[test]
    fn playback_starts_from_first_frame_when_at_end_or_unset() {
        let frame_ids = vec![10, 20, 30];

        assert_eq!(playback_start_frame_id(&frame_ids, None), Some(10));
        assert_eq!(playback_start_frame_id(&frame_ids, Some(20)), Some(20));
        assert_eq!(playback_start_frame_id(&frame_ids, Some(30)), Some(10));
    }

    #[test]
    fn following_frame_only_exists_before_end() {
        let frame_ids = vec![10, 20, 30];

        assert_eq!(following_frame_id(&frame_ids, Some(10)), Some(20));
        assert_eq!(following_frame_id(&frame_ids, Some(20)), Some(30));
        assert_eq!(following_frame_id(&frame_ids, Some(30)), None);
    }

    #[test]
    fn immediate_preview_prefers_thumbnail_when_not_playing() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec {
                rotate_quarter_turns: 0,
                flip_horizontal: false,
                flip_vertical: false,
                crop: None,
                resize: None,
                fit_mode: FitMode::Contain,
            },
            thumbnail_path: Some(PathBuf::from("thumb.png")),
            enabled: true,
            source_dimensions: None,
        };

        assert_eq!(
            immediate_preview_path(&frame, None, None, false),
            PathBuf::from("thumb.png")
        );
    }

    #[test]
    fn immediate_preview_prefers_cached_render_when_available() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec::default(),
            thumbnail_path: Some(PathBuf::from("thumb.png")),
            enabled: true,
            source_dimensions: None,
        };

        assert_eq!(
            immediate_preview_path(&frame, Some(PathBuf::from("preview.png")), None, true),
            PathBuf::from("preview.png")
        );
    }

    #[test]
    fn immediate_preview_uses_source_during_playback() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec::default(),
            thumbnail_path: Some(PathBuf::from("thumb.png")),
            enabled: true,
            source_dimensions: None,
        };

        assert_eq!(
            immediate_preview_path(&frame, None, None, true),
            PathBuf::from("source.png")
        );
    }

    #[test]
    fn immediate_preview_keeps_existing_preview_for_transformed_playback_frame() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec {
                rotate_quarter_turns: 1,
                flip_horizontal: false,
                flip_vertical: false,
                crop: None,
                resize: None,
                fit_mode: FitMode::Contain,
            },
            thumbnail_path: Some(PathBuf::from("thumb.png")),
            enabled: true,
            source_dimensions: None,
        };

        assert_eq!(
            immediate_preview_path(
                &frame,
                None,
                Some(&PathBuf::from("existing-preview.png")),
                true,
            ),
            PathBuf::from("existing-preview.png")
        );
    }

    #[test]
    fn preview_render_size_uses_allocated_size_and_scale_factor() {
        let render_size = preview_render_size_from_values(800, 450, 720, 360, 2);

        assert_eq!(
            render_size,
            PreviewRenderSize {
                width: 1600,
                height: 900
            }
        );
    }

    #[test]
    fn preview_render_size_falls_back_to_requested_size() {
        let render_size = preview_render_size_from_values(0, 0, 720, 360, 1);

        assert_eq!(
            render_size,
            PreviewRenderSize {
                width: 720,
                height: 360
            }
        );
    }

    #[test]
    fn should_refresh_preview_only_when_target_exceeds_rendered_size() {
        assert!(should_refresh_preview(
            None,
            PreviewRenderSize {
                width: 720,
                height: 360
            }
        ));
        assert!(!should_refresh_preview(
            Some(PreviewRenderSize {
                width: 1440,
                height: 720
            }),
            PreviewRenderSize {
                width: 720,
                height: 360
            },
        ));
        assert!(should_refresh_preview(
            Some(PreviewRenderSize {
                width: 720,
                height: 360
            }),
            PreviewRenderSize {
                width: 1080,
                height: 720
            },
        ));
    }

    #[test]
    fn preview_result_is_usable_only_for_current_frame_and_target() {
        assert!(preview_result_is_usable(
            Some(5),
            PreviewRenderSize {
                width: 720,
                height: 360
            },
            5,
            PreviewRenderSize {
                width: 1440,
                height: 720
            },
        ));
        assert!(!preview_result_is_usable(
            Some(5),
            PreviewRenderSize {
                width: 1080,
                height: 720
            },
            5,
            PreviewRenderSize {
                width: 720,
                height: 360
            },
        ));
        assert!(!preview_result_is_usable(
            Some(5),
            PreviewRenderSize {
                width: 720,
                height: 360
            },
            6,
            PreviewRenderSize {
                width: 1440,
                height: 720
            },
        ));
    }
}
