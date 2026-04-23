use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gtk::glib::clone;
use gtk::prelude::*;
use gtk::{gdk, gio};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};
use tracing::error;

use crate::export::{build_command_preview, export_animation};
use crate::project::{load_project, save_project};
use crate::runtime::{Diagnostics, collect_diagnostics};
use crate::thumbnail::{ensure_cache_dir, populate_frame_metadata, refresh_thumbnail};
use crate::timeline::Timeline;
use crate::types::{
    CropRect, EncoderPreset, ExportPreset, ExportProfile, FitMode, FrameItem, ProjectDocument,
    ResizeTarget,
};

pub fn run() {
    let app = RelmApp::new("dev.truevfx.awebpinator");
    app.run::<AppModel>(());
}

#[derive(Debug)]
pub enum AppMsg {
    ImportPaths(Vec<PathBuf>),
    ToggleSelected(u64, bool),
    ToggleEnabled(u64, bool),
    SetFrameDuration(u64, u32),
    ApplyBatchDuration(u32),
    MoveSelectionUp,
    MoveSelectionDown,
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
    SetStatus(String),
}

#[derive(Debug, Clone)]
pub struct InspectorValues {
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub crop: Option<CropRect>,
    pub resize: Option<ResizeTarget>,
    pub fit_mode: FitMode,
}

pub struct AppModel {
    timeline: Timeline,
    selection: BTreeSet<u64>,
    clipboard: Vec<FrameItem>,
    export_profile: ExportProfile,
    diagnostics: Diagnostics,
    status: String,
    cache_dir: PathBuf,
    last_output_path: Option<PathBuf>,
    command_preview: String,
}

pub struct AppWidgets {
    frame_list: gtk::ListBox,
    diagnostics_label: gtk::Label,
    selection_label: gtk::Label,
    status_label: gtk::Label,
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
}

impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Widgets = AppWidgets;
    type Root = gtk::Window;

    fn init_root() -> Self::Root {
        gtk::Window::builder()
            .title("AWEBPinator")
            .default_width(1440)
            .default_height(920)
            .build()
    }

    fn init(_init: Self::Init, window: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let cache_dir = ensure_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
        let diagnostics = collect_diagnostics();
        let model = AppModel {
            timeline: Timeline::new(),
            selection: BTreeSet::new(),
            clipboard: Vec::new(),
            export_profile: ExportProfile::default(),
            diagnostics,
            status: "Import images to begin building an animated WebP.".to_string(),
            cache_dir,
            last_output_path: None,
            command_preview: String::new(),
        };

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

        let frame_list = gtk::ListBox::new();
        frame_list.set_selection_mode(gtk::SelectionMode::None);
        frame_list.set_hexpand(true);
        frame_list.set_vexpand(true);
        let frame_scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&frame_list)
            .build();
        left_box.append(&frame_scroll);

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
            .width_request(430)
            .build();
        paned.set_end_child(Some(&right_box));

        let diagnostics_label = gtk::Label::new(None);
        diagnostics_label.set_xalign(0.0);
        diagnostics_label.set_selectable(true);
        diagnostics_label.set_wrap(true);
        right_box.append(&section("Diagnostics", &diagnostics_label));

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

        let status_label = gtk::Label::new(None);
        status_label.set_xalign(0.0);
        status_label.set_wrap(true);
        root.append(&status_label);

        import_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| {
                open_image_dialog(&window, sender.clone());
            }
        ));
        open_project_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| {
                open_project_dialog(&window, sender.clone());
            }
        ));
        save_project_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| {
                save_project_dialog(&window, sender.clone());
            }
        ));
        browse_output_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            window,
            move |_| {
                choose_export_dialog(&window, sender.clone());
            }
        ));

        duplicate_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::DuplicateSelection)));
        copy_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::CopySelection)));
        paste_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::PasteClipboard)));
        remove_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::RemoveSelection)));
        move_up_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::MoveSelectionUp)));
        move_down_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::MoveSelectionDown)));
        loop_dup_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::AppendDuplicateLoop)));
        loop_reverse_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::AppendReverseLoop(true))));
        loop_ping_pong_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::AppendReverseLoop(false))));
        batch_duration_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            batch_duration_spin,
            move |_| sender.input(AppMsg::ApplyBatchDuration(batch_duration_spin.value() as u32))
        ));
        rotate_left_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::RotateSelection(-1))));
        rotate_right_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::RotateSelection(1))));
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
                let crop = match (crop_w.value() as u32, crop_h.value() as u32) {
                    (0, 0) => None,
                    (width, height) => Some(CropRect {
                        x: crop_x.value() as u32,
                        y: crop_y.value() as u32,
                        width,
                        height,
                    }),
                };
                let resize = match (resize_w.value() as u32, resize_h.value() as u32) {
                    (0, 0) => None,
                    (width, height) if width > 0 && height > 0 => Some(ResizeTarget { width, height }),
                    _ => None,
                };
                sender.input(AppMsg::ApplyInspectorTransform(InspectorValues {
                    flip_horizontal: flip_h_check.is_active(),
                    flip_vertical: flip_v_check.is_active(),
                    crop,
                    resize,
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
                crop_x.set_value(0.0);
                crop_y.set_value(0.0);
                crop_w.set_value(0.0);
                crop_h.set_value(0.0);
                resize_w.set_value(0.0);
                resize_h.set_value(0.0);
                flip_h_check.set_active(false);
                flip_v_check.set_active(false);
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
        export_button.connect_clicked(clone!(#[strong] sender, move |_| sender.input(AppMsg::ExportNow)));

        let drop_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::COPY);
        drop_target.connect_drop(clone!(
            #[strong]
            sender,
            move |_, value, _, _| {
                if let Ok(text) = value.get::<String>() {
                    let paths = parse_uri_list(&text);
                    if !paths.is_empty() {
                        sender.input(AppMsg::ImportPaths(paths));
                        return true;
                    }
                }
                false
            }
        ));
        frame_list.add_controller(drop_target);

        let widgets = AppWidgets {
            frame_list,
            diagnostics_label,
            selection_label,
            status_label,
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
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppMsg::ImportPaths(paths) => {
                let valid = filter_image_paths(paths);
                if valid.is_empty() {
                    self.status = "No supported image files were provided.".to_string();
                } else {
                    self.timeline.import_paths(valid);
                    self.refresh_all_thumbnails();
                    self.status = format!("Imported {} frame(s).", self.timeline.frames().len());
                }
            }
            AppMsg::ToggleSelected(id, selected) => {
                if selected {
                    self.selection.insert(id);
                } else {
                    self.selection.remove(&id);
                }
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
                self.timeline.apply_duration(&self.selection, duration.max(10));
                self.status = format!("Applied {} ms to selected frames.", duration.max(10));
            }
            AppMsg::MoveSelectionUp => self.timeline.move_selection_up(&self.selection),
            AppMsg::MoveSelectionDown => self.timeline.move_selection_down(&self.selection),
            AppMsg::DuplicateSelection => {
                let inserted = self.timeline.duplicate_selected(&self.selection);
                self.selection = inserted.into_iter().collect();
                self.refresh_all_thumbnails();
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
                let inserted = self
                    .timeline
                    .paste_after_selection(&self.selection, &self.clipboard);
                self.selection = inserted.into_iter().collect();
                self.refresh_all_thumbnails();
            }
            AppMsg::RemoveSelection => {
                let removed = self.selection.len();
                self.timeline.remove_selected(&self.selection);
                self.selection.clear();
                self.status = format!("Removed {removed} frame(s).");
            }
            AppMsg::AppendDuplicateLoop => {
                let inserted = self.timeline.append_duplicate_loop(&self.selection);
                self.selection = inserted.into_iter().collect();
                self.refresh_all_thumbnails();
            }
            AppMsg::AppendReverseLoop(repeat_edges) => {
                let inserted = self.timeline.append_reverse_loop(&self.selection, repeat_edges);
                self.selection = inserted.into_iter().collect();
                self.refresh_all_thumbnails();
            }
            AppMsg::RotateSelection(delta) => {
                self.apply_to_selection(|frame| {
                    frame.transform_spec.rotate_quarter_turns += delta;
                });
                self.refresh_selection_thumbnails();
            }
            AppMsg::ApplyInspectorTransform(values) => {
                self.apply_to_selection(|frame| {
                    frame.transform_spec.flip_horizontal = values.flip_horizontal;
                    frame.transform_spec.flip_vertical = values.flip_vertical;
                    frame.transform_spec.crop = values.crop;
                    frame.transform_spec.resize = values.resize;
                    frame.transform_spec.fit_mode = values.fit_mode;
                });
                self.refresh_selection_thumbnails();
            }
            AppMsg::SetExportPreset(preset) => self.export_profile.apply_preset(preset),
            AppMsg::SetOutputPath(path) => {
                self.last_output_path = (!path.trim().is_empty()).then_some(PathBuf::from(path.trim()));
            }
            AppMsg::SetOutputWidth(width) => {
                self.export_profile.output_width = if width == 0 { None } else { Some(width) }
            }
            AppMsg::SetOutputHeight(height) => {
                self.export_profile.output_height = if height == 0 { None } else { Some(height) }
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
            AppMsg::OpenProject(path) => match load_project(&path) {
                Ok(document) => {
                    self.timeline = Timeline::from_frames(document.frames);
                    self.selection.clear();
                    self.export_profile = document.export_profile;
                    self.last_output_path = document.last_output_path;
                    self.refresh_all_thumbnails();
                    self.status = format!("Loaded project {}", path.display());
                }
                Err(err) => self.status = format!("Failed to load project: {err}"),
            },
            AppMsg::ChooseOutputPath(path) => {
                self.last_output_path = Some(path);
            }
            AppMsg::ExportNow => {
                let Some(output_path) = self.last_output_path.clone() else {
                    self.status = "Choose an output path first.".to_string();
                    self.recompute_command_preview();
                    return;
                };

                match export_animation(self.timeline.frames(), &self.export_profile, &output_path) {
                    Ok(job) => self.status = format!("Exported {}", job.output_path.display()),
                    Err(err) => self.status = format!("Export failed: {err}"),
                }
            }
            AppMsg::SetStatus(status) => self.status = status,
        }

        self.recompute_command_preview();
        let _ = sender;
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        widgets.diagnostics_label.set_label(&self.diagnostics.summary());
        widgets.selection_label.set_label(&format!(
            "{} selected / {} total",
            self.selection.len(),
            self.timeline.frames().len()
        ));
        widgets.status_label.set_label(&self.status);
        widgets.command_preview_label.set_label(&self.command_preview);

        let output_text = self
            .last_output_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if widgets.output_entry.text().as_str() != output_text {
            widgets.output_entry.set_text(&output_text);
        }
        sync_combo_active_export_preset(&widgets.preset_combo, self.export_profile.preset);
        sync_combo_active_encoder_preset(&widgets.encoder_combo, self.export_profile.encoder_preset);
        sync_combo_active_fit_mode(&widgets.fit_mode_combo, self.export_profile.fit_mode);
        widgets.quality_spin.set_value(self.export_profile.quality as f64);
        widgets
            .width_spin
            .set_value(self.export_profile.output_width.unwrap_or_default() as f64);
        widgets
            .height_spin
            .set_value(self.export_profile.output_height.unwrap_or_default() as f64);
        widgets.lossless_check.set_active(self.export_profile.lossless);
        widgets
            .cr_threshold_spin
            .set_value(self.export_profile.cr_threshold as f64);
        widgets.cr_size_spin.set_value(self.export_profile.cr_size as f64);
        widgets.loop_spin.set_value(self.export_profile.loop_count as f64);
        widgets.overwrite_check.set_active(self.export_profile.overwrite);
        if widgets.raw_args_entry.text().as_str() != self.export_profile.raw_args {
            widgets.raw_args_entry.set_text(&self.export_profile.raw_args);
        }

        while let Some(child) = widgets.frame_list.first_child() {
            widgets.frame_list.remove(&child);
        }

        for (index, frame) in self.timeline.frames().iter().enumerate() {
            widgets
                .frame_list
                .append(&build_frame_row(frame, index, self.selection.contains(&frame.id), sender.clone()));
        }
    }
}

impl AppModel {
    fn frame_mut(&mut self, id: u64) -> Option<&mut FrameItem> {
        self.timeline.frames_mut().iter_mut().find(|frame| frame.id == id)
    }

    fn refresh_all_thumbnails(&mut self) {
        for frame in self.timeline.frames_mut() {
            populate_frame_metadata(frame);
            if let Err(err) = refresh_thumbnail(frame, &self.cache_dir) {
                error!("thumbnail refresh failed for {}: {err}", frame.source_path.display());
            }
        }
    }

    fn refresh_selection_thumbnails(&mut self) {
        for frame in self.timeline.frames_mut() {
            if self.selection.contains(&frame.id) {
                populate_frame_metadata(frame);
                if let Err(err) = refresh_thumbnail(frame, &self.cache_dir) {
                    error!("thumbnail refresh failed for {}: {err}", frame.source_path.display());
                }
            }
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
}

fn build_frame_row(
    frame: &FrameItem,
    index: usize,
    selected: bool,
    sender: ComponentSender<AppModel>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let frame_id = frame.id;
    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    row.set_child(Some(&row_box));

    let select = gtk::CheckButton::new();
    select.set_active(selected);
    select.connect_toggled(clone!(
        #[strong]
        sender,
        move |check| sender.input(AppMsg::ToggleSelected(frame_id, check.is_active()))
    ));
    row_box.append(&select);

    let picture = if let Some(path) = frame.thumbnail_path.as_ref() {
        gtk::Picture::for_filename(path)
    } else {
        gtk::Picture::new()
    };
    picture.set_size_request(80, 80);
    picture.set_can_shrink(true);
    row_box.append(&picture);

    let meta = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();
    let title = gtk::Label::new(Some(&format!("{:03} {}", index + 1, frame.file_name())));
    title.set_xalign(0.0);
    let dims = frame
        .source_dimensions
        .map(|(w, h)| format!("{w}x{h}"))
        .unwrap_or_else(|| "unknown size".to_string());
    let subtitle = gtk::Label::new(Some(&format!(
        "{} | {} ms | {}",
        dims,
        frame.duration_ms,
        if frame.enabled { "enabled" } else { "disabled" }
    )));
    subtitle.set_xalign(0.0);
    meta.append(&title);
    meta.append(&subtitle);
    row_box.append(&meta);

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let duration_spin = gtk::SpinButton::with_range(10.0, 30_000.0, 5.0);
    duration_spin.set_value(frame.duration_ms as f64);
    duration_spin.connect_value_changed(clone!(
        #[strong]
        sender,
        move |spin| sender.input(AppMsg::SetFrameDuration(frame_id, spin.value() as u32))
    ));
    let enabled_check = gtk::CheckButton::with_label("On");
    enabled_check.set_active(frame.enabled);
    enabled_check.connect_toggled(clone!(
        #[strong]
        sender,
        move |check| sender.input(AppMsg::ToggleEnabled(frame_id, check.is_active()))
    ));
    controls.append(&gtk::Label::new(Some("Duration")));
    controls.append(&duration_spin);
    controls.append(&enabled_check);
    row_box.append(&controls);

    row
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
    combo.set_active(Some(match mode {
        FitMode::Contain => 0,
        FitMode::Cover => 1,
        FitMode::Stretch => 2,
    }));
}

fn sync_combo_active_export_preset(combo: &gtk::ComboBoxText, preset: ExportPreset) {
    combo.set_active(Some(match preset {
        ExportPreset::FastPreview => 0,
        ExportPreset::Balanced => 1,
        ExportPreset::HighQuality => 2,
        ExportPreset::Lossless => 3,
    }));
}

fn sync_combo_active_encoder_preset(combo: &gtk::ComboBoxText, preset: EncoderPreset) {
    combo.set_active(Some(match preset {
        EncoderPreset::Default => 0,
        EncoderPreset::Picture => 1,
        EncoderPreset::Photo => 2,
        EncoderPreset::Drawing => 3,
        EncoderPreset::Icon => 4,
        EncoderPreset::Text => 5,
    }));
}

fn section<W: IsA<gtk::Widget>>(title: &str, child: &W) -> gtk::Frame {
    gtk::Frame::builder().label(title).child(child).build()
}

fn attach_labeled_spin(grid: &gtk::Grid, label: &str, spin: &gtk::SpinButton, column: i32, row: i32) {
    grid.attach(&gtk::Label::new(Some(label)), column, row, 1, 1);
    grid.attach(spin, column, row + 1, 1, 1);
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
    for mime in ["image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp", "image/tiff"] {
        filter.add_mime_type(mime);
    }
    dialog.add_filter(&filter);
}

fn filter_image_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|value| matches!(value.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"))
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
            gio::File::for_uri(line).path()
        })
        .collect()
}
