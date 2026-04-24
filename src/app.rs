use std::cell::Cell;
use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::export::{
    build_command_preview, export_animation_with_progress, normalized_output_path,
};
use crate::preferences::{UiPreferences, load_ui_preferences, save_ui_preferences};
use crate::project::{load_autosave_project, load_project, save_autosave_project, save_project};
use crate::runtime::{Diagnostics, collect_diagnostics};
use crate::selection::{SelectionMode, apply_selection};
use crate::thumbnail::{
    ensure_cache_dir, export_preview_cache_path, populate_frame_metadata, preview_cache_path,
    refresh_thumbnail, render_export_preview, render_preview,
};
use crate::timeline::Timeline;
use crate::types::{
    CropRect, EncoderPreset, ExportJob, ExportPreset, ExportProfile, FitMode, FrameItem,
    ProjectDocument, ResizeTarget,
};
use gtk::glib::clone;
use gtk::prelude::*;
use gtk::{gdk, gio};
use relm4::{Component, ComponentParts, ComponentSender, RelmApp};

const DEFAULT_PREVIEW_LOGICAL_WIDTH: i32 = 720;
const DEFAULT_PREVIEW_LOGICAL_HEIGHT: i32 = 360;
const MAX_PREVIEW_RENDER_EDGE: u32 = 4096;
static EXPORT_LAYOUT_WATCH_SUSPENDED: AtomicBool = AtomicBool::new(false);

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
    WindowLayoutChanged(i32),
    SetActiveTab(WorkflowTab),
    SetAdvancedMode(bool),
    PreviewLayoutChanged {
        tab: WorkflowTab,
        size: PreviewRenderSize,
    },
    RunDiagnostics,
    PreviewExport,
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
    SetLoopMethod(LoopMethod),
    SetLoopRepeats(u32),
    SetLoopScope(LoopScope),
    CreateLoop,
    SetCropPreset(CropPreset),
    SetCropAnchor(CropAnchor),
    ApplyQuickCrop,
    ClearQuickCrop,
    SetQuickResizePreset(DimensionPreset),
    SetQuickResizeMode(QuickResizeMode),
    SetQuickResizeMultiplier(f64),
    SetQuickResizeCustomWidth(u32),
    SetQuickResizeCustomHeight(u32),
    ApplyQuickResize,
    RotateSelection(i32),
    ToggleSelectionFlip {
        horizontal: bool,
    },
    ApplyInspectorTransform(InspectorValues),
    SetExportPreset(ExportPreset),
    SetExportSizePreset(DimensionPreset),
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
    StartExportWorker(ExportStartRequest),
    PollExportState,
    FinalizeExportUi,
    ResumePreviewLayoutWatch,
    CompleteExportUiRestore,
    CloseWithAutosave {
        window: gtk::Window,
        close_allowed: Rc<Cell<bool>>,
    },
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
    ExportPreviewReady {
        frame_id: u64,
        generation: u64,
        render_size: PreviewRenderSize,
        preview_path: Option<PathBuf>,
        error: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowTab {
    Organize,
    Edit,
    Loop,
    Export,
    Diagnostics,
}

impl WorkflowTab {
    fn stack_name(self) -> &'static str {
        match self {
            Self::Organize => "organize",
            Self::Edit => "edit",
            Self::Loop => "loop",
            Self::Export => "export",
            Self::Diagnostics => "diagnostics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterStatusScope {
    Global,
    Organize,
    Edit,
    Loop,
    Export,
    Diagnostics,
}

impl FooterStatusScope {
    fn matches_tab(self, tab: WorkflowTab) -> bool {
        match self {
            Self::Global => false,
            Self::Organize => tab == WorkflowTab::Organize,
            Self::Edit => tab == WorkflowTab::Edit,
            Self::Loop => tab == WorkflowTab::Loop,
            Self::Export => tab == WorkflowTab::Export,
            Self::Diagnostics => tab == WorkflowTab::Diagnostics,
        }
    }
}

impl From<WorkflowTab> for FooterStatusScope {
    fn from(tab: WorkflowTab) -> Self {
        match tab {
            WorkflowTab::Organize => Self::Organize,
            WorkflowTab::Edit => Self::Edit,
            WorkflowTab::Loop => Self::Loop,
            WorkflowTab::Export => Self::Export,
            WorkflowTab::Diagnostics => Self::Diagnostics,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMethod {
    Duplicate,
    Reverse,
    PingPong,
}

impl LoopMethod {
    fn title(self) -> &'static str {
        match self {
            Self::Duplicate => "Duplicate",
            Self::Reverse => "Reverse",
            Self::PingPong => "Ping-Pong",
        }
    }

    fn helper_text(self) -> &'static str {
        match self {
            Self::Duplicate => "Repeat the sequence from start to finish.",
            Self::Reverse => "Play forward, then backward to the start.",
            Self::PingPong => "Play forward, then backward without repeating the endpoints.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopScope {
    Selected,
    AllFrames,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionPreset {
    Original,
    Hd1080,
    Hd720,
    Custom,
}

impl DimensionPreset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Hd1080 => "1080p",
            Self::Hd720 => "720p",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickResizeMode {
    Multiplier,
    Custom,
}

impl QuickResizeMode {
    fn stack_name(self) -> &'static str {
        match self {
            Self::Multiplier => "multiplier",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropPreset {
    Square,
    Landscape16x9,
    Portrait9x16,
}

impl CropPreset {
    fn title(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Landscape16x9 => "Widescreen",
            Self::Portrait9x16 => "Story",
        }
    }

    fn helper_text(self) -> &'static str {
        match self {
            Self::Square => "1:1 crop for avatars, thumbnails, and grids.",
            Self::Landscape16x9 => "16:9 crop for video-like framing and banners.",
            Self::Portrait9x16 => "9:16 crop for stories, reels, and vertical posts.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropAnchor {
    Start,
    Center,
    End,
}

impl CropAnchor {
    fn helper_text(self) -> &'static str {
        match self {
            Self::Start => "Bias the crop toward the top or left edge.",
            Self::Center => "Balance the crop around the middle.",
            Self::End => "Bias the crop toward the bottom or right edge.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Regular,
    Compact,
}

#[derive(Debug, Clone)]
struct ExportProgressState {
    fraction: f64,
    detail: String,
}

#[derive(Debug, Clone)]
struct ExportWorkerState {
    version: u64,
    progress: Option<ExportProgressState>,
    result: Option<Result<ExportJob, String>>,
}

#[derive(Debug, Clone)]
pub struct ExportStartRequest {
    frames: Vec<FrameItem>,
    profile: ExportProfile,
    output_path: PathBuf,
    export_worker_state: Arc<Mutex<ExportWorkerState>>,
}

pub struct AppModel {
    timeline: Timeline,
    selection: BTreeSet<u64>,
    selection_anchor_id: Option<u64>,
    clipboard: Vec<FrameItem>,
    export_profile: ExportProfile,
    diagnostics: Diagnostics,
    status: String,
    footer_status_scope: FooterStatusScope,
    ui_preferences: UiPreferences,
    layout_mode: LayoutMode,
    active_tab: WorkflowTab,
    advanced_mode: bool,
    loop_method: LoopMethod,
    loop_repeats: u32,
    loop_scope: LoopScope,
    crop_preset: CropPreset,
    crop_anchor: CropAnchor,
    quick_resize_preset: DimensionPreset,
    quick_resize_mode: QuickResizeMode,
    quick_resize_multiplier: f64,
    quick_resize_custom_width: u32,
    quick_resize_custom_height: u32,
    cache_dir: PathBuf,
    last_output_path: Option<PathBuf>,
    command_preview: String,
    preview_path: Option<PathBuf>,
    preview_frame_id: Option<u64>,
    export_preview_path: Option<PathBuf>,
    preview_target_size: PreviewRenderSize,
    preview_rendered_size: Option<PreviewRenderSize>,
    export_preview_rendered_size: Option<PreviewRenderSize>,
    export_preview_generation: u64,
    playback_active: bool,
    playback_generation: u64,
    thumbnails_pending: usize,
    export_in_progress: bool,
    export_completion_pending: bool,
    export_progress: Option<ExportProgressState>,
    export_worker_state: Option<Arc<Mutex<ExportWorkerState>>>,
    export_poll_active: Option<Rc<Cell<bool>>>,
}

pub struct AppWidgets {
    workspace_box: gtk::Box,
    content_stack: gtk::Stack,
    tab_organize_button: gtk::Button,
    tab_edit_button: gtk::Button,
    tab_loop_button: gtk::Button,
    tab_export_button: gtk::Button,
    advanced_switch: gtk::Switch,
    preview_panel: gtk::Box,
    timeline_toolbar: gtk::Box,
    loop_body: gtk::Box,
    loop_settings_column: gtk::Box,
    export_body: gtk::Box,
    export_settings_column: gtk::Box,
    timeline_strip: gtk::Box,
    nav_first_button: gtk::Button,
    nav_prev_button: gtk::Button,
    nav_play_button: gtk::Button,
    nav_next_button: gtk::Button,
    nav_last_button: gtk::Button,
    diagnostics_label: gtk::Label,
    diagnostics_overview_label: gtk::Label,
    diagnostics_details_box: gtk::Box,
    status_label: gtk::Label,
    footer_frames_label: gtk::Label,
    footer_duration_label: gtk::Label,
    footer_progress_bar: gtk::ProgressBar,
    footer_state_label: gtk::Label,
    preview_picture: gtk::Picture,
    preview_meta: gtk::Label,
    loop_preview_picture: gtk::Picture,
    export_preview_picture: gtk::Picture,
    crop_summary_label: gtk::Label,
    crop_square_button: gtk::Button,
    crop_widescreen_button: gtk::Button,
    crop_story_button: gtk::Button,
    crop_start_button: gtk::Button,
    crop_center_button: gtk::Button,
    crop_end_button: gtk::Button,
    apply_crop_button: gtk::Button,
    clear_crop_button: gtk::Button,
    output_entry: gtk::Entry,
    suppress_output_entry_change: Rc<Cell<bool>>,
    quick_resize_combo: gtk::ComboBoxText,
    quick_resize_custom_panel: gtk::Box,
    quick_resize_mode_stack: gtk::Stack,
    quick_resize_multiplier_spin: gtk::SpinButton,
    quick_resize_custom_width_spin: gtk::SpinButton,
    quick_resize_custom_height_spin: gtk::SpinButton,
    export_size_combo: gtk::ComboBoxText,
    loop_source_label: gtk::Label,
    loop_summary_label: gtk::Label,
    loop_repeats_spin: gtk::SpinButton,
    loop_create_button: gtk::Button,
    loop_duplicate_button: gtk::Button,
    loop_reverse_button: gtk::Button,
    loop_ping_pong_button: gtk::Button,
    loop_scope_selected_button: gtk::Button,
    loop_scope_all_button: gtk::Button,
    export_preset_fast_button: gtk::Button,
    export_preset_balanced_button: gtk::Button,
    export_preset_high_button: gtk::Button,
    export_preset_lossless_button: gtk::Button,
    export_summary_dimensions_value: gtk::Label,
    export_summary_frame_count_value: gtk::Label,
    export_summary_duration_value: gtk::Label,
    export_summary_loop_count_value: gtk::Label,
    export_summary_quality_value: gtk::Label,
    export_summary_estimated_size_value: gtk::Label,
    export_advanced_box: gtk::Box,
    edit_advanced_box: gtk::Box,
    preview_export_button: gtk::Button,
    export_button: gtk::Button,
    export_action_icon: gtk::Image,
    export_action_title_label: gtk::Label,
    export_action_detail_label: gtk::Label,
    quality_scale: gtk::Scale,
    quality_spin: gtk::SpinButton,
    width_spin: gtk::SpinButton,
    height_spin: gtk::SpinButton,
    lossless_check: gtk::CheckButton,
    encoder_combo: gtk::ComboBoxText,
    cr_threshold_spin: gtk::SpinButton,
    cr_size_spin: gtk::SpinButton,
    loop_spin: gtk::SpinButton,
    loop_count_combo: gtk::ComboBoxText,
    overwrite_switch: gtk::Switch,
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
    timeline_render_signature: u64,
    preview_picture_path: Option<PathBuf>,
    loop_preview_picture_path: Option<PathBuf>,
    export_preview_picture_path: Option<PathBuf>,
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
            .default_width(1180)
            .default_height(720)
            .resizable(true)
            .build()
    }

    fn init(
        _init: Self::Init,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        window.add_css_class("app-window");
        install_app_css(&window);
        let cache_dir = ensure_cache_dir().unwrap_or_else(|_| std::env::temp_dir());
        let diagnostics = collect_diagnostics();
        let (ui_preferences, status) = match load_ui_preferences() {
            Ok(preferences) => (
                preferences,
                "Import images to begin building an animated WebP.".to_string(),
            ),
            Err(err) => (
                UiPreferences::default(),
                format!(
                    "Import images to begin building an animated WebP. UI preferences could not be loaded: {err}"
                ),
            ),
        };
        let mut model = AppModel {
            timeline: Timeline::new(),
            selection: BTreeSet::new(),
            selection_anchor_id: None,
            clipboard: Vec::new(),
            export_profile: ExportProfile::default(),
            diagnostics,
            status,
            footer_status_scope: FooterStatusScope::Global,
            ui_preferences: ui_preferences.clone(),
            layout_mode: layout_mode_for_width(1280),
            active_tab: WorkflowTab::Organize,
            advanced_mode: ui_preferences.advanced_mode,
            loop_method: LoopMethod::PingPong,
            loop_repeats: 1,
            loop_scope: LoopScope::Selected,
            crop_preset: CropPreset::Square,
            crop_anchor: CropAnchor::Center,
            quick_resize_preset: DimensionPreset::Original,
            quick_resize_mode: QuickResizeMode::Multiplier,
            quick_resize_multiplier: 1.0,
            quick_resize_custom_width: 0,
            quick_resize_custom_height: 0,
            cache_dir,
            last_output_path: None,
            command_preview: String::new(),
            preview_path: None,
            preview_frame_id: None,
            export_preview_path: None,
            preview_target_size: PreviewRenderSize {
                width: DEFAULT_PREVIEW_LOGICAL_WIDTH as u32,
                height: DEFAULT_PREVIEW_LOGICAL_HEIGHT as u32,
            },
            preview_rendered_size: None,
            export_preview_rendered_size: None,
            export_preview_generation: 0,
            playback_active: false,
            playback_generation: 0,
            thumbnails_pending: 0,
            export_in_progress: false,
            export_completion_pending: false,
            export_progress: None,
            export_worker_state: None,
            export_poll_active: None,
        };
        let restored_frame_ids = match load_autosave_project() {
            Ok(Some(document)) => {
                let ids = model.apply_project_document(document);
                model.set_status(
                    FooterStatusScope::Global,
                    format!("Restored autosaved project with {} frame(s).", ids.len()),
                );
                ids
            }
            Ok(None) => Vec::new(),
            Err(err) => {
                model.set_status(
                    FooterStatusScope::Global,
                    format!(
                        "Import images to begin building an animated WebP. Autosave could not be loaded: {err}"
                    ),
                );
                Vec::new()
            }
        };
        model.recompute_command_preview();
        model.sync_quick_resize_state_from_selection();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        root.add_css_class("app-shell");
        window.set_child(Some(&root));

        let header_handle = gtk::WindowHandle::new();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(8)
            .margin_end(8)
            .build();
        header.add_css_class("top-shell");
        let import_button =
            build_labeled_button("Import Images", "folder-open-symbolic", "icon-tone-cyan");
        let open_project_button =
            build_labeled_button("Open Project", "document-open-symbolic", "icon-tone-amber");
        let save_project_button =
            build_labeled_button("Save Project", "document-save-symbolic", "icon-tone-green");
        let run_diagnostics_button =
            build_labeled_button("Diagnostics", "system-run-symbolic", "icon-tone-coral");
        let window_controls_start = gtk::WindowControls::builder()
            .side(gtk::PackType::Start)
            .build();
        let window_controls_end = gtk::WindowControls::builder()
            .side(gtk::PackType::End)
            .build();
        let header_left = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();
        header_left.append(&window_controls_start);

        let title_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        let title_label = gtk::Label::new(Some("AWEBPinator"));
        title_label.add_css_class("title-2");
        title_label.set_halign(gtk::Align::Center);
        title_box.append(&title_label);

        let advanced_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk::Align::Center)
            .build();
        let advanced_label = gtk::Label::new(Some("Advanced"));
        advanced_label.set_xalign(1.0);
        let advanced_switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
        set_accessible_label(&advanced_switch, "Advanced mode");
        advanced_box.append(&advanced_label);
        advanced_box.append(&advanced_switch);
        let menu_button = gtk::MenuButton::new();
        set_accessible_label(&menu_button, "Secondary actions");
        menu_button.add_css_class("pill-button");
        menu_button.set_icon_name("open-menu-symbolic");
        let secondary_menu = gtk::Popover::new();
        let secondary_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(10)
            .margin_end(10)
            .width_request(220)
            .build();
        for button in [
            &import_button,
            &open_project_button,
            &save_project_button,
            &run_diagnostics_button,
        ] {
            button.add_css_class("menu-action");
            secondary_actions.append(button);
        }
        secondary_menu.set_child(Some(&secondary_actions));
        menu_button.set_popover(Some(&secondary_menu));
        let header_right = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .hexpand(true)
            .halign(gtk::Align::End)
            .build();
        header_right.append(&advanced_box);
        header_right.append(&menu_button);
        header_right.append(&window_controls_end);

        header.append(&header_left);
        header.append(&title_box);
        header.append(&header_right);
        header_handle.set_child(Some(&header));
        window.set_titlebar(Some(&header_handle));

        let tab_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();
        tab_bar.add_css_class("workflow-tabs");
        let tab_organize_button =
            build_tab_button("Organize", "view-grid-symbolic", "icon-tone-cyan");
        let tab_edit_button = build_tab_button("Edit", "document-edit-symbolic", "icon-tone-cyan");
        let tab_loop_button =
            build_tab_button("Timeline", "view-refresh-symbolic", "icon-tone-amber");
        let tab_export_button = build_tab_button("Export", "mail-send-symbolic", "icon-tone-green");
        for button in [
            &tab_organize_button,
            &tab_edit_button,
            &tab_loop_button,
            &tab_export_button,
        ] {
            tab_bar.append(button);
        }
        let tab_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(48)
            .child(&tab_bar)
            .build();
        root.append(&tab_scroller);

        let workspace_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.append(&workspace_box);

        let preview_panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .build();
        preview_panel.add_css_class("preview-panel");
        let preview_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .vexpand(true)
            .build();
        let preview_picture = gtk::Picture::new();
        set_accessible_label(&preview_picture, "Selected frame preview");
        preview_picture.set_size_request(320, 200);
        preview_picture.set_can_shrink(true);
        preview_picture.set_hexpand(true);
        preview_picture.set_vexpand(true);
        model.preview_target_size = preview_render_size_for_widget(&preview_picture);
        install_preview_layout_watch(&preview_picture, WorkflowTab::Edit, sender.clone());
        let preview_meta = gtk::Label::new(Some("Select a frame to inspect it."));
        preview_meta.set_xalign(0.0);
        preview_meta.set_wrap(true);
        preview_meta.add_css_class("dim-label");
        preview_box.append(&preview_picture);
        preview_box.append(&preview_meta);
        let preview_frame = section("Preview", &preview_box);
        preview_frame.set_hexpand(true);
        preview_frame.set_vexpand(true);
        preview_panel.append(&preview_frame);
        workspace_box.append(&preview_panel);

        let page_stack = gtk::Stack::builder()
            .hexpand(true)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        workspace_box.append(&page_stack);
        install_workspace_split_watch(&workspace_box, &preview_panel, &page_stack);

        let organize_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let organize_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();
        let organize_import_button =
            build_labeled_button("Import Images", "folder-open-symbolic", "icon-tone-cyan");
        let organize_open_button =
            build_labeled_button("Open Project", "document-open-symbolic", "icon-tone-amber");
        let organize_save_button =
            build_labeled_button("Save Project", "document-save-symbolic", "icon-tone-green");
        for button in [
            &organize_import_button,
            &organize_open_button,
            &organize_save_button,
        ] {
            button.add_css_class("pill-button");
            organize_actions.append(button);
        }
        organize_page.append(&section("Project", &organize_actions));
        organize_page.append(&section(
            "Selection",
            &summary_label(
                "Select frames in the timeline below. Drag thumbnails to reorder frames.",
            ),
        ));
        let organize_scroll = page_scroller(&organize_page);
        page_stack.add_titled(
            &organize_scroll,
            Some(WorkflowTab::Organize.stack_name()),
            "Organize",
        );

        let edit_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let quick_actions_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        quick_actions_grid.set_column_homogeneous(true);
        quick_actions_grid.set_row_homogeneous(true);
        quick_actions_grid.set_hexpand(true);
        let rotate_left_button = build_labeled_button(
            "Rotate Left",
            "object-rotate-left-symbolic",
            "icon-tone-cyan",
        );
        let rotate_right_button = build_labeled_button(
            "Rotate Right",
            "object-rotate-right-symbolic",
            "icon-tone-cyan",
        );
        let flip_horizontal_button = build_labeled_button(
            "Flip Horizontal",
            "object-flip-horizontal-symbolic",
            "icon-tone-amber",
        );
        let flip_vertical_button = build_labeled_button(
            "Flip Vertical",
            "object-flip-vertical-symbolic",
            "icon-tone-amber",
        );
        for button in [
            &rotate_left_button,
            &rotate_right_button,
            &flip_horizontal_button,
            &flip_vertical_button,
        ] {
            button.add_css_class("pill-button");
            button.add_css_class("edit-quick-action");
            button.set_hexpand(true);
            button.set_vexpand(true);
            button.set_halign(gtk::Align::Fill);
            button.set_valign(gtk::Align::Fill);
        }
        quick_actions_grid.attach(&rotate_left_button, 0, 0, 1, 1);
        quick_actions_grid.attach(&rotate_right_button, 1, 0, 1, 1);
        quick_actions_grid.attach(&flip_horizontal_button, 0, 1, 1, 1);
        quick_actions_grid.attach(&flip_vertical_button, 1, 1, 1, 1);
        let quick_actions_section = section("Quick Actions", &quick_actions_grid);
        quick_actions_section.set_hexpand(true);
        edit_page.append(&quick_actions_section);

        let crop_preset_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();
        let crop_square_button = build_choice_button(
            "Square",
            "1:1",
            "image-x-generic-symbolic",
            "icon-tone-cyan",
        );
        let crop_widescreen_button = build_choice_button(
            "Widescreen",
            "16:9",
            "video-x-generic-symbolic",
            "icon-tone-green",
        );
        let crop_story_button =
            build_choice_button("Story", "9:16", "camera-photo-symbolic", "icon-tone-coral");
        for button in [
            &crop_square_button,
            &crop_widescreen_button,
            &crop_story_button,
        ] {
            crop_preset_row.append(button);
        }
        let crop_anchor_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let crop_start_button =
            build_labeled_button("Anchor Left", "go-first-symbolic", "icon-tone-cyan");
        let crop_center_button = build_labeled_button(
            "Anchor Center",
            "align-horizontal-center-symbolic",
            "icon-tone-green",
        );
        let crop_end_button =
            build_labeled_button("Anchor Right", "go-last-symbolic", "icon-tone-coral");
        for button in [&crop_start_button, &crop_center_button, &crop_end_button] {
            button.add_css_class("pill-button");
            crop_anchor_row.append(button);
        }
        let crop_action_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let apply_crop_button =
            build_labeled_button("Apply", "emblem-ok-symbolic", "icon-tone-green");
        apply_crop_button.add_css_class("suggested-action");
        let clear_crop_button =
            build_labeled_button("Clear", "edit-clear-symbolic", "icon-tone-coral");
        clear_crop_button.add_css_class("pill-button");
        crop_action_row.append(&apply_crop_button);
        crop_action_row.append(&clear_crop_button);
        let crop_summary_label =
            summary_label("Choose a crop shape, then apply it to the selected frames.");
        let crop_controls_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        crop_controls_box.append(&crop_preset_row);
        crop_controls_box.append(&crop_anchor_row);
        crop_controls_box.append(&crop_action_row);
        crop_controls_box.append(&crop_summary_label);
        let quick_resize_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();
        let quick_resize_combo = combo_for_dimension_preset();
        set_accessible_label(&quick_resize_combo, "Quick resize preset");
        quick_resize_combo.add_css_class("control-surface");
        quick_resize_combo.set_hexpand(true);
        quick_resize_combo.set_halign(gtk::Align::Fill);
        let quick_apply_button =
            build_labeled_button("Apply", "emblem-ok-symbolic", "icon-tone-green");
        quick_apply_button.add_css_class("suggested-action");
        quick_apply_button.set_halign(gtk::Align::End);
        quick_resize_row.append(&quick_resize_combo);
        quick_resize_row.append(&quick_apply_button);
        let quick_resize_custom_panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .visible(false)
            .build();
        let quick_resize_mode_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .hexpand(true)
            .build();
        let quick_resize_mode_switcher = gtk::StackSwitcher::new();
        quick_resize_mode_switcher.set_stack(Some(&quick_resize_mode_stack));
        quick_resize_mode_switcher.set_halign(gtk::Align::Start);
        quick_resize_custom_panel.append(&quick_resize_mode_switcher);

        let multiplier_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let multiplier_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let multiplier_down_button =
            build_icon_button("list-remove-symbolic", "Decrease resize multiplier by 1");
        let quick_resize_multiplier_spin = gtk::SpinButton::with_range(0.1, 64.0, 1.0);
        quick_resize_multiplier_spin.set_digits(2);
        quick_resize_multiplier_spin.set_hexpand(true);
        quick_resize_multiplier_spin.set_halign(gtk::Align::Fill);
        quick_resize_multiplier_spin.add_css_class("control-surface");
        set_accessible_label(&quick_resize_multiplier_spin, "Resize multiplier");
        let multiplier_up_button =
            build_icon_button("list-add-symbolic", "Increase resize multiplier by 1");
        multiplier_row.append(&multiplier_down_button);
        multiplier_row.append(&quick_resize_multiplier_spin);
        multiplier_row.append(&multiplier_up_button);
        multiplier_page.append(&helper_label(
            "Scale the selected frame by a typed multiplier, or step it up and down in whole numbers.",
        ));
        multiplier_page.append(&multiplier_row);
        quick_resize_mode_stack.add_titled(
            &multiplier_page,
            Some(QuickResizeMode::Multiplier.stack_name()),
            "Multiplier",
        );

        let custom_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let custom_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        let quick_resize_custom_width_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        quick_resize_custom_width_spin.set_hexpand(true);
        quick_resize_custom_width_spin.set_halign(gtk::Align::Fill);
        quick_resize_custom_width_spin.add_css_class("control-surface");
        set_accessible_label(&quick_resize_custom_width_spin, "Quick resize custom width");
        let quick_resize_custom_height_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        quick_resize_custom_height_spin.set_hexpand(true);
        quick_resize_custom_height_spin.set_halign(gtk::Align::Fill);
        quick_resize_custom_height_spin.add_css_class("control-surface");
        set_accessible_label(
            &quick_resize_custom_height_spin,
            "Quick resize custom height",
        );
        attach_labeled_spin(&custom_grid, "Width", &quick_resize_custom_width_spin, 0, 0);
        attach_labeled_spin(
            &custom_grid,
            "Height",
            &quick_resize_custom_height_spin,
            1,
            0,
        );
        custom_page.append(&helper_label(
            "Enter either width or height and the other value updates to keep the selected frame's aspect ratio.",
        ));
        custom_page.append(&custom_grid);
        quick_resize_mode_stack.add_titled(
            &custom_page,
            Some(QuickResizeMode::Custom.stack_name()),
            "Custom",
        );

        quick_resize_custom_panel.append(&quick_resize_mode_stack);
        let quick_resize_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        quick_resize_box.append(&quick_resize_row);
        quick_resize_box.append(&quick_resize_custom_panel);
        edit_page.append(&section("Guided Crop", &crop_controls_box));
        edit_page.append(&section("Resize", &quick_resize_box));

        let edit_advanced_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let transform_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        let flip_h_check = gtk::CheckButton::with_label("Flip H");
        let flip_v_check = gtk::CheckButton::with_label("Flip V");
        let crop_x = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_y = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_w = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let crop_h = gtk::SpinButton::with_range(0.0, 16384.0, 1.0);
        let resize_w = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let resize_h = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        let inspector_fit_combo = combo_for_fit_mode();
        set_accessible_label(&crop_x, "Crop X");
        set_accessible_label(&crop_y, "Crop Y");
        set_accessible_label(&crop_w, "Crop width");
        set_accessible_label(&crop_h, "Crop height");
        set_accessible_label(&resize_w, "Resize width");
        set_accessible_label(&resize_h, "Resize height");
        set_accessible_label(&inspector_fit_combo, "Edit fit mode");
        let apply_transform_button = build_labeled_button(
            "Apply to Selected Frames",
            "emblem-ok-symbolic",
            "icon-tone-green",
        );
        apply_transform_button.add_css_class("suggested-action");
        let clear_transform_button = build_labeled_button(
            "Clear Crop/Resize",
            "edit-clear-symbolic",
            "icon-tone-coral",
        );

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
        edit_advanced_box.append(&helper_label("Advanced mode exposes direct crop, resize, fit, and flip controls for expert adjustments."));
        edit_advanced_box.append(&transform_grid);
        edit_page.append(&collapsible_section(
            "Advanced Edit Controls",
            &edit_advanced_box,
            false,
        ));
        let edit_scroll = page_scroller(&edit_page);
        page_stack.add_titled(&edit_scroll, Some(WorkflowTab::Edit.stack_name()), "Edit");

        let loop_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let loop_body = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .homogeneous(true)
            .build();
        let loop_left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .build();

        let loop_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let loop_cards = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();
        let loop_duplicate_button = build_choice_button(
            "Duplicate",
            "Repeat the sequence from start to finish.",
            "edit-copy-symbolic",
            "icon-tone-cyan",
        );
        let loop_reverse_button = build_choice_button(
            "Reverse",
            "Play forward, then backward to the start.",
            "view-refresh-symbolic",
            "icon-tone-amber",
        );
        let loop_ping_pong_button = build_choice_button(
            "Ping-Pong",
            "Play forward, then backward without repeating the endpoints.",
            "media-playlist-repeat-symbolic",
            "icon-tone-green",
        );
        for button in [
            &loop_duplicate_button,
            &loop_reverse_button,
            &loop_ping_pong_button,
        ] {
            loop_cards.append(button);
        }
        loop_box.append(&loop_cards);
        let loop_source_label =
            helper_label("Select a range in the timeline to create a focused loop.");
        loop_box.append(&loop_source_label);
        let loop_controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let loop_repeats_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let loop_repeats_spin = gtk::SpinButton::with_range(1.0, 32.0, 1.0);
        set_accessible_label(&loop_repeats_spin, "Loop repeats");
        loop_repeats_spin.set_value(1.0);
        loop_repeats_row.append(&gtk::Label::new(Some("Repeats")));
        loop_repeats_row.append(&loop_repeats_spin);
        let loop_scope_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let loop_scope_selected_button = build_labeled_button(
            "Selected Range",
            "edit-select-all-symbolic",
            "icon-tone-cyan",
        );
        let loop_scope_all_button =
            build_labeled_button("All Images", "view-grid-symbolic", "icon-tone-amber");
        loop_scope_selected_button.add_css_class("pill-button");
        loop_scope_all_button.add_css_class("pill-button");
        loop_scope_row.append(&loop_scope_selected_button);
        loop_scope_row.append(&loop_scope_all_button);
        let loop_create_button =
            build_labeled_button("Create Loop", "list-add-symbolic", "icon-tone-green");
        loop_create_button.add_css_class("suggested-action");
        loop_controls.append(&loop_repeats_row);
        loop_controls.append(&loop_scope_row);
        loop_controls.append(&loop_create_button);
        loop_box.append(&loop_controls);
        loop_left.append(&collapsible_section("Loop", &loop_box, true));

        let timeline_selection_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let selection_action_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let duplicate_button =
            build_labeled_button("Duplicate", "edit-copy-symbolic", "icon-tone-cyan");
        let remove_button =
            build_labeled_button("Remove", "edit-delete-symbolic", "icon-tone-coral");
        for button in [&duplicate_button, &remove_button] {
            button.add_css_class("pill-button");
            selection_action_row.append(button);
        }
        let batch_duration_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let batch_duration_spin = gtk::SpinButton::with_range(10.0, 30_000.0, 5.0);
        set_accessible_label(&batch_duration_spin, "Timeline batch duration");
        batch_duration_spin.set_value(100.0);
        let batch_duration_button = build_labeled_button(
            "Set Duration",
            "preferences-system-time-symbolic",
            "icon-tone-amber",
        );
        batch_duration_button.add_css_class("pill-button");
        batch_duration_row.append(&gtk::Label::new(Some("Selected Frames")));
        batch_duration_row.append(&batch_duration_spin);
        batch_duration_row.append(&batch_duration_button);
        timeline_selection_actions.append(&helper_label(
            "Apply common duration and selection actions without crowding the timeline strip itself.",
        ));
        timeline_selection_actions.append(&selection_action_row);
        timeline_selection_actions.append(&batch_duration_row);
        loop_left.append(&collapsible_section(
            "Timeline Actions",
            &timeline_selection_actions,
            true,
        ));

        let timeline_order_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let clipboard_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let copy_button = build_labeled_button("Copy", "edit-copy-symbolic", "icon-tone-cyan");
        let paste_button = build_labeled_button("Paste", "edit-paste-symbolic", "icon-tone-green");
        for button in [&copy_button, &paste_button] {
            button.add_css_class("pill-button");
            clipboard_row.append(button);
        }
        let order_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let move_up_button = build_labeled_button("Move Up", "go-up-symbolic", "icon-tone-amber");
        let move_down_button =
            build_labeled_button("Move Down", "go-down-symbolic", "icon-tone-amber");
        for button in [&move_up_button, &move_down_button] {
            button.add_css_class("pill-button");
            order_row.append(button);
        }
        timeline_order_actions.append(&helper_label(
            "Copy, paste, and reposition frames here while drag reorder remains available in the timeline below.",
        ));
        timeline_order_actions.append(&clipboard_row);
        timeline_order_actions.append(&order_row);
        loop_left.append(&collapsible_section(
            "Clipboard And Order",
            &timeline_order_actions,
            false,
        ));

        // Left Side
        let loop_right = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .build();
        let loop_preview_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let loop_preview_picture = gtk::Picture::new();
        set_accessible_label(&loop_preview_picture, "Loop preview");
        loop_preview_picture.set_size_request(320, 200);
        loop_preview_picture.set_can_shrink(true);
        loop_preview_picture.set_hexpand(true);
        loop_preview_picture.set_vexpand(true);
        install_preview_layout_watch(&loop_preview_picture, WorkflowTab::Loop, sender.clone());
        loop_preview_box.append(&loop_preview_picture);
        loop_right.append(&section("Preview", &loop_preview_box));
        let loop_summary_label =
            summary_label("Choose a loop method to preview the result of the current range.");
        loop_right.append(&collapsible_section(
            "Loop Summary",
            &loop_summary_label,
            true,
        ));
        loop_body.append(&loop_right);
        loop_body.append(&loop_left);
        loop_page.append(&loop_body);
        let loop_scroll = page_scroller(&loop_page);
        page_stack.add_titled(
            &loop_scroll,
            Some(WorkflowTab::Loop.stack_name()),
            "Timeline",
        );

        let export_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let export_body = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .homogeneous(true)
            .build();
        let export_left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .vexpand(true)
            .build();
        let export_preview_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let export_preview_picture = gtk::Picture::new();
        set_accessible_label(&export_preview_picture, "Export preview");
        export_preview_picture.set_size_request(320, 225);
        export_preview_picture.set_can_shrink(true);
        export_preview_picture.set_hexpand(true);
        export_preview_picture.set_vexpand(false);
        export_preview_picture.set_valign(gtk::Align::Start);
        export_preview_picture.add_css_class("preview-image");
        export_preview_picture.add_css_class("export-preview-fixed");
        install_preview_layout_watch(&export_preview_picture, WorkflowTab::Export, sender.clone());
        export_preview_box.append(&export_preview_picture);
        let export_preview_section = section("Preview", &export_preview_box);
        export_preview_section.set_hexpand(true);
        export_left.append(&export_preview_section);

        let export_right = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .build();
        let preset_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let preset_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .homogeneous(true)
            .build();
        let export_preset_fast_button = build_choice_button(
            "Fast Preview",
            "Small file size for quick sharing.",
            "weather-storm-symbolic",
            "icon-tone-cyan",
        );
        let export_preset_balanced_button = build_choice_button(
            "Balanced",
            "Good quality and size for most use cases.",
            "balance-scale-symbolic",
            "icon-tone-green",
        );
        let export_preset_high_button = build_choice_button(
            "High Quality",
            "Better quality with larger files.",
            "starred-symbolic",
            "icon-tone-amber",
        );
        let export_preset_lossless_button = build_choice_button(
            "Lossless",
            "Maximum quality with lossless output.",
            "media-playlist-repeat-symbolic",
            "icon-tone-coral",
        );
        for button in [
            &export_preset_fast_button,
            &export_preset_balanced_button,
            &export_preset_high_button,
            &export_preset_lossless_button,
        ] {
            preset_row.append(button);
        }
        preset_box.append(&preset_row);
        preset_box.append(&helper_label(
            "Balanced is recommended for most sharing and web use.",
        ));
        export_right.append(&section("Export Preset", &preset_box));

        let export_basic_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let output_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .hexpand(true)
            .build();
        output_row.add_css_class("combined-input");
        let output_entry = gtk::Entry::new();
        let suppress_output_entry_change = Rc::new(Cell::new(false));
        set_accessible_label(&output_entry, "Export output path");
        output_entry.set_placeholder_text(Some("/path/to/output.webp"));
        output_entry.set_hexpand(true);
        output_entry.set_halign(gtk::Align::Fill);
        output_entry.add_css_class("control-surface");
        let browse_output_button =
            build_icon_button("folder-open-symbolic", "Choose export output folder");
        let export_size_combo = combo_for_dimension_preset();
        export_size_combo.add_css_class("control-surface");
        let width_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        width_spin.add_css_class("control-surface");
        let height_spin = gtk::SpinButton::with_range(0.0, 8192.0, 1.0);
        height_spin.add_css_class("control-surface");
        let quality_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        quality_scale.set_draw_value(false);
        quality_scale.set_hexpand(true);
        quality_scale.add_css_class("control-surface");
        let quality_spin = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
        quality_spin.set_width_chars(3);
        quality_spin.add_css_class("control-surface");
        set_accessible_label(&export_size_combo, "Export size preset");
        set_accessible_label(&width_spin, "Export width");
        set_accessible_label(&height_spin, "Export height");
        set_accessible_label(&quality_scale, "Export quality slider");
        set_accessible_label(&quality_spin, "Export quality");
        quality_spin.set_value(75.0);
        let lossless_check = gtk::CheckButton::with_label("Lossless");
        let encoder_combo = combo_for_encoder_preset();
        encoder_combo.add_css_class("control-surface");
        let cr_threshold_spin = gtk::SpinButton::with_range(0.0, 1024.0, 1.0);
        cr_threshold_spin.add_css_class("control-surface");
        let cr_size_spin = gtk::SpinButton::with_range(0.0, 256.0, 1.0);
        cr_size_spin.add_css_class("control-surface");
        set_accessible_label(&encoder_combo, "Encoder preset");
        set_accessible_label(&cr_threshold_spin, "Conditional replenishment threshold");
        set_accessible_label(&cr_size_spin, "Conditional replenishment block size");
        cr_size_spin.set_value(16.0);
        let loop_spin = gtk::SpinButton::with_range(0.0, 9999.0, 1.0);
        let loop_count_combo = combo_for_loop_count();
        loop_count_combo.add_css_class("control-surface");
        set_accessible_label(&loop_spin, "Export loop count");
        set_accessible_label(&loop_count_combo, "Export loop count");
        let overwrite_switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .build();
        overwrite_switch.add_css_class("control-surface");
        overwrite_switch.set_active(true);
        set_accessible_label(&overwrite_switch, "Overwrite existing export");
        let fit_mode_combo = combo_for_fit_mode();
        fit_mode_combo.add_css_class("control-surface");
        let raw_args_entry = gtk::Entry::new();
        raw_args_entry.add_css_class("control-surface");
        set_accessible_label(&fit_mode_combo, "Export fit mode");
        set_accessible_label(&raw_args_entry, "Advanced ffmpeg arguments");
        raw_args_entry.set_placeholder_text(Some("-metadata title='Animated export'"));
        let export_button = build_labeled_button(
            "Export Animated WebP",
            "mail-send-symbolic",
            "icon-tone-green",
        );
        let preview_export_button =
            build_labeled_button("Preview Export", "view-reveal-symbolic", "icon-tone-cyan");
        export_button.add_css_class("pill-button");
        output_row.append(&output_entry);
        output_row.append(&browse_output_button);
        let quality_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();
        quality_row.append(&quality_scale);
        quality_row.append(&quality_spin);
        export_basic_box.append(&settings_row(
            "Output File",
            "Where should we save your file?",
            &output_row,
        ));
        export_basic_box.append(&settings_row(
            "Export Size",
            "Choose the size of your export.",
            &export_size_combo,
        ));
        export_basic_box.append(&settings_row(
            "Quality",
            "Adjust quality. Higher means better quality and larger file size.",
            &quality_row,
        ));
        export_basic_box.append(&settings_row(
            "Loop Count",
            "How many times should the animation loop? 0 = infinite loop.",
            &loop_count_combo,
        ));
        export_basic_box.append(&settings_toggle_row(
            "Overwrite Existing",
            "Replace the file if it already exists.",
            &overwrite_switch,
        ));
        export_right.append(&section_without_header(&export_basic_box));

        let export_summary_action_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .homogeneous(true)
            .build();
        export_summary_action_row.add_css_class("export-summary-action-row");
        let export_summary_grid = gtk::Grid::builder()
            .column_spacing(18)
            .row_spacing(10)
            .build();
        export_summary_grid.add_css_class("export-summary-grid");
        let export_summary_format_value = summary_value_label();
        export_summary_format_value.set_label("Animated WebP");
        let export_summary_dimensions_value = summary_value_label();
        let export_summary_frame_count_value = summary_value_label();
        let export_summary_duration_value = summary_value_label();
        let export_summary_loop_count_value = summary_value_label();
        let export_summary_quality_value = summary_value_label();
        let export_summary_estimated_size_value = summary_value_label();
        attach_summary_row(
            &export_summary_grid,
            0,
            "Format",
            &export_summary_format_value,
        );
        attach_summary_row(
            &export_summary_grid,
            1,
            "Dimensions",
            &export_summary_dimensions_value,
        );
        attach_summary_row(
            &export_summary_grid,
            2,
            "Frame Count",
            &export_summary_frame_count_value,
        );
        attach_summary_row(
            &export_summary_grid,
            3,
            "Duration",
            &export_summary_duration_value,
        );
        attach_summary_row(
            &export_summary_grid,
            4,
            "Loop Count",
            &export_summary_loop_count_value,
        );
        attach_summary_row(
            &export_summary_grid,
            5,
            "Quality",
            &export_summary_quality_value,
        );
        attach_summary_row(
            &export_summary_grid,
            6,
            "Estimated File Size",
            &export_summary_estimated_size_value,
        );
        let export_summary_section = section("Export Summary", &export_summary_grid);
        export_summary_section.set_hexpand(true);

        let export_action_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .build();
        let export_action_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        let export_action_icon = gtk::Image::from_icon_name("object-select-symbolic");
        export_action_icon.add_css_class("export-action-icon");
        let export_action_title_label = gtk::Label::new(Some("Add Frames"));
        export_action_title_label.set_halign(gtk::Align::Center);
        export_action_title_label.set_justify(gtk::Justification::Center);
        export_action_title_label.add_css_class("export-action-title");
        let export_action_detail_label =
            gtk::Label::new(Some("Import frames and choose an output file to export."));
        export_action_detail_label.set_wrap(true);
        export_action_detail_label.set_halign(gtk::Align::Center);
        export_action_detail_label.set_justify(gtk::Justification::Center);
        export_action_detail_label.add_css_class("export-action-detail");
        let export_action_buttons = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        export_action_buttons.set_hexpand(true);
        preview_export_button.add_css_class("pill-button");
        export_button.add_css_class("suggested-action");
        preview_export_button.set_hexpand(true);
        preview_export_button.set_halign(gtk::Align::Fill);
        export_button.set_hexpand(true);
        export_button.set_halign(gtk::Align::Fill);
        export_action_header.append(&export_action_icon);
        export_action_header.append(&export_action_title_label);
        export_action_box.append(&export_action_header);
        export_action_box.append(&export_action_detail_label);
        export_action_buttons.append(&export_button);
        export_action_buttons.append(&preview_export_button);
        export_action_box.append(&export_action_buttons);
        let export_action_section = section_without_header(&export_action_box);
        export_action_section.set_hexpand(true);
        export_summary_action_row.append(&export_summary_section);
        export_summary_action_row.append(&export_action_section);
        export_left.append(&export_summary_action_row);

        let export_advanced_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        let export_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        attach_labeled_spin(&export_grid, "Width", &width_spin, 0, 0);
        attach_labeled_spin(&export_grid, "Height", &height_spin, 1, 0);
        export_grid.attach(&gtk::Label::new(Some("Encoder Preset")), 0, 2, 1, 1);
        export_grid.attach(&encoder_combo, 1, 2, 1, 1);
        attach_labeled_spin(&export_grid, "CR Threshold", &cr_threshold_spin, 0, 3);
        attach_labeled_spin(&export_grid, "CR Size", &cr_size_spin, 1, 3);
        export_grid.attach(&gtk::Label::new(Some("Export Fit Mode")), 0, 5, 1, 1);
        export_grid.attach(&fit_mode_combo, 1, 5, 1, 1);
        export_grid.attach(&gtk::Label::new(Some("Advanced ffmpeg args")), 0, 6, 1, 1);
        export_grid.attach(&raw_args_entry, 0, 7, 2, 1);
        let command_preview_label = gtk::Label::new(None);
        command_preview_label.set_xalign(0.0);
        command_preview_label.set_wrap(true);
        command_preview_label.set_selectable(true);
        export_advanced_box.append(&helper_label(
            "Show advanced export settings and encoder options.",
        ));
        export_advanced_box.append(&lossless_check);
        export_advanced_box.append(&export_grid);
        export_advanced_box.append(&section("Effective Command", &command_preview_label));
        export_body.append(&export_left);
        export_right.append(&section("Advanced Options", &export_advanced_box));
        export_body.append(&export_right);
        export_page.append(&export_body);
        let export_scroll = page_scroller(&export_page);
        page_stack.add_titled(
            &export_scroll,
            Some(WorkflowTab::Export.stack_name()),
            "Export",
        );

        let diagnostics_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let diagnostics_page_button =
            build_labeled_button("Run Diagnostics", "system-run-symbolic", "icon-tone-coral");
        diagnostics_page_button.add_css_class("suggested-action");
        diagnostics_page.append(&diagnostics_page_button);
        let diagnostics_overview_label =
            summary_label("Ready to check ffmpeg and ffprobe availability.");
        diagnostics_page.append(&section("Health", &diagnostics_overview_label));
        let diagnostics_label = gtk::Label::new(None);
        diagnostics_label.set_xalign(0.0);
        diagnostics_label.set_selectable(true);
        diagnostics_label.set_wrap(true);
        let diagnostics_details_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        diagnostics_details_box.append(&helper_label(
            "Advanced mode shows the raw command availability details and version strings.",
        ));
        diagnostics_details_box.append(&diagnostics_label);
        diagnostics_page.append(&section("Details", &diagnostics_details_box));
        let diagnostics_scroll = page_scroller(&diagnostics_page);
        page_stack.add_titled(
            &diagnostics_scroll,
            Some(WorkflowTab::Diagnostics.stack_name()),
            "Diagnostics",
        );

        let timeline_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        timeline_box.add_css_class("timeline-shell");
        let timeline_toolbar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
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
        timeline_toolbar.append(&transport_box);
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
        timeline_box.append(&timeline_toolbar);
        timeline_box.append(&frame_scroll);
        root.append(&timeline_box);

        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        footer.add_css_class("status-shell");
        let footer_frames_label = gtk::Label::new(Some("0 images"));
        footer_frames_label.set_xalign(0.0);
        let footer_duration_label = gtk::Label::new(Some("0.0 s total"));
        footer_duration_label.set_xalign(0.0);
        let footer_progress_bar = gtk::ProgressBar::builder()
            .hexpand(true)
            .show_text(true)
            .visible(false)
            .valign(gtk::Align::Center)
            .build();
        let status_label = gtk::Label::new(None);
        status_label.set_xalign(0.0);
        status_label.set_hexpand(true);
        status_label.set_halign(gtk::Align::Fill);
        status_label.set_wrap(false);
        status_label.set_single_line_mode(true);
        status_label.set_ellipsize(pango::EllipsizeMode::End);
        status_label.add_css_class("dim-label");
        let footer_state_label = gtk::Label::new(Some("Ready"));
        footer_state_label.set_xalign(1.0);
        footer_state_label.add_css_class("status-pill");
        footer.append(&footer_frames_label);
        footer.append(&footer_duration_label);
        footer.append(&footer_progress_bar);
        footer.append(&status_label);
        footer.append(&footer_state_label);
        root.append(&footer);

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
        organize_import_button.connect_clicked(clone!(
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
        organize_open_button.connect_clicked(clone!(
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
        organize_save_button.connect_clicked(clone!(
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
        advanced_switch.connect_state_set(clone!(
            #[strong]
            sender,
            move |_, state| {
                sender.input(AppMsg::SetAdvancedMode(state));
                false.into()
            }
        ));
        tab_organize_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetActiveTab(WorkflowTab::Organize))
        ));
        tab_edit_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetActiveTab(WorkflowTab::Edit))
        ));
        tab_loop_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetActiveTab(WorkflowTab::Loop))
        ));
        tab_export_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetActiveTab(WorkflowTab::Export))
        ));
        run_diagnostics_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| {
                sender.input(AppMsg::SetActiveTab(WorkflowTab::Diagnostics));
                sender.input(AppMsg::RunDiagnostics);
            }
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
        flip_horizontal_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ToggleSelectionFlip { horizontal: true })
        ));
        flip_vertical_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ToggleSelectionFlip { horizontal: false })
        ));
        crop_square_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropPreset(CropPreset::Square))
        ));
        crop_widescreen_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropPreset(CropPreset::Landscape16x9))
        ));
        crop_story_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropPreset(CropPreset::Portrait9x16))
        ));
        crop_start_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropAnchor(CropAnchor::Start))
        ));
        crop_center_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropAnchor(CropAnchor::Center))
        ));
        crop_end_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetCropAnchor(CropAnchor::End))
        ));
        apply_crop_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ApplyQuickCrop)
        ));
        clear_crop_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ClearQuickCrop)
        ));
        quick_resize_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| {
                sender.input(AppMsg::SetQuickResizePreset(dimension_preset_from_combo(
                    combo,
                )));
            }
        ));
        quick_resize_mode_stack.connect_notify_local(
            Some("visible-child-name"),
            clone!(
                #[strong]
                sender,
                move |stack, _| {
                    let mode = match stack.visible_child_name().as_deref() {
                        Some("custom") => QuickResizeMode::Custom,
                        _ => QuickResizeMode::Multiplier,
                    };
                    sender.input(AppMsg::SetQuickResizeMode(mode));
                }
            ),
        );
        multiplier_down_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            quick_resize_multiplier_spin,
            move |_| {
                sender.input(AppMsg::SetQuickResizeMultiplier(
                    quick_resize_multiplier_spin.value() - 1.0,
                ));
            }
        ));
        quick_resize_multiplier_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetQuickResizeMultiplier(spin.value()))
        ));
        multiplier_up_button.connect_clicked(clone!(
            #[strong]
            sender,
            #[strong]
            quick_resize_multiplier_spin,
            move |_| {
                sender.input(AppMsg::SetQuickResizeMultiplier(
                    quick_resize_multiplier_spin.value() + 1.0,
                ));
            }
        ));
        quick_resize_custom_width_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetQuickResizeCustomWidth(spin.value() as u32))
        ));
        quick_resize_custom_height_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetQuickResizeCustomHeight(spin.value() as u32))
        ));
        loop_duplicate_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetLoopMethod(LoopMethod::Duplicate))
        ));
        loop_reverse_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetLoopMethod(LoopMethod::Reverse))
        ));
        loop_ping_pong_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetLoopMethod(LoopMethod::PingPong))
        ));
        loop_repeats_spin.connect_value_changed(clone!(
            #[strong]
            sender,
            move |spin| sender.input(AppMsg::SetLoopRepeats(spin.value() as u32))
        ));
        loop_scope_selected_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetLoopScope(LoopScope::Selected))
        ));
        loop_scope_all_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetLoopScope(LoopScope::AllFrames))
        ));
        loop_create_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::CreateLoop)
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
        quick_apply_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ApplyQuickResize)
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
        export_preset_fast_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetExportPreset(ExportPreset::FastPreview))
        ));
        export_preset_balanced_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetExportPreset(ExportPreset::Balanced))
        ));
        export_preset_high_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetExportPreset(ExportPreset::HighQuality))
        ));
        export_preset_lossless_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::SetExportPreset(ExportPreset::Lossless))
        ));
        export_size_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| sender.input(AppMsg::SetExportSizePreset(dimension_preset_from_combo(
                combo
            )))
        ));
        output_entry.connect_changed(clone!(
            #[strong]
            sender,
            #[strong]
            suppress_output_entry_change,
            move |entry| {
                if !suppress_output_entry_change.get() {
                    sender.input(AppMsg::SetOutputPath(entry.text().to_string()));
                }
            }
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
        quality_scale.connect_value_changed(clone!(
            #[strong]
            sender,
            move |scale| sender.input(AppMsg::SetQuality(scale.value() as f32))
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
        loop_count_combo.connect_changed(clone!(
            #[strong]
            sender,
            move |combo| sender.input(AppMsg::SetLoopCount(loop_count_from_combo(combo)))
        ));
        overwrite_switch.connect_state_set(clone!(
            #[strong]
            sender,
            move |_, state| {
                sender.input(AppMsg::SetOverwrite(state));
                false.into()
            }
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
        preview_export_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::PreviewExport)
        ));
        export_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::ExportNow)
        ));
        diagnostics_page_button.connect_clicked(clone!(
            #[strong]
            sender,
            move |_| sender.input(AppMsg::RunDiagnostics)
        ));

        install_import_drop_targets(&root, sender.clone());
        install_window_layout_watch(&window, sender.clone());
        install_autosave_on_close(&window, sender.clone());

        let widgets = AppWidgets {
            workspace_box,
            content_stack: page_stack,
            tab_organize_button,
            tab_edit_button,
            tab_loop_button,
            tab_export_button,
            advanced_switch,
            preview_panel,
            timeline_toolbar,
            loop_body,
            loop_settings_column: loop_left,
            export_body,
            export_settings_column: export_right,
            timeline_strip,
            nav_first_button,
            nav_prev_button,
            nav_play_button,
            nav_next_button,
            nav_last_button,
            diagnostics_label,
            diagnostics_overview_label,
            diagnostics_details_box,
            status_label,
            footer_frames_label,
            footer_duration_label,
            footer_progress_bar,
            footer_state_label,
            preview_picture,
            preview_meta,
            loop_preview_picture,
            export_preview_picture,
            crop_summary_label,
            crop_square_button,
            crop_widescreen_button,
            crop_story_button,
            crop_start_button,
            crop_center_button,
            crop_end_button,
            apply_crop_button,
            clear_crop_button,
            output_entry,
            suppress_output_entry_change,
            quick_resize_combo,
            quick_resize_custom_panel,
            quick_resize_mode_stack,
            quick_resize_multiplier_spin,
            quick_resize_custom_width_spin,
            quick_resize_custom_height_spin,
            export_size_combo,
            loop_source_label,
            loop_summary_label,
            loop_repeats_spin,
            loop_create_button,
            loop_duplicate_button,
            loop_reverse_button,
            loop_ping_pong_button,
            loop_scope_selected_button,
            loop_scope_all_button,
            export_preset_fast_button,
            export_preset_balanced_button,
            export_preset_high_button,
            export_preset_lossless_button,
            export_summary_dimensions_value,
            export_summary_frame_count_value,
            export_summary_duration_value,
            export_summary_loop_count_value,
            export_summary_quality_value,
            export_summary_estimated_size_value,
            export_advanced_box,
            edit_advanced_box,
            preview_export_button,
            export_button,
            export_action_icon,
            export_action_title_label,
            export_action_detail_label,
            quality_scale,
            quality_spin,
            width_spin,
            height_spin,
            lossless_check,
            encoder_combo,
            cr_threshold_spin,
            cr_size_spin,
            loop_spin,
            loop_count_combo,
            overwrite_switch,
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
            timeline_render_signature: 0,
            preview_picture_path: None,
            loop_preview_picture_path: None,
            export_preview_picture_path: None,
        };

        if !restored_frame_ids.is_empty() {
            model.refresh_frame_jobs(restored_frame_ids, &sender);
            model.queue_preview_for_primary_selection(&sender);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::ImportPaths(paths) => {
                let valid = filter_image_paths(paths);
                if valid.is_empty() {
                    self.set_status(
                        FooterStatusScope::Organize,
                        "No supported image files were provided.",
                    );
                } else if self.timeline.is_empty() {
                    self.import_paths(valid, ImportMode::Append, &sender);
                } else {
                    choose_import_mode(root, sender.clone(), valid);
                }
            }
            AppMsg::ImportPathsWithMode { paths, mode } => {
                self.import_paths(paths, mode, &sender);
            }
            AppMsg::WindowLayoutChanged(width) => {
                self.layout_mode = layout_mode_for_width(width);
            }
            AppMsg::SetActiveTab(tab) => {
                self.active_tab = tab;
                self.preview_rendered_size = None;
            }
            AppMsg::SetAdvancedMode(enabled) => {
                let status = if let Err(err) = self.set_advanced_mode(enabled) {
                    err
                } else if enabled {
                    "Advanced controls are visible.".to_string()
                } else {
                    "Advanced controls are hidden.".to_string()
                };
                self.set_status(FooterStatusScope::from(self.active_tab), status);
            }
            AppMsg::PreviewLayoutChanged { tab, size } => {
                if tab == self.active_tab && size != self.preview_target_size {
                    self.preview_target_size = size;
                    if self.primary_selected_frame().is_some_and(|frame| {
                        should_refresh_preview(
                            self.preview_rendered_size,
                            self.preview_render_size_for_frame(frame),
                        )
                    }) {
                        self.queue_preview_for_primary_selection(&sender);
                    }
                }
            }
            AppMsg::RunDiagnostics => {
                self.diagnostics = collect_diagnostics();
                self.set_status(FooterStatusScope::Diagnostics, "Diagnostics refreshed.");
            }
            AppMsg::PreviewExport => {
                self.invalidate_export_preview();
                self.queue_export_preview_for_primary_selection(&sender);
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
                        self.set_status(FooterStatusScope::Global, "Playback finished.");
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
                self.sync_quick_resize_state_from_selection();
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
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Applied {} ms to selected frames.", duration.max(10)),
                );
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
                    self.set_status(FooterStatusScope::Organize, "Reordered frame.");
                }
            }
            AppMsg::DuplicateSelection => {
                self.stop_playback(None);
                let inserted = self.timeline.duplicate_selected(&self.selection);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Duplicated {} frame(s).", inserted.len()),
                );
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
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Copied {} frame(s).", self.clipboard.len()),
                );
            }
            AppMsg::PasteClipboard => {
                self.stop_playback(None);
                let inserted = self
                    .timeline
                    .paste_after_selection(&self.selection, &self.clipboard);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Pasted {} frame(s).", inserted.len()),
                );
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
                self.invalidate_export_preview();
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Removed {removed} frame(s)."),
                );
            }
            AppMsg::AppendDuplicateLoop => {
                self.stop_playback(None);
                let inserted = self.timeline.append_duplicate_loop(&self.selection);
                self.selection = inserted.iter().copied().collect();
                self.selection_anchor_id = inserted.first().copied();
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Appended duplicate loop with {} frame(s).", inserted.len()),
                );
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
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Loop,
                    format!("Appended reverse loop with {} frame(s).", inserted.len()),
                );
                self.refresh_frame_jobs(inserted, &sender);
                self.queue_preview_for_primary_selection(&sender);
            }
            AppMsg::SetLoopMethod(method) => {
                self.loop_method = method;
            }
            AppMsg::SetLoopRepeats(value) => {
                self.loop_repeats = value.max(1);
            }
            AppMsg::SetLoopScope(scope) => {
                self.loop_scope = scope;
            }
            AppMsg::CreateLoop => {
                self.stop_playback(None);
                let selection = self.loop_selection();
                if self.loop_scope == LoopScope::Selected && selection.is_empty() {
                    self.set_status(
                        FooterStatusScope::Loop,
                        "Select a range in the timeline first.",
                    );
                } else {
                    let source = self.current_loop_source();
                    if source.is_empty() {
                        self.set_status(
                            FooterStatusScope::Loop,
                            "No frames available for loop creation.",
                        );
                    } else {
                        let inserted = self.timeline.append_copies(&source, self.loop_repeats);
                        self.selection = inserted.iter().copied().collect();
                        self.selection_anchor_id = inserted.first().copied();
                        self.sync_quick_resize_state_from_selection();
                        self.set_status(
                            FooterStatusScope::Loop,
                            format!(
                                "Created a {} loop with {} new frame(s).",
                                self.loop_method.title().to_ascii_lowercase(),
                                inserted.len()
                            ),
                        );
                        self.refresh_frame_jobs(inserted, &sender);
                        self.queue_preview_for_primary_selection(&sender);
                    }
                }
            }
            AppMsg::SetCropPreset(preset) => {
                self.crop_preset = preset;
            }
            AppMsg::SetCropAnchor(anchor) => {
                self.crop_anchor = anchor;
            }
            AppMsg::ApplyQuickCrop => {
                self.apply_quick_crop(&sender);
            }
            AppMsg::ClearQuickCrop => {
                self.clear_quick_crop(&sender);
            }
            AppMsg::SetQuickResizePreset(preset) => {
                self.quick_resize_preset = preset;
                if preset == DimensionPreset::Custom {
                    if self.quick_resize_custom_width == 0 || self.quick_resize_custom_height == 0 {
                        self.seed_quick_resize_custom_dimensions();
                    }
                }
            }
            AppMsg::SetQuickResizeMode(mode) => {
                self.quick_resize_mode = mode;
            }
            AppMsg::SetQuickResizeMultiplier(value) => {
                self.quick_resize_multiplier = value.clamp(0.1, 64.0);
            }
            AppMsg::SetQuickResizeCustomWidth(width) => {
                let height = self
                    .linked_resize_height_for_width(width)
                    .unwrap_or(self.quick_resize_custom_height);
                self.quick_resize_custom_width = width;
                self.quick_resize_custom_height = height;
            }
            AppMsg::SetQuickResizeCustomHeight(height) => {
                let width = self
                    .linked_resize_width_for_height(height)
                    .unwrap_or(self.quick_resize_custom_width);
                self.quick_resize_custom_height = height;
                self.quick_resize_custom_width = width;
            }
            AppMsg::ApplyQuickResize => {
                if self.timeline.is_empty() {
                    self.set_status(FooterStatusScope::Edit, "Import frames before resizing.");
                } else if let Some(target) = self.quick_resize_target() {
                    self.apply_to_all_frames(|frame| {
                        frame.transform_spec.resize = Some(target);
                    });
                    self.sync_quick_resize_state_from_selection();
                    self.set_status(
                        FooterStatusScope::Edit,
                        format!(
                            "Applied resize target {} x {} to all frames.",
                            target.width, target.height
                        ),
                    );
                    self.refresh_all_frame_jobs(&sender);
                } else if self.quick_resize_preset == DimensionPreset::Original {
                    self.apply_to_all_frames(|frame| {
                        frame.transform_spec.resize = None;
                    });
                    self.sync_quick_resize_state_from_selection();
                    self.set_status(
                        FooterStatusScope::Edit,
                        "Reset all frames to their original size.",
                    );
                    self.refresh_all_frame_jobs(&sender);
                } else {
                    self.set_status(
                        FooterStatusScope::Edit,
                        "Enter a valid multiplier or linked custom size before resizing all frames.",
                    );
                }
            }
            AppMsg::RotateSelection(delta) => {
                self.apply_to_selection(|frame| {
                    frame.transform_spec.rotate_quarter_turns += delta;
                });
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Edit,
                    "Updated rotation for selected frames.",
                );
                self.refresh_selection_jobs(&sender);
            }
            AppMsg::ToggleSelectionFlip { horizontal } => {
                self.apply_to_selection(|frame| {
                    if horizontal {
                        frame.transform_spec.flip_horizontal =
                            !frame.transform_spec.flip_horizontal;
                    } else {
                        frame.transform_spec.flip_vertical = !frame.transform_spec.flip_vertical;
                    }
                });
                self.sync_quick_resize_state_from_selection();
                let status = if horizontal {
                    "Updated horizontal flip for selected frames.".to_string()
                } else {
                    "Updated vertical flip for selected frames.".to_string()
                };
                self.set_status(FooterStatusScope::Edit, status);
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
                self.sync_quick_resize_state_from_selection();
                self.set_status(
                    FooterStatusScope::Edit,
                    "Applied edit values to selected frames.",
                );
                self.refresh_selection_jobs(&sender);
            }
            AppMsg::SetExportPreset(preset) => {
                self.export_profile.apply_preset(preset);
                self.invalidate_export_preview();
            }
            AppMsg::SetExportSizePreset(preset) => match preset {
                DimensionPreset::Original => {
                    self.export_profile.output_width = None;
                    self.export_profile.output_height = None;
                    self.invalidate_export_preview();
                }
                DimensionPreset::Hd1080 => {
                    self.export_profile.output_width = Some(1920);
                    self.export_profile.output_height = Some(1080);
                    self.invalidate_export_preview();
                }
                DimensionPreset::Hd720 => {
                    self.export_profile.output_width = Some(1280);
                    self.export_profile.output_height = Some(720);
                    self.invalidate_export_preview();
                }
                DimensionPreset::Custom => {
                    let (width, height) = self
                        .preferred_export_custom_dimensions()
                        .unwrap_or((1280, 720));
                    self.export_profile.output_width = Some(width);
                    self.export_profile.output_height = Some(height);
                    self.invalidate_export_preview();
                    let _ = self.set_advanced_mode(true);
                }
            },
            AppMsg::SetOutputPath(path) => {
                self.last_output_path =
                    (!path.trim().is_empty()).then_some(PathBuf::from(path.trim()));
            }
            AppMsg::SetOutputWidth(width) => {
                self.export_profile.output_width = if width == 0 { None } else { Some(width) };
                self.invalidate_export_preview();
            }
            AppMsg::SetOutputHeight(height) => {
                self.export_profile.output_height = if height == 0 { None } else { Some(height) };
                self.invalidate_export_preview();
            }
            AppMsg::SetQuality(quality) => self.export_profile.quality = quality.clamp(0.0, 100.0),
            AppMsg::SetLossless(lossless) => self.export_profile.lossless = lossless,
            AppMsg::SetEncoderPreset(preset) => self.export_profile.encoder_preset = preset,
            AppMsg::SetCrThreshold(value) => self.export_profile.cr_threshold = value,
            AppMsg::SetCrSize(value) => self.export_profile.cr_size = value,
            AppMsg::SetLoopCount(value) => self.export_profile.loop_count = value,
            AppMsg::SetOverwrite(value) => self.export_profile.overwrite = value,
            AppMsg::SetExportFitMode(value) => {
                self.export_profile.fit_mode = value;
                self.invalidate_export_preview();
            }
            AppMsg::SetRawArgs(args) => self.export_profile.raw_args = args,
            AppMsg::SaveProject(path) => {
                let document = self.project_document();
                match save_project(&path, &document) {
                    Ok(_) => self.set_status(
                        FooterStatusScope::Organize,
                        format!("Saved project to {}", path.display()),
                    ),
                    Err(err) => self.set_status(
                        FooterStatusScope::Organize,
                        format!("Failed to save project: {err}"),
                    ),
                }
            }
            AppMsg::OpenProject(path) => {
                self.stop_playback(None);
                match load_project(&path) {
                    Ok(document) => {
                        let frame_ids = self.apply_project_document(document);
                        self.set_status(
                            FooterStatusScope::Organize,
                            format!(
                                "Loaded project {}. Refreshing thumbnails...",
                                path.display()
                            ),
                        );
                        self.refresh_frame_jobs(frame_ids, &sender);
                        self.queue_preview_for_primary_selection(&sender);
                    }
                    Err(err) => self.set_status(
                        FooterStatusScope::Organize,
                        format!("Failed to load project: {err}"),
                    ),
                }
            }
            AppMsg::ChooseOutputPath(path) => {
                self.last_output_path = Some(normalized_output_path(&path));
                self.set_status(FooterStatusScope::Export, "Export output path updated.");
            }
            AppMsg::ExportNow => {
                let Some(output_path) = self.last_output_path.clone() else {
                    self.set_status(FooterStatusScope::Export, "Choose an output path first.");
                    self.recompute_command_preview();
                    return;
                };
                let output_path = normalized_output_path(&output_path);
                self.last_output_path = Some(output_path.clone());
                if self.export_in_progress {
                    self.set_status(FooterStatusScope::Export, "Export already running.");
                    self.recompute_command_preview();
                    return;
                }
                EXPORT_LAYOUT_WATCH_SUSPENDED.store(true, Ordering::Relaxed);
                self.export_in_progress = true;
                self.set_status(
                    FooterStatusScope::Export,
                    format!("Exporting to {} ...", output_path.display()),
                );
                let initial_progress = ExportProgressState {
                    fraction: 0.0,
                    detail: "Preparing export...".to_string(),
                };
                self.export_progress = Some(initial_progress.clone());
                let frames = self.timeline.frames().to_vec();
                let profile = self.export_profile.clone();
                let export_worker_state = Arc::new(Mutex::new(ExportWorkerState {
                    version: 1,
                    progress: Some(initial_progress),
                    result: None,
                }));
                self.export_worker_state = Some(export_worker_state.clone());
                if let Some(active) = self.export_poll_active.take() {
                    active.set(false);
                }
                let export_poll_active = Rc::new(Cell::new(true));
                self.export_poll_active = Some(export_poll_active.clone());
                install_export_poll(
                    sender.clone(),
                    export_poll_active,
                    export_worker_state.clone(),
                );
                let export_start_request = ExportStartRequest {
                    frames,
                    profile,
                    output_path,
                    export_worker_state,
                };
                gtk::glib::timeout_add_local_once(Duration::from_millis(0), move || {
                    sender.input(AppMsg::StartExportWorker(export_start_request));
                });
            }
            AppMsg::StartExportWorker(export_start_request) => {
                let ExportStartRequest {
                    frames,
                    profile,
                    output_path,
                    export_worker_state,
                } = export_start_request;

                let thread_name = "export-worker".to_string();
                match thread::Builder::new()
                    .name(thread_name.clone())
                    .spawn(move || {
                        let result = export_animation_with_progress(
                            &frames,
                            &profile,
                            &output_path,
                            |progress| {
                                if let Ok(mut state) = export_worker_state.lock() {
                                    state.progress = Some(ExportProgressState {
                                        fraction: progress.fraction.clamp(0.0, 1.0),
                                        detail: progress.detail,
                                    });
                                    state.version = state.version.saturating_add(1);
                                }
                            },
                        )
                        .map_err(|err| err.to_string());

                        if let Ok(mut state) = export_worker_state.lock() {
                            state.result = Some(result);
                            state.version = state.version.saturating_add(1);
                        }
                    }) {
                    Ok(_) => {}
                    Err(err) => {
                        self.export_in_progress = false;
                        self.export_progress = None;
                        self.export_worker_state = None;
                        if let Some(active) = self.export_poll_active.take() {
                            active.set(false);
                        }
                        self.set_status(
                            FooterStatusScope::Export,
                            format!("Failed to start export worker: {err}"),
                        );
                    }
                }
            }
            AppMsg::PollExportState => {
                self.poll_export_state(&sender);
            }
            AppMsg::FinalizeExportUi => {
                let sender = sender.clone();
                gtk::glib::timeout_add_local_once(Duration::from_millis(0), move || {
                    sender.input(AppMsg::ResumePreviewLayoutWatch);
                });
            }
            AppMsg::ResumePreviewLayoutWatch => {
                EXPORT_LAYOUT_WATCH_SUSPENDED.store(false, Ordering::Relaxed);
                let sender = sender.clone();
                gtk::glib::timeout_add_local_once(Duration::from_millis(0), move || {
                    sender.input(AppMsg::CompleteExportUiRestore);
                });
            }
            AppMsg::CompleteExportUiRestore => {
                self.export_completion_pending = false;
            }
            AppMsg::CloseWithAutosave {
                window,
                close_allowed,
            } => {
                if let Err(err) = self.save_autosave_project() {
                    self.set_status(
                        FooterStatusScope::Global,
                        format!("Autosave failed during close: {err}"),
                    );
                }
                close_allowed.set(true);
                window.close();
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
                    self.set_status(
                        FooterStatusScope::Global,
                        format!("Thumbnail failed for frame {frame_id}: {error}"),
                    );
                } else if self.thumbnails_pending == 0 {
                    self.set_status(FooterStatusScope::Global, "Timeline thumbnails ready.");
                }
            }
            CommandMsg::PreviewReady {
                frame_id,
                render_size,
                preview_path,
                error,
            } => {
                if preview_result_is_usable(self.primary_selected_id(), frame_id) {
                    self.preview_frame_id = Some(frame_id);
                    if let Some(preview_path) = usable_preview_path(preview_path) {
                        self.preview_rendered_size = Some(render_size);
                        self.preview_path = Some(preview_path);
                        if self.primary_selected_frame().is_some_and(|frame| {
                            should_refresh_preview(
                                self.preview_rendered_size,
                                self.preview_render_size_for_frame(frame),
                            )
                        }) {
                            self.queue_preview_for_primary_selection(&sender);
                        }
                    }
                }
                if let Some(error) = error {
                    self.set_status(
                        FooterStatusScope::Global,
                        format!("Preview failed for frame {frame_id}: {error}"),
                    );
                }
            }
            CommandMsg::ExportPreviewReady {
                frame_id,
                generation,
                render_size,
                preview_path,
                error,
            } => {
                if generation == self.export_preview_generation
                    && self.primary_selected_id() == Some(frame_id)
                    && let Some(preview_path) = usable_preview_path(preview_path)
                {
                    self.export_preview_path = Some(preview_path);
                    self.export_preview_rendered_size = Some(render_size);
                }
                if let Some(error) = error {
                    self.set_status(
                        FooterStatusScope::Export,
                        format!("Export preview failed for frame {frame_id}: {error}"),
                    );
                } else if self.primary_selected_id() == Some(frame_id) {
                    self.set_status(
                        FooterStatusScope::Export,
                        "Export preview refreshed with the current export settings.",
                    );
                }
            }
        }

        self.recompute_command_preview();
        let _ = sender;
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        let frame_count = self.timeline.frames().len();
        let export_frame_count = self
            .timeline
            .frames()
            .iter()
            .filter(|frame| frame.enabled)
            .count();
        let has_frames = frame_count != 0;
        let readiness_text = self.readiness_text();

        if self.export_in_progress || self.export_completion_pending {
            widgets.status_label.set_visible(false);
            widgets.footer_progress_bar.set_visible(true);
            widgets.footer_frames_label.set_label(&format!(
                "{} image{}",
                frame_count,
                if frame_count == 1 { "" } else { "s" }
            ));
            widgets
                .footer_duration_label
                .set_label(&format_duration_ms(self.total_duration_ms()));
            if self.export_in_progress {
                if let Some(progress) = &self.export_progress {
                    widgets
                        .footer_progress_bar
                        .set_fraction(progress.fraction.clamp(0.0, 1.0));
                    widgets.footer_progress_bar.set_text(Some(&progress.detail));
                } else {
                    widgets.footer_progress_bar.set_fraction(0.0);
                    widgets
                        .footer_progress_bar
                        .set_text(Some("Preparing export..."));
                }
            } else {
                widgets.footer_progress_bar.set_fraction(1.0);
                widgets
                    .footer_progress_bar
                    .set_text(Some("Finishing export..."));
            }
            widgets.footer_state_label.set_label(&readiness_text);
            widgets
                .preview_export_button
                .set_sensitive(!self.export_completion_pending && has_frames);
            widgets.export_button.set_sensitive(
                !self.export_completion_pending && has_frames && self.last_output_path.is_some(),
            );
            return;
        }

        let compact = self.layout_mode == LayoutMode::Compact;
        set_box_orientation_if_needed(
            &widgets.workspace_box,
            if compact {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            },
        );
        set_box_orientation_if_needed(
            &widgets.loop_body,
            if compact {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            },
        );
        set_box_orientation_if_needed(
            &widgets.export_body,
            if compact {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            },
        );
        set_box_orientation_if_needed(&widgets.timeline_toolbar, gtk::Orientation::Horizontal);
        set_width_request_if_needed(&widgets.loop_settings_column, -1);
        set_width_request_if_needed(&widgets.export_settings_column, -1);
        set_size_request_if_needed(
            &widgets.preview_picture,
            if compact { 260 } else { 320 },
            if compact { 160 } else { 200 },
        );
        set_size_request_if_needed(
            &widgets.loop_preview_picture,
            if compact { 260 } else { 320 },
            if compact { 160 } else { 200 },
        );
        set_size_request_if_needed(
            &widgets.export_preview_picture,
            if compact { 260 } else { 320 },
            if compact { 140 } else { 225 },
        );

        set_stack_visible_child_name_if_needed(
            &widgets.content_stack,
            self.active_tab.stack_name(),
        );
        set_switch_if_needed(&widgets.advanced_switch, self.advanced_mode);
        set_visible_if_needed(
            &widgets.preview_panel,
            matches!(self.active_tab, WorkflowTab::Organize | WorkflowTab::Edit),
        );
        sync_workspace_split_widths(
            &widgets.workspace_box,
            &widgets.preview_panel,
            &widgets.content_stack,
        );
        set_visible_if_needed(&widgets.edit_advanced_box, self.advanced_mode);
        set_visible_if_needed(&widgets.export_advanced_box, self.advanced_mode);
        set_visible_if_needed(&widgets.diagnostics_details_box, self.advanced_mode);

        set_widget_css_class(
            &widgets.tab_organize_button,
            "workflow-tab-active",
            self.active_tab == WorkflowTab::Organize,
        );
        set_widget_css_class(
            &widgets.tab_edit_button,
            "workflow-tab-active",
            self.active_tab == WorkflowTab::Edit,
        );
        set_widget_css_class(
            &widgets.tab_loop_button,
            "workflow-tab-active",
            self.active_tab == WorkflowTab::Loop,
        );
        set_widget_css_class(
            &widgets.tab_export_button,
            "workflow-tab-active",
            self.active_tab == WorkflowTab::Export,
        );
        widgets.status_label.set_visible(true);
        widgets.footer_progress_bar.set_visible(false);
        widgets.status_label.set_label(&self.footer_context_text());
        widgets.footer_frames_label.set_label(&format!(
            "{} image{}",
            frame_count,
            if frame_count == 1 { "" } else { "s" }
        ));
        widgets
            .footer_duration_label
            .set_label(&format_duration_ms(self.total_duration_ms()));
        widgets.footer_progress_bar.set_fraction(0.0);
        widgets.footer_progress_bar.set_text(None);
        widgets.footer_state_label.set_label(&readiness_text);
        widgets
            .diagnostics_overview_label
            .set_label(&self.diagnostics_overview_text());
        widgets
            .diagnostics_label
            .set_label(&self.diagnostics.summary());
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
        set_picture_from_path_if_needed(
            &widgets.preview_picture,
            self.preview_path.as_deref(),
            &mut widgets.preview_picture_path,
        );
        set_picture_from_path_if_needed(
            &widgets.loop_preview_picture,
            self.preview_path.as_deref(),
            &mut widgets.loop_preview_picture_path,
        );
        let export_preview_path = self.export_preview_path.as_ref().filter(|_| {
            self.primary_selected_frame().is_some_and(|frame| {
                !should_refresh_preview(
                    self.export_preview_rendered_size,
                    self.export_preview_render_size_for_frame(frame),
                )
            })
        });
        set_picture_from_path_if_needed(
            &widgets.export_preview_picture,
            export_preview_path
                .or(self.preview_path.as_ref())
                .map(PathBuf::as_path),
            &mut widgets.export_preview_picture_path,
        );
        widgets.preview_meta.set_label(&self.preview_meta_text());
        widgets
            .crop_summary_label
            .set_label(&self.crop_summary_text());
        set_widget_css_class(
            &widgets.crop_square_button,
            "choice-card-active",
            self.crop_preset == CropPreset::Square,
        );
        set_widget_css_class(
            &widgets.crop_widescreen_button,
            "choice-card-active",
            self.crop_preset == CropPreset::Landscape16x9,
        );
        set_widget_css_class(
            &widgets.crop_story_button,
            "choice-card-active",
            self.crop_preset == CropPreset::Portrait9x16,
        );
        set_widget_css_class(
            &widgets.crop_start_button,
            "pill-button-active",
            self.crop_anchor == CropAnchor::Start,
        );
        set_widget_css_class(
            &widgets.crop_center_button,
            "pill-button-active",
            self.crop_anchor == CropAnchor::Center,
        );
        set_widget_css_class(
            &widgets.crop_end_button,
            "pill-button-active",
            self.crop_anchor == CropAnchor::End,
        );
        widgets
            .apply_crop_button
            .set_sensitive(has_frames && !self.selection.is_empty());
        widgets.clear_crop_button.set_sensitive(
            has_frames
                && self.timeline.frames().iter().any(|frame| {
                    self.selection.contains(&frame.id) && frame.transform_spec.crop.is_some()
                }),
        );
        widgets
            .loop_source_label
            .set_label(&self.loop_source_text());
        widgets
            .loop_summary_label
            .set_label(&self.loop_summary_text());
        set_spin_if_needed(&widgets.loop_repeats_spin, self.loop_repeats as f64);
        widgets.loop_create_button.set_sensitive(
            has_frames && (self.loop_scope == LoopScope::AllFrames || !self.selection.is_empty()),
        );
        set_widget_css_class(
            &widgets.loop_duplicate_button,
            "choice-card-active",
            self.loop_method == LoopMethod::Duplicate,
        );
        set_widget_css_class(
            &widgets.loop_reverse_button,
            "choice-card-active",
            self.loop_method == LoopMethod::Reverse,
        );
        set_widget_css_class(
            &widgets.loop_ping_pong_button,
            "choice-card-active",
            self.loop_method == LoopMethod::PingPong,
        );
        set_widget_css_class(
            &widgets.loop_scope_selected_button,
            "pill-button-active",
            self.loop_scope == LoopScope::Selected,
        );
        set_widget_css_class(
            &widgets.loop_scope_all_button,
            "pill-button-active",
            self.loop_scope == LoopScope::AllFrames,
        );

        let output_text = self
            .last_output_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if widgets.output_entry.text().as_str() != output_text {
            widgets.suppress_output_entry_change.set(true);
            widgets.output_entry.set_text(&output_text);
            widgets.suppress_output_entry_change.set(false);
        }
        sync_combo_active_encoder_preset(
            &widgets.encoder_combo,
            self.export_profile.encoder_preset,
        );
        sync_combo_active_fit_mode(&widgets.fit_mode_combo, self.export_profile.fit_mode);
        sync_combo_active_dimension_preset(&widgets.quick_resize_combo, self.quick_resize_preset);
        set_visible_if_needed(
            &widgets.quick_resize_custom_panel,
            self.quick_resize_preset == DimensionPreset::Custom,
        );
        set_stack_visible_child_name_if_needed(
            &widgets.quick_resize_mode_stack,
            self.quick_resize_mode.stack_name(),
        );
        set_spin_if_needed(
            &widgets.quick_resize_multiplier_spin,
            self.quick_resize_multiplier,
        );
        set_spin_if_needed(
            &widgets.quick_resize_custom_width_spin,
            self.quick_resize_custom_width as f64,
        );
        set_spin_if_needed(
            &widgets.quick_resize_custom_height_spin,
            self.quick_resize_custom_height as f64,
        );
        sync_combo_active_dimension_preset(
            &widgets.export_size_combo,
            export_dimension_preset(&self.export_profile),
        );
        set_widget_css_class(
            &widgets.export_preset_fast_button,
            "choice-card-active",
            self.export_profile.preset == ExportPreset::FastPreview,
        );
        set_widget_css_class(
            &widgets.export_preset_balanced_button,
            "choice-card-active",
            self.export_profile.preset == ExportPreset::Balanced,
        );
        set_widget_css_class(
            &widgets.export_preset_high_button,
            "choice-card-active",
            self.export_profile.preset == ExportPreset::HighQuality,
        );
        set_widget_css_class(
            &widgets.export_preset_lossless_button,
            "choice-card-active",
            self.export_profile.preset == ExportPreset::Lossless,
        );
        set_scale_if_needed(&widgets.quality_scale, self.export_profile.quality as f64);
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
        sync_combo_active_loop_count(&widgets.loop_count_combo, self.export_profile.loop_count);
        set_switch_if_needed(&widgets.overwrite_switch, self.export_profile.overwrite);
        if widgets.raw_args_entry.text().as_str() != self.export_profile.raw_args {
            widgets
                .raw_args_entry
                .set_text(&self.export_profile.raw_args);
        }
        widgets
            .export_summary_dimensions_value
            .set_label(&self.export_dimensions_text());
        widgets.export_summary_frame_count_value.set_label(&format!(
            "{export_frame_count} frame{}",
            if export_frame_count == 1 { "" } else { "s" }
        ));
        widgets
            .export_summary_duration_value
            .set_label(&self.export_duration_summary_text());
        widgets
            .export_summary_loop_count_value
            .set_label(&self.export_loop_count_summary_text());
        widgets
            .export_summary_quality_value
            .set_label(&format!("{:.0}", self.export_profile.quality));
        widgets.export_summary_estimated_size_value.set_label(
            &self.estimated_file_size_text(export_frame_count, self.total_duration_ms()),
        );
        let (action_icon, action_title, action_detail, action_class) =
            self.export_action_presentation();
        widgets.export_action_icon.set_icon_name(Some(action_icon));
        widgets.export_action_title_label.set_label(action_title);
        widgets.export_action_detail_label.set_label(action_detail);
        for class_name in [
            "export-action-info",
            "export-action-success",
            "export-action-warning",
            "export-action-error",
        ] {
            widgets.export_action_icon.remove_css_class(class_name);
            widgets
                .export_action_title_label
                .remove_css_class(class_name);
        }
        widgets.export_action_icon.add_css_class(action_class);
        widgets
            .export_action_title_label
            .add_css_class(action_class);
        widgets.preview_export_button.set_sensitive(has_frames);
        widgets
            .export_button
            .set_sensitive(has_frames && self.last_output_path.is_some());

        self.sync_inspector_widgets(widgets);

        let timeline_render_signature = self.timeline_render_signature();
        if widgets.timeline_render_signature != timeline_render_signature {
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

            widgets.timeline_render_signature = timeline_render_signature;
        }
    }
}

impl AppModel {
    fn set_status(&mut self, scope: FooterStatusScope, status: impl Into<String>) {
        self.status = status.into();
        self.footer_status_scope = scope;
    }

    fn scoped_status_text(&self, scope: FooterStatusScope) -> Option<&str> {
        (self.footer_status_scope == scope).then_some(self.status.as_str())
    }

    fn footer_summary_text_for_tab(&self, tab: WorkflowTab) -> String {
        match tab {
            WorkflowTab::Organize => self.organize_footer_summary_text(),
            WorkflowTab::Edit => self.edit_footer_summary_text(),
            WorkflowTab::Loop => self.loop_footer_summary_text(),
            WorkflowTab::Export => self.export_footer_summary_text(),
            WorkflowTab::Diagnostics => self.diagnostics_footer_summary_text(),
        }
    }

    fn footer_context_text(&self) -> String {
        let summary = self.footer_summary_text_for_tab(self.active_tab);
        if self.footer_status_scope.matches_tab(self.active_tab) {
            if summary.is_empty() || self.status == summary {
                return self.status.clone();
            }
            return format!("{} • {}", self.status, summary);
        }
        if !summary.is_empty() {
            return summary;
        }
        if self.footer_status_scope == FooterStatusScope::Global {
            return self.status.clone();
        }
        String::new()
    }

    fn organize_footer_summary_text(&self) -> String {
        let frame_count = self.timeline.frames().len();
        if frame_count == 0 {
            return "Import images or open a project to begin.".to_string();
        }
        if self.selection.is_empty() {
            return format!(
                "{} image{} loaded. Drag timeline tiles to reorder.",
                frame_count,
                if frame_count == 1 { "" } else { "s" }
            );
        }
        format!(
            "{} selected. Drag tiles to reorder or switch to Edit/Timeline for changes.",
            self.selection.len()
        )
    }

    fn edit_footer_summary_text(&self) -> String {
        if self.selection.is_empty() {
            return "Select a frame to inspect it. Quick resize applies to all frames.".to_string();
        }
        if self.selection.len() > 1 {
            return format!(
                "{} selected. Quick actions and advanced edits apply to all selected frames.",
                self.selection.len()
            );
        }

        let Some(frame) = self.primary_selected_frame() else {
            return "Select a frame to rotate, flip, crop, or resize.".to_string();
        };

        let index = self
            .current_frame_index()
            .map(|value| value + 1)
            .unwrap_or(1);
        let mut parts = vec![
            format!("Frame {index:03} {}", frame.file_name()),
            frame
                .source_dimensions
                .map(|(width, height)| format!("{width} x {height}"))
                .unwrap_or_else(|| "unknown size".to_string()),
            format!("{} ms", frame.duration_ms),
        ];
        let mut edits = Vec::new();
        if let Some(crop) = frame.transform_spec.crop {
            edits.push(format!("crop {} x {}", crop.width, crop.height));
        }
        if let Some(resize) = frame.transform_spec.resize {
            edits.push(format!("resize {} x {}", resize.width, resize.height));
            if frame.transform_spec.fit_mode != FitMode::Contain {
                edits.push(format!(
                    "fit {}",
                    frame.transform_spec.fit_mode.as_str().to_ascii_lowercase()
                ));
            }
        }
        let rotation = frame.transform_spec.rotate_quarter_turns.rem_euclid(4);
        if rotation != 0 {
            edits.push(format!("rotate {}°", rotation * 90));
        }
        match (
            frame.transform_spec.flip_horizontal,
            frame.transform_spec.flip_vertical,
        ) {
            (true, true) => edits.push("flip H/V".to_string()),
            (true, false) => edits.push("flip H".to_string()),
            (false, true) => edits.push("flip V".to_string()),
            (false, false) => {}
        }
        if edits.is_empty() {
            edits.push("no edits yet".to_string());
        }
        parts.extend(edits);
        parts.join(" • ")
    }

    fn loop_footer_summary_text(&self) -> String {
        if self.timeline.is_empty() {
            return "Import images to build loops and timeline actions.".to_string();
        }

        let source = self.current_loop_source();
        if source.is_empty() {
            return "Select a range to build a loop, or switch scope to All Images.".to_string();
        }

        let added_frames = source.len().saturating_mul(self.loop_repeats as usize);
        let added_duration = source
            .iter()
            .map(|frame| u64::from(frame.duration_ms))
            .sum::<u64>()
            .saturating_mul(u64::from(self.loop_repeats));
        let scope_label = match self.loop_scope {
            LoopScope::Selected => "Selected Range",
            LoopScope::AllFrames => "All Images",
        };
        format!(
            "{} • {} • {} repeat{} • adds {} frame(s) / {}",
            self.loop_method.title(),
            scope_label,
            self.loop_repeats,
            if self.loop_repeats == 1 { "" } else { "s" },
            added_frames,
            format_duration_ms(added_duration)
        )
    }

    fn export_footer_summary_text(&self) -> String {
        let export_frame_count = self
            .timeline
            .frames()
            .iter()
            .filter(|frame| frame.enabled)
            .count();
        if export_frame_count == 0 {
            return "Import images before exporting.".to_string();
        }
        if self.last_output_path.is_none() {
            return "Choose an output file.".to_string();
        }
        let loop_text = if self.export_profile.loop_count == 0 {
            "infinite".to_string()
        } else {
            self.export_profile.loop_count.to_string()
        };
        format!(
            "{} • {} • {} frame{} • {} • loop {}",
            self.export_profile.preset.as_str(),
            self.export_dimensions_text(),
            export_frame_count,
            if export_frame_count == 1 { "" } else { "s" },
            format_duration_ms(self.total_duration_ms()),
            loop_text
        )
    }

    fn diagnostics_footer_summary_text(&self) -> String {
        if self.diagnostics.ffmpeg_ok && self.diagnostics.ffprobe_ok {
            "ffmpeg and ffprobe are available.".to_string()
        } else {
            self.diagnostics
                .issues
                .first()
                .cloned()
                .unwrap_or_else(|| "A required system tool is unavailable.".to_string())
        }
    }

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
        self.sync_quick_resize_state_from_selection();
        self.set_status(
            FooterStatusScope::Organize,
            format!(
                "Imported {} frame(s). Generating thumbnails...",
                imported_ids.len()
            ),
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
        self.sync_quick_resize_state_from_selection();
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
            self.set_status(FooterStatusScope::Global, "No frames in timeline.");
            return;
        };
        self.select_single_frame(frame_id, sender);
        let status = if end {
            "Moved to last frame.".to_string()
        } else {
            "Moved to first frame.".to_string()
        };
        self.set_status(FooterStatusScope::Global, status);
    }

    fn navigate_by_step(&mut self, offset: isize, sender: &ComponentSender<Self>) {
        self.stop_playback(None);
        let frame_ids = self.timeline_frame_ids();
        let Some(frame_id) = step_frame_id(&frame_ids, self.primary_selected_id(), offset) else {
            self.set_status(FooterStatusScope::Global, "No frames in timeline.");
            return;
        };
        self.select_single_frame(frame_id, sender);
        let status = if offset < 0 {
            "Moved back one frame.".to_string()
        } else {
            "Moved forward one frame.".to_string()
        };
        self.set_status(FooterStatusScope::Global, status);
    }

    fn toggle_playback(&mut self, sender: &ComponentSender<Self>) {
        if self.playback_active {
            self.stop_playback(Some("Playback paused."));
            return;
        }

        let frame_ids = self.timeline_frame_ids();
        let Some(frame_id) = playback_start_frame_id(&frame_ids, self.primary_selected_id()) else {
            self.set_status(FooterStatusScope::Global, "No frames in timeline.");
            return;
        };

        self.playback_generation = self.playback_generation.wrapping_add(1);
        self.playback_active = true;
        self.select_single_frame(frame_id, sender);
        self.set_status(FooterStatusScope::Global, "Playback started.");
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
            self.set_status(FooterStatusScope::Global, status);
        }
    }

    fn project_document(&self) -> ProjectDocument {
        ProjectDocument {
            frames: self.timeline.frames().to_vec(),
            export_profile: self.export_profile.clone(),
            last_output_path: self.last_output_path.clone(),
        }
    }

    fn apply_project_document(&mut self, document: ProjectDocument) -> Vec<u64> {
        let ids: Vec<_> = document.frames.iter().map(|frame| frame.id).collect();
        self.timeline = Timeline::from_frames(document.frames);
        self.selection = ids.iter().copied().collect();
        self.selection_anchor_id = self.timeline.frames().first().map(|frame| frame.id);
        self.export_profile = document.export_profile;
        self.last_output_path = document.last_output_path;
        self.preview_path = None;
        self.preview_frame_id = None;
        self.preview_rendered_size = None;
        self.invalidate_export_preview();
        self.sync_quick_resize_state_from_selection();
        ids
    }

    fn save_autosave_project(&self) -> anyhow::Result<()> {
        let _ = save_autosave_project(&self.project_document())?;
        Ok(())
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

    fn refresh_all_frame_jobs(&mut self, sender: &ComponentSender<Self>) {
        let ids = self.timeline_frame_ids();
        self.refresh_frame_jobs(ids, sender);
        self.preview_rendered_size = None;
        self.queue_preview_for_primary_selection(sender);
    }

    fn queue_preview_for_primary_selection(&mut self, sender: &ComponentSender<Self>) {
        let Some(frame) = self.primary_selected_frame().cloned() else {
            self.preview_frame_id = None;
            self.preview_path = None;
            self.preview_rendered_size = None;
            self.invalidate_export_preview();
            return;
        };
        self.invalidate_export_preview();
        let same_frame = self.preview_frame_id == Some(frame.id);
        let render_size = self.preview_render_size_for_frame(&frame);
        let cached_preview_path = preview_cache_path(&frame, &self.cache_dir, render_size);
        let cached_preview_path = cached_preview_path.is_file().then_some(cached_preview_path);
        self.preview_frame_id = Some(frame.id);

        if let Some(cached_preview_path) = cached_preview_path {
            self.preview_path = Some(cached_preview_path);
            self.preview_rendered_size = Some(render_size);
        } else {
            let current_preview_path = same_frame
                .then_some(self.preview_path.as_ref())
                .flatten()
                .filter(|path| {
                    self.preview_rendered_size.is_some() || !preview_path_is_proxy(&frame, path)
                });
            let fallback_path =
                immediate_preview_path(&frame, None, current_preview_path, self.playback_active);
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

    fn queue_export_preview_for_primary_selection(&mut self, sender: &ComponentSender<Self>) {
        let Some(frame) = self.primary_selected_frame().cloned() else {
            self.set_status(
                FooterStatusScope::Export,
                "Select a frame to render an export preview.",
            );
            return;
        };

        let generation = self.export_preview_generation;
        let render_size = self.export_preview_render_size_for_frame(&frame);
        let export_size = self.export_preview_target();
        let export_fit_mode = self.export_profile.fit_mode;
        let cached_preview_path = export_preview_cache_path(
            &frame,
            &self.cache_dir,
            render_size,
            export_size,
            export_fit_mode,
        );
        if cached_preview_path.is_file() {
            self.export_preview_path = Some(cached_preview_path);
            self.export_preview_rendered_size = Some(render_size);
            self.set_status(
                FooterStatusScope::Export,
                "Export preview refreshed with the current export settings.",
            );
            return;
        }

        self.set_status(
            FooterStatusScope::Export,
            "Rendering export preview with the current export settings...",
        );
        let frame_id = frame.id;
        let cache_dir = self.cache_dir.clone();
        sender.spawn_oneshot_command(move || {
            let result = render_export_preview(
                &frame,
                &cache_dir,
                render_size,
                export_size,
                export_fit_mode,
            );
            CommandMsg::ExportPreviewReady {
                frame_id,
                generation,
                render_size,
                preview_path: result.as_ref().ok().cloned(),
                error: result.err().map(|err| err.to_string()),
            }
        });
    }

    fn prewarm_upcoming_playback_previews(&self) {
        if !self.playback_active {
            return;
        }

        let Some(current_index) = self.current_frame_index() else {
            return;
        };

        for frame in self
            .timeline
            .frames()
            .iter()
            .skip(current_index + 1)
            .take(2)
        {
            let frame = frame.clone();
            let cache_dir = self.cache_dir.clone();
            let render_size = preview_render_size_for_dimensions(
                frame_effective_dimensions(&frame),
                self.preview_target_size,
            );
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

    fn apply_to_all_frames(&mut self, mut apply: impl FnMut(&mut FrameItem)) {
        for frame in self.timeline.frames_mut() {
            apply(frame);
        }
    }

    fn apply_quick_crop(&mut self, sender: &ComponentSender<Self>) {
        if self.selection.is_empty() {
            self.set_status(
                FooterStatusScope::Edit,
                "Select frames in the timeline before applying a crop.",
            );
            return;
        }

        let mut applied = 0usize;
        let mut skipped = 0usize;
        let crop_preset = self.crop_preset;
        let crop_anchor = self.crop_anchor;
        for frame in self.timeline.frames_mut() {
            if !self.selection.contains(&frame.id) {
                continue;
            }
            if let Some(crop) = crop_rect_for_frame(frame, crop_preset, crop_anchor) {
                frame.transform_spec.crop = Some(crop);
                applied += 1;
            } else {
                skipped += 1;
            }
        }

        if applied == 0 {
            self.set_status(
                FooterStatusScope::Edit,
                "Crop could not be applied yet because the selected frames have no image size information.",
            );
            return;
        }

        let status = if skipped == 0 {
            format!(
                "Applied a {} crop to {} frame(s).",
                self.crop_preset.title().to_ascii_lowercase(),
                applied
            )
        } else {
            format!(
                "Applied a {} crop to {} frame(s); skipped {} frame(s) without image size information.",
                self.crop_preset.title().to_ascii_lowercase(),
                applied,
                skipped
            )
        };
        self.sync_quick_resize_state_from_selection();
        self.set_status(FooterStatusScope::Edit, status);
        self.refresh_selection_jobs(sender);
    }

    fn clear_quick_crop(&mut self, sender: &ComponentSender<Self>) {
        if self.selection.is_empty() {
            self.set_status(
                FooterStatusScope::Edit,
                "Select frames in the timeline before clearing a crop.",
            );
            return;
        }

        let mut cleared = 0usize;
        for frame in self.timeline.frames_mut() {
            if self.selection.contains(&frame.id) && frame.transform_spec.crop.take().is_some() {
                cleared += 1;
            }
        }

        if cleared == 0 {
            self.set_status(
                FooterStatusScope::Edit,
                "The selected frames do not currently have a crop to clear.",
            );
            return;
        }

        self.sync_quick_resize_state_from_selection();
        self.set_status(
            FooterStatusScope::Edit,
            format!("Cleared crop from {} frame(s).", cleared),
        );
        self.refresh_selection_jobs(sender);
    }

    fn set_advanced_mode(&mut self, enabled: bool) -> Result<(), String> {
        self.advanced_mode = enabled;
        self.ui_preferences.advanced_mode = enabled;
        save_ui_preferences(&self.ui_preferences)
            .map_err(|err| format!("Failed to save UI preferences: {err}"))
    }

    fn invalidate_export_preview(&mut self) {
        self.export_preview_generation = self.export_preview_generation.wrapping_add(1);
        self.export_preview_path = None;
        self.export_preview_rendered_size = None;
    }

    fn export_preview_target(&self) -> Option<ResizeTarget> {
        match (
            self.export_profile.output_width,
            self.export_profile.output_height,
        ) {
            (Some(width), Some(height)) if width > 0 && height > 0 => {
                Some(ResizeTarget { width, height })
            }
            _ => None,
        }
    }

    fn export_dimensions_text(&self) -> String {
        self.export_preview_target()
            .map(|resize| format!("{} x {}", resize.width, resize.height))
            .or_else(|| {
                self.timeline
                    .frames()
                    .iter()
                    .find_map(frame_effective_dimensions)
                    .map(|(width, height)| format!("{width} x {height}"))
            })
            .unwrap_or_else(|| "Original size".to_string())
    }

    fn crop_summary_text(&self) -> String {
        if self.selection.is_empty() {
            return "Choose a crop shape, then apply it to the selected frames.".to_string();
        }

        let Some(frame) = self.primary_selected_frame() else {
            return "Choose a crop shape, then apply it to the selected frames.".to_string();
        };

        let Some(crop) = crop_rect_for_frame(frame, self.crop_preset, self.crop_anchor) else {
            return "Crop will be ready once the selected frame dimensions are known.".to_string();
        };

        let current = frame
            .transform_spec
            .crop
            .map(|existing| format!("Current crop: {} x {}. ", existing.width, existing.height))
            .unwrap_or_default();
        format!(
            "{}{} preset. {} {}. Primary frame preview crop: {} x {} at {},{}.",
            current,
            self.crop_preset.title(),
            self.crop_preset.helper_text(),
            self.crop_anchor.helper_text(),
            crop.width,
            crop.height,
            crop.x,
            crop.y
        )
    }

    fn total_duration_ms(&self) -> u64 {
        self.timeline
            .frames()
            .iter()
            .map(|frame| u64::from(frame.duration_ms))
            .sum()
    }

    fn poll_export_state(&mut self, sender: &ComponentSender<Self>) {
        let Some(export_worker_state) = self.export_worker_state.clone() else {
            if let Some(active) = self.export_poll_active.take() {
                active.set(false);
            }
            return;
        };

        let snapshot = match export_worker_state.lock() {
            Ok(state) => state.clone(),
            Err(_) => {
                self.export_in_progress = false;
                self.export_progress = None;
                self.export_worker_state = None;
                EXPORT_LAYOUT_WATCH_SUSPENDED.store(false, Ordering::Relaxed);
                if let Some(active) = self.export_poll_active.take() {
                    active.set(false);
                }
                self.set_status(
                    FooterStatusScope::Export,
                    "Export failed: background export state lock poisoned.",
                );
                return;
            }
        };

        if let Some(progress) = snapshot.progress {
            self.set_status(FooterStatusScope::Export, progress.detail.clone());
            self.export_progress = Some(progress);
        }

        if let Some(result) = snapshot.result {
            self.export_in_progress = false;
            self.export_completion_pending = true;
            self.export_worker_state = None;
            self.export_progress = None;
            if let Some(active) = self.export_poll_active.take() {
                active.set(false);
            }
            match result {
                Ok(job) => {
                    self.last_output_path = Some(job.output_path.clone());
                    self.set_status(
                        FooterStatusScope::Export,
                        format!("Exported {}", job.output_path.display()),
                    );
                }
                Err(err) => {
                    self.set_status(FooterStatusScope::Export, format!("Export failed: {err}"));
                }
            }
            let sender = sender.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(0), move || {
                sender.input(AppMsg::FinalizeExportUi);
            });
        }
    }

    fn timeline_render_signature(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.timeline.frames().len().hash(&mut hasher);
        for frame in self.timeline.frames() {
            frame.id.hash(&mut hasher);
            frame.source_path.hash(&mut hasher);
            frame.thumbnail_path.hash(&mut hasher);
            frame.enabled.hash(&mut hasher);
            frame.duration_ms.hash(&mut hasher);
            frame_effective_dimensions(frame).hash(&mut hasher);
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
            frame
                .thumbnail_path
                .as_deref()
                .and_then(timeline_file_stamp)
                .hash(&mut hasher);
        }
        for selected_id in &self.selection {
            selected_id.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn readiness_text(&self) -> String {
        if self.export_in_progress || self.export_completion_pending {
            "Exporting".to_string()
        } else if !self.diagnostics.ffmpeg_ok || !self.diagnostics.ffprobe_ok {
            "Needs Setup".to_string()
        } else if self.timeline.is_empty() {
            "Add Images".to_string()
        } else if self.thumbnails_pending > 0 {
            "Preparing".to_string()
        } else {
            "Ready".to_string()
        }
    }

    fn diagnostics_overview_text(&self) -> String {
        if self.diagnostics.ffmpeg_ok && self.diagnostics.ffprobe_ok {
            "Everything looks good. ffmpeg and ffprobe are available for export and probing."
                .to_string()
        } else {
            self.diagnostics
                .issues
                .first()
                .cloned()
                .unwrap_or_else(|| "A required system tool is unavailable.".to_string())
        }
    }

    fn loop_selection(&self) -> BTreeSet<u64> {
        match self.loop_scope {
            LoopScope::Selected => self.selection.clone(),
            LoopScope::AllFrames => BTreeSet::new(),
        }
    }

    fn current_loop_source(&self) -> Vec<FrameItem> {
        let selection = self.loop_selection();
        if self.loop_scope == LoopScope::Selected && selection.is_empty() {
            return Vec::new();
        }
        match self.loop_method {
            LoopMethod::Duplicate => self.timeline.duplicate_loop_source(&selection),
            LoopMethod::Reverse => self.timeline.reverse_loop_source(&selection, true),
            LoopMethod::PingPong => self.timeline.reverse_loop_source(&selection, false),
        }
    }

    fn loop_source_text(&self) -> String {
        let scope_label = match self.loop_scope {
            LoopScope::Selected => {
                if self.selection.is_empty() {
                    "No range selected yet"
                } else {
                    "Using the selected timeline range"
                }
            }
            LoopScope::AllFrames => "Using all images in the timeline",
        };
        let source = self.current_loop_source();
        let duration = source
            .iter()
            .map(|frame| u64::from(frame.duration_ms))
            .sum::<u64>();
        format!(
            "{}: {} frame(s) per loop • {}",
            scope_label,
            source.len(),
            format_duration_ms(duration)
        )
    }

    fn loop_summary_text(&self) -> String {
        let source = self.current_loop_source();
        let added_frames = source.len().saturating_mul(self.loop_repeats as usize);
        let added_duration = source
            .iter()
            .map(|frame| u64::from(frame.duration_ms))
            .sum::<u64>()
            .saturating_mul(u64::from(self.loop_repeats));
        format!(
            "Method: {}\n{}\nEstimated addition: {} frame(s) • {}",
            self.loop_method.title(),
            self.loop_method.helper_text(),
            added_frames,
            format_duration_ms(added_duration)
        )
    }

    fn export_duration_summary_text(&self) -> String {
        let frame_count = self
            .timeline
            .frames()
            .iter()
            .filter(|frame| frame.enabled)
            .count();
        let duration_ms = self.total_duration_ms();
        if frame_count == 0 || duration_ms == 0 {
            return "--".to_string();
        }
        let fps = frame_count as f64 / (duration_ms as f64 / 1000.0);
        format!("{} ({fps:.0} fps)", format_duration_ms(duration_ms))
    }

    fn export_loop_count_summary_text(&self) -> String {
        if self.timeline.is_empty() {
            return "--".to_string();
        }
        if self.export_profile.loop_count == 0 {
            "Infinite".to_string()
        } else {
            self.export_profile.loop_count.to_string()
        }
    }

    fn export_action_presentation(
        &self,
    ) -> (&'static str, &'static str, &'static str, &'static str) {
        if self.export_in_progress {
            return (
                "view-refresh-symbolic",
                "Exporting",
                "Your animated WebP is being created.",
                "export-action-info",
            );
        }
        if self.export_completion_pending {
            return (
                "view-refresh-symbolic",
                "Finishing Export",
                "Updating the interface after export.",
                "export-action-info",
            );
        }
        if self.timeline.is_empty() {
            return (
                "dialog-warning-symbolic",
                "No Frames Imported",
                "Import images before exporting.",
                "export-action-warning",
            );
        }
        if self.last_output_path.is_none() {
            return (
                "dialog-warning-symbolic",
                "Missing Output File",
                "Choose where to save your animated WebP.",
                "export-action-warning",
            );
        }
        if self
            .scoped_status_text(FooterStatusScope::Export)
            .is_some_and(|status| status.starts_with("Export failed"))
        {
            return (
                "dialog-error-symbolic",
                "Export Failed",
                "Open Diagnostics for technical details.",
                "export-action-error",
            );
        }
        if self
            .scoped_status_text(FooterStatusScope::Export)
            .is_some_and(|status| status.starts_with("Exported "))
        {
            return (
                "emblem-ok-symbolic",
                "Export Complete",
                "Your animated WebP was created.",
                "export-action-success",
            );
        }
        (
            "emblem-ok-symbolic",
            "Ready to Export",
            "Everything looks good. Click export to create your animated WebP.",
            "export-action-success",
        )
    }

    fn estimated_file_size_text(&self, frame_count: usize, duration_ms: u64) -> String {
        if frame_count == 0 {
            return "--".to_string();
        }
        let dimensions_factor = match (
            self.export_profile.output_width,
            self.export_profile.output_height,
        ) {
            (Some(width), Some(height)) => {
                (u64::from(width) * u64::from(height)) as f64 / 921_600.0
            }
            _ => 1.0,
        };
        let duration_factor = (duration_ms as f64 / 1000.0).max(0.2);
        let quality_factor = (self.export_profile.quality as f64 / 80.0).clamp(0.35, 1.4);
        let lossless_factor = if self.export_profile.lossless {
            2.0
        } else {
            1.0
        };
        let mb = (duration_factor * dimensions_factor * quality_factor * lossless_factor * 0.85)
            .max(0.1);
        format!("~ {:.1} MB", mb)
    }

    fn selection_dimension_preset(&self) -> DimensionPreset {
        match self
            .primary_selected_frame()
            .or_else(|| self.timeline.frames().first())
            .and_then(|frame| frame.transform_spec.resize)
        {
            None => DimensionPreset::Original,
            Some(ResizeTarget {
                width: 1920,
                height: 1080,
            }) => DimensionPreset::Hd1080,
            Some(ResizeTarget {
                width: 1280,
                height: 720,
            }) => DimensionPreset::Hd720,
            Some(_) => DimensionPreset::Custom,
        }
    }

    fn sync_quick_resize_state_from_selection(&mut self) {
        self.quick_resize_preset = self.selection_dimension_preset();
        self.quick_resize_multiplier = 1.0;
        self.seed_quick_resize_custom_dimensions();
    }

    fn seed_quick_resize_custom_dimensions(&mut self) {
        let (width, height) = self.quick_resize_reference_dimensions().unwrap_or((0, 0));
        self.quick_resize_custom_width = width;
        self.quick_resize_custom_height = height;
    }

    fn quick_resize_reference_dimensions(&self) -> Option<(u32, u32)> {
        self.primary_selected_frame()
            .or_else(|| self.timeline.frames().first())
            .and_then(frame_effective_dimensions)
    }

    fn linked_resize_height_for_width(&self, width: u32) -> Option<u32> {
        if width == 0 {
            return Some(0);
        }
        let (base_width, base_height) = self.quick_resize_reference_dimensions()?;
        scale_dimension_by_ratio(width, base_height, base_width)
    }

    fn linked_resize_width_for_height(&self, height: u32) -> Option<u32> {
        if height == 0 {
            return Some(0);
        }
        let (base_width, base_height) = self.quick_resize_reference_dimensions()?;
        scale_dimension_by_ratio(height, base_width, base_height)
    }

    fn quick_resize_target(&self) -> Option<ResizeTarget> {
        match self.quick_resize_preset {
            DimensionPreset::Original => None,
            DimensionPreset::Hd1080 => Some(ResizeTarget {
                width: 1920,
                height: 1080,
            }),
            DimensionPreset::Hd720 => Some(ResizeTarget {
                width: 1280,
                height: 720,
            }),
            DimensionPreset::Custom => match self.quick_resize_mode {
                QuickResizeMode::Multiplier => {
                    let (base_width, base_height) = self.quick_resize_reference_dimensions()?;
                    let multiplier = self.quick_resize_multiplier.max(0.1);
                    Some(ResizeTarget {
                        width: scaled_resize_dimension(base_width, multiplier),
                        height: scaled_resize_dimension(base_height, multiplier),
                    })
                }
                QuickResizeMode::Custom => {
                    let width = self.quick_resize_custom_width;
                    let height = self.quick_resize_custom_height;
                    (width > 0 && height > 0).then_some(ResizeTarget { width, height })
                }
            },
        }
    }

    fn preferred_export_custom_dimensions(&self) -> Option<(u32, u32)> {
        if self.quick_resize_preset == DimensionPreset::Custom {
            self.quick_resize_target()
                .map(|target| (target.width, target.height))
        } else {
            None
        }
    }

    fn recompute_command_preview(&mut self) {
        let output_path = self
            .last_output_path
            .as_deref()
            .map(normalized_output_path)
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
        let source_dims = frame
            .source_dimensions
            .map(|(w, h)| format!("{w} x {h}"))
            .unwrap_or_else(|| "unknown".to_string());
        let effective_dims = frame_effective_dimensions(frame)
            .map(|(w, h)| format!("{w} x {h}"))
            .unwrap_or_else(|| source_dims.clone());
        let dims = if effective_dims == source_dims {
            source_dims.clone()
        } else {
            format!("{source_dims} -> {effective_dims}")
        };
        if !self.advanced_mode {
            return format!("{}\n{} • {} ms", frame.file_name(), dims, frame.duration_ms,);
        }
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
            "{}\n{} | current {} | {} ms | rotate {} quarter-turns\n{} | {} | fit {} | flip h:{} v:{}",
            frame.file_name(),
            source_dims,
            effective_dims,
            frame.duration_ms,
            frame.transform_spec.rotate_quarter_turns.rem_euclid(4),
            crop,
            resize,
            frame.transform_spec.fit_mode,
            frame.transform_spec.flip_horizontal,
            frame.transform_spec.flip_vertical
        )
    }

    fn preview_render_size_for_frame(&self, frame: &FrameItem) -> PreviewRenderSize {
        preview_render_size_for_dimensions(
            frame_effective_dimensions(frame),
            self.preview_target_size,
        )
    }

    fn export_preview_render_size_for_frame(&self, frame: &FrameItem) -> PreviewRenderSize {
        let export_dimensions = self
            .export_preview_target()
            .map(|target| (target.width, target.height))
            .or_else(|| frame_effective_dimensions(frame));
        preview_render_size_for_dimensions(export_dimensions, self.preview_target_size)
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
    let (picture_width, picture_height) = timeline_tile_picture_size(frame);
    let tile = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .width_request(picture_width + 12)
        .build();
    set_accessible_label(
        &tile,
        &format!("Frame {:03} {}", index + 1, frame.file_name()),
    );
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

    let picture = gtk::Picture::new();
    set_picture_from_path(&picture, frame.thumbnail_path.as_deref());
    picture.set_size_request(picture_width, picture_height);
    picture.set_can_shrink(true);
    picture.set_halign(gtk::Align::Center);
    picture.add_css_class("timeline-picture");

    let media_overlay = gtk::Overlay::new();
    media_overlay.set_halign(gtk::Align::Center);
    media_overlay.set_child(Some(&picture));

    let badge_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Start)
        .margin_top(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    let badge = gtk::Label::new(Some(&format!("{:03}", index + 1)));
    badge.add_css_class("timeline-badge");
    let badge_spacer = gtk::Box::builder().hexpand(true).build();
    badge_row.append(&badge);
    badge_row.append(&badge_spacer);
    if selected {
        let check = gtk::Label::new(Some("✓"));
        check.add_css_class("timeline-check");
        badge_row.append(&check);
    }
    media_overlay.add_overlay(&badge_row);
    tile.append(&media_overlay);

    let subtitle = gtk::Label::new(Some(&frame.file_name()));
    subtitle.set_xalign(0.5);
    subtitle.set_justify(gtk::Justification::Center);
    subtitle.set_halign(gtk::Align::Center);
    subtitle.set_wrap(true);
    subtitle.set_wrap_mode(pango::WrapMode::WordChar);
    subtitle.set_max_width_chars(16);
    subtitle.set_lines(2);
    subtitle.add_css_class("timeline-filename");
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

fn combo_for_dimension_preset() -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    combo.set_hexpand(true);
    combo.set_halign(gtk::Align::Fill);
    for preset in [
        DimensionPreset::Original,
        DimensionPreset::Hd1080,
        DimensionPreset::Hd720,
        DimensionPreset::Custom,
    ] {
        combo.append_text(preset.as_str());
    }
    combo.set_active(Some(0));
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

fn combo_for_loop_count() -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    for value in ["0 (Infinite)", "1", "2", "3", "Custom"] {
        combo.append_text(value);
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

fn dimension_preset_from_combo(combo: &gtk::ComboBoxText) -> DimensionPreset {
    match combo.active_text().as_deref() {
        Some("1080p") => DimensionPreset::Hd1080,
        Some("720p") => DimensionPreset::Hd720,
        Some("Custom") => DimensionPreset::Custom,
        _ => DimensionPreset::Original,
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

fn loop_count_from_combo(combo: &gtk::ComboBoxText) -> u32 {
    match combo.active_text().as_deref() {
        Some("1") => 1,
        Some("2") => 2,
        Some("3") => 3,
        Some("Custom") => 0,
        _ => 0,
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

fn sync_combo_active_dimension_preset(combo: &gtk::ComboBoxText, preset: DimensionPreset) {
    let target = match preset {
        DimensionPreset::Original => 0,
        DimensionPreset::Hd1080 => 1,
        DimensionPreset::Hd720 => 2,
        DimensionPreset::Custom => 3,
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

fn sync_combo_active_loop_count(combo: &gtk::ComboBoxText, loop_count: u32) {
    let target = match loop_count {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        _ => 4,
    };
    if combo.active() != Some(target) {
        combo.set_active(Some(target));
    }
}

fn section<W: IsA<gtk::Widget>>(title: &str, child: &W) -> gtk::Frame {
    let frame = gtk::Frame::builder().child(child).build();
    frame.add_css_class("content-card");
    frame.add_css_class("section-card");
    let label = gtk::Label::new(Some(title));
    label.add_css_class("section-title");
    label.set_xalign(0.0);
    frame.set_label_widget(Some(&label));
    frame
}

fn collapsible_section<W: IsA<gtk::Widget>>(title: &str, child: &W, expanded: bool) -> gtk::Frame {
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .reveal_child(expanded)
        .child(child)
        .build();
    let frame = gtk::Frame::builder().child(&revealer).build();
    frame.add_css_class("content-card");
    frame.add_css_class("section-card");
    frame.add_css_class("collapsible-section");

    let header_button = gtk::ToggleButton::builder()
        .active(expanded)
        .focus_on_click(false)
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    header_button.add_css_class("section-toggle");
    set_accessible_label(&header_button, &format!("Toggle {title} section"));

    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let title_label = gtk::Label::new(Some(title));
    title_label.add_css_class("section-title");
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    let chevron = gtk::Image::from_icon_name(if expanded {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    });
    chevron.add_css_class("section-chevron");
    header_row.append(&title_label);
    header_row.append(&chevron);
    header_button.set_child(Some(&header_row));
    header_button.connect_toggled(move |button| {
        let open = button.is_active();
        revealer.set_reveal_child(open);
        chevron.set_icon_name(Some(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
    });
    frame.set_label_widget(Some(&header_button));
    frame
}

fn section_without_header<W: IsA<gtk::Widget>>(child: &W) -> gtk::Frame {
    let frame = gtk::Frame::builder().child(child).build();
    frame.add_css_class("content-card");
    frame.add_css_class("section-card");
    frame.add_css_class("section-without-header");
    frame
}

fn settings_row<W: IsA<gtk::Widget>>(title: &str, helper: &str, control: &W) -> gtk::Grid {
    settings_row_with_options(title, helper, control, true, false)
}

fn settings_toggle_row<W: IsA<gtk::Widget>>(title: &str, helper: &str, control: &W) -> gtk::Grid {
    settings_row_with_options(title, helper, control, false, true)
}

fn settings_row_with_options<W: IsA<gtk::Widget>>(
    title: &str,
    helper: &str,
    control: &W,
    control_expands: bool,
    align_end: bool,
) -> gtk::Grid {
    let row = gtk::Grid::builder()
        .column_spacing(20)
        .column_homogeneous(true)
        .build();
    row.add_css_class("settings-row");
    let copy = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .build();
    copy.add_css_class("settings-copy");
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.add_css_class("settings-title");
    let helper_label = helper_label(helper);
    helper_label.set_wrap(true);
    helper_label.set_wrap_mode(pango::WrapMode::WordChar);
    helper_label.set_max_width_chars(28);
    copy.append(&title_label);
    copy.append(&helper_label);
    let control_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .build();
    control_box.add_css_class("settings-control");
    control.as_ref().set_hexpand(control_expands);
    control.as_ref().set_halign(if control_expands {
        gtk::Align::Fill
    } else {
        gtk::Align::End
    });
    if align_end {
        let spacer = gtk::Box::builder().hexpand(true).build();
        control_box.append(&spacer);
    }
    control_box.append(control);
    row.attach(&copy, 0, 0, 1, 1);
    row.attach(&control_box, 1, 0, 2, 1);
    row
}

fn build_tab_button(label: &str, icon_name: &str, icon_tone_class: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("workflow-tab");
    set_accessible_label(&button, &format!("{label} workflow tab"));
    button.set_child(Some(&button_label_content(
        label,
        icon_name,
        icon_tone_class,
    )));
    button
}

fn build_choice_button(
    title: &str,
    subtitle: &str,
    icon_name: &str,
    icon_tone_class: &str,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("choice-card");
    set_accessible_label(&button, title);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();
    let icon_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class("button-leading-icon");
    icon.add_css_class("choice-card-icon");
    icon.add_css_class(icon_tone_class);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.5);
    title_label.set_justify(gtk::Justification::Center);
    title_label.add_css_class("heading");
    let subtitle_label = helper_label(subtitle);
    subtitle_label.set_xalign(0.5);
    subtitle_label.set_justify(gtk::Justification::Center);
    icon_row.append(&icon);
    content.append(&icon_row);
    content.append(&title_label);
    content.append(&subtitle_label);
    button.set_child(Some(&content));
    button
}

fn build_labeled_button(label: &str, icon_name: &str, icon_tone_class: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_accessible_label(&button, label);
    button.set_child(Some(&button_label_content(
        label,
        icon_name,
        icon_tone_class,
    )));
    button
}

fn button_label_content(label: &str, icon_name: &str, icon_tone_class: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.add_css_class("button-leading-icon");
    icon.add_css_class(icon_tone_class);
    let text = gtk::Label::new(Some(label));
    row.append(&icon);
    row.append(&text);
    row
}

fn helper_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label
}

fn summary_label(text: &str) -> gtk::Label {
    let label = helper_label(text);
    label.add_css_class("summary-blurb");
    label
}

fn summary_value_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(1.0);
    label.set_hexpand(true);
    label.add_css_class("summary-value");
    label
}

fn attach_summary_row(grid: &gtk::Grid, row: i32, label: &str, value: &gtk::Label) {
    let key = gtk::Label::new(Some(label));
    key.set_xalign(0.0);
    key.add_css_class("summary-key");
    grid.attach(&key, 0, row, 1, 1);
    grid.attach(value, 1, row, 1, 1);
}

fn page_scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(child)
        .build()
}

fn timeline_tile_picture_size(frame: &FrameItem) -> (i32, i32) {
    const PICTURE_HEIGHT: i32 = 78;
    const MIN_WIDTH: i32 = 72;
    const MAX_WIDTH: i32 = 156;

    let aspect_ratio = frame
        .transform_spec
        .resize
        .map(|resize| (resize.width, resize.height))
        .or_else(|| frame_effective_dimensions(frame))
        .or(frame.source_dimensions)
        .filter(|(_, height)| *height > 0)
        .map(|(width, height)| width as f64 / height as f64)
        .unwrap_or(1.0);
    let picture_width = ((PICTURE_HEIGHT as f64) * aspect_ratio).round() as i32;

    (picture_width.clamp(MIN_WIDTH, MAX_WIDTH), PICTURE_HEIGHT)
}

fn set_widget_css_class(widget: &impl IsA<gtk::Widget>, class_name: &str, enabled: bool) {
    if enabled {
        widget.as_ref().add_css_class(class_name);
    } else {
        widget.as_ref().remove_css_class(class_name);
    }
}

fn layout_mode_for_width(width: i32) -> LayoutMode {
    if width > 0 && width < 980 {
        LayoutMode::Compact
    } else {
        LayoutMode::Regular
    }
}

fn export_dimension_preset(profile: &ExportProfile) -> DimensionPreset {
    match (profile.output_width, profile.output_height) {
        (None, None) => DimensionPreset::Original,
        (Some(1920), Some(1080)) => DimensionPreset::Hd1080,
        (Some(1280), Some(720)) => DimensionPreset::Hd720,
        _ => DimensionPreset::Custom,
    }
}

fn format_duration_ms(duration_ms: u64) -> String {
    format!("{:.2} s total", duration_ms as f64 / 1000.0)
}

fn build_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("pill-button");
    set_button_icon(&button, icon_name);
    button.set_tooltip_text(Some(tooltip));
    set_accessible_label(&button, tooltip);
    button
}

fn set_accessible_label(widget: &impl IsA<gtk::Accessible>, label: &str) {
    widget.update_property(&[gtk::accessible::Property::Label(label)]);
}

fn install_window_layout_watch(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let last_layout_mode = Rc::new(Cell::new(None));
    window.connect_map(clone!(
        #[strong]
        sender,
        #[strong]
        last_layout_mode,
        move |window| send_window_layout_change(window, &sender, &last_layout_mode)
    ));
    window.connect_notify_local(
        Some("width"),
        clone!(
            #[strong]
            sender,
            #[strong]
            last_layout_mode,
            move |window, _| send_window_layout_change(window, &sender, &last_layout_mode)
        ),
    );
}

fn install_workspace_split_watch(
    workspace_box: &gtk::Box,
    preview_panel: &gtk::Box,
    content_stack: &gtk::Stack,
) {
    sync_workspace_split_widths(workspace_box, preview_panel, content_stack);
    workspace_box.connect_notify_local(
        Some("width"),
        clone!(
            #[strong]
            preview_panel,
            #[strong]
            content_stack,
            move |box_widget, _| {
                sync_workspace_split_widths(box_widget, &preview_panel, &content_stack);
            }
        ),
    );
}

fn send_window_layout_change(
    window: &gtk::Window,
    sender: &ComponentSender<AppModel>,
    last_layout_mode: &Rc<Cell<Option<LayoutMode>>>,
) {
    let width = window.width();
    let next_layout_mode = layout_mode_for_width(width);
    if last_layout_mode.get() != Some(next_layout_mode) {
        last_layout_mode.set(Some(next_layout_mode));
        sender.input(AppMsg::WindowLayoutChanged(width));
    }
}

fn install_autosave_on_close(window: &gtk::Window, sender: ComponentSender<AppModel>) {
    let close_allowed = Rc::new(Cell::new(false));
    window.connect_close_request(clone!(
        #[strong]
        sender,
        #[strong]
        close_allowed,
        move |window| {
            if close_allowed.get() {
                return gtk::glib::Propagation::Proceed;
            }
            sender.input(AppMsg::CloseWithAutosave {
                window: window.clone(),
                close_allowed: close_allowed.clone(),
            });
            gtk::glib::Propagation::Stop
        }
    ));
}

fn install_preview_layout_watch(
    preview_picture: &gtk::Picture,
    tab: WorkflowTab,
    sender: ComponentSender<AppModel>,
) {
    let last_size = Rc::new(Cell::new(None));
    let probe_active = Rc::new(Cell::new(false));

    preview_picture.connect_map(clone!(
        #[strong]
        sender,
        #[strong]
        last_size,
        #[strong]
        probe_active,
        move |picture| {
            schedule_preview_layout_probe(picture, tab, &sender, &last_size, &probe_active)
        }
    ));
    preview_picture.connect_notify_local(
        Some("width"),
        clone!(
            #[strong]
            sender,
            #[strong]
            last_size,
            #[strong]
            probe_active,
            move |picture, _| {
                schedule_preview_layout_probe(picture, tab, &sender, &last_size, &probe_active)
            }
        ),
    );
    preview_picture.connect_notify_local(
        Some("height"),
        clone!(
            #[strong]
            sender,
            #[strong]
            last_size,
            #[strong]
            probe_active,
            move |picture, _| {
                schedule_preview_layout_probe(picture, tab, &sender, &last_size, &probe_active)
            }
        ),
    );
    preview_picture.connect_notify_local(
        Some("scale-factor"),
        clone!(
            #[strong]
            sender,
            #[strong]
            last_size,
            #[strong]
            probe_active,
            move |picture, _| {
                schedule_preview_layout_probe(picture, tab, &sender, &last_size, &probe_active)
            }
        ),
    );
}

fn schedule_preview_layout_probe(
    picture: &gtk::Picture,
    tab: WorkflowTab,
    sender: &ComponentSender<AppModel>,
    last_size: &Rc<Cell<Option<PreviewRenderSize>>>,
    probe_active: &Rc<Cell<bool>>,
) {
    if probe_active.get() {
        return;
    }

    probe_active.set(true);
    let stable_ticks = Rc::new(Cell::new(0_u8));
    picture.add_tick_callback(clone!(
        #[strong]
        sender,
        #[strong]
        last_size,
        #[strong]
        probe_active,
        #[strong]
        stable_ticks,
        move |picture, _| {
            let size_changed = send_preview_layout_change(picture, tab, &sender, &last_size);
            if size_changed {
                stable_ticks.set(0);
                return gtk::glib::ControlFlow::Continue;
            }

            let next_stable_ticks = stable_ticks.get().saturating_add(1);
            stable_ticks.set(next_stable_ticks);
            if next_stable_ticks >= 3 {
                probe_active.set(false);
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        }
    ));
}

fn set_button_icon(button: &gtk::Button, icon_name: &str) {
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
}

fn set_picture_from_path(picture: &gtk::Picture, path: Option<&Path>) {
    if let Some(texture) = path
        .filter(|path| path.is_file())
        .and_then(|path| gdk::Texture::from_file(&gio::File::for_path(path)).ok())
    {
        picture.set_paintable(Some(&texture));
        picture.set_visible(true);
    } else {
        picture.set_visible(false);
    }
}

fn set_picture_from_path_if_needed(
    picture: &gtk::Picture,
    path: Option<&Path>,
    cached_path: &mut Option<PathBuf>,
) {
    let next_path = path.map(Path::to_path_buf);
    if *cached_path != next_path {
        set_picture_from_path(picture, path);
        *cached_path = next_path;
    }
}

fn timeline_file_stamp(path: &Path) -> Option<(u64, u128)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let timestamp = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((metadata.len(), timestamp))
}

fn install_export_poll(
    sender: ComponentSender<AppModel>,
    active: Rc<Cell<bool>>,
    export_worker_state: Arc<Mutex<ExportWorkerState>>,
) {
    let last_version = Rc::new(Cell::new(0_u64));
    gtk::glib::timeout_add_local(
        Duration::from_millis(75),
        clone!(
            #[strong]
            sender,
            #[strong]
            active,
            #[strong]
            export_worker_state,
            #[strong]
            last_version,
            move || {
                if !active.get() {
                    return gtk::glib::ControlFlow::Break;
                }

                let snapshot = match export_worker_state.lock() {
                    Ok(state) => state.clone(),
                    Err(_) => {
                        sender.input(AppMsg::PollExportState);
                        active.set(false);
                        return gtk::glib::ControlFlow::Break;
                    }
                };

                let version = snapshot.version;
                if version != last_version.get() {
                    last_version.set(version);
                    sender.input(AppMsg::PollExportState);
                }

                gtk::glib::ControlFlow::Continue
            }
        ),
    );
}

fn send_preview_layout_change(
    widget: &impl IsA<gtk::Widget>,
    tab: WorkflowTab,
    sender: &ComponentSender<AppModel>,
    last_size: &Cell<Option<PreviewRenderSize>>,
) -> bool {
    if !widget.as_ref().is_visible() || !widget.as_ref().is_mapped() {
        return false;
    }
    if EXPORT_LAYOUT_WATCH_SUSPENDED.load(Ordering::Relaxed) {
        return false;
    }
    let size = preview_render_size_for_widget(widget);
    if last_size.get() != Some(size) {
        last_size.set(Some(size));
        sender
            .input_sender()
            .send(AppMsg::PreviewLayoutChanged { tab, size })
            .is_ok()
    } else {
        false
    }
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

fn preview_render_size_for_dimensions(
    dimensions: Option<(u32, u32)>,
    requested_size: PreviewRenderSize,
) -> PreviewRenderSize {
    match dimensions {
        Some((width, height)) => PreviewRenderSize {
            width: requested_size.width.min(width.max(1)),
            height: requested_size.height.min(height.max(1)),
        },
        None => requested_size,
    }
}

fn install_app_css(window: &gtk::Window) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        window.app-window,
        .app-window {
            background: #11161d;
        }
        .app-shell {
            background: #11161d;
            color: #ecf1f8;
        }
        .top-shell,
        .timeline-shell,
        .status-shell {
            background: #171d25;
            border: 1px solid rgba(91, 115, 150, 0.24);
            border-radius: 10px;
            padding: 12px;
            box-shadow: 0 10px 24px rgba(0, 0, 0, 0.20);
        }
        .top-shell {
            box-shadow: none;
        }
        .content-card {
            border-radius: 12px;
            background: #18202a;
            padding: 12px;
            box-shadow: 0 8px 18px rgba(0, 0, 0, 0.18);
        }
        .section-card {
            border-color: rgba(87, 116, 156, 0.36);
        }
        .section-header {
            margin-bottom: 4px;
        }
        .section-title {
            font-weight: 700;
            letter-spacing: 0.02em;
            color: #f4f7fb;
        }
        .section-toggle {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }
        .section-toggle:hover {
            background: transparent;
        }
        .section-chevron {
            color: #9fb6d5;
        }
        .collapsible-section > border {
            padding-top: 4px;
        }
        .workflow-tab,
        .pill-button,
        .choice-card {
            border-radius: 10px;
            background: #1b2b42;
            border: 1px solid #3c669e;
        }
        .workflow-tab-active {
            background: transparent;
            border-color: transparent;
            border-bottom: 3px solid #5a9bff;
            color: #7fb2ff;
            font-weight: 700;
        }
        .suggested-action {
            border-radius: 10px;
            background: #2d6cdf;
            border-color: #6fa7ff;
        }
        .pill-button-active,
        .choice-card-active {
            background: #1b2b42;
            border-color: #4f8fe6;
            color: #eef6ff;
        }
        .choice-card {
            padding: 0;
            min-height: 112px;
            min-width: 72px;
        }
        .choice-card-active {
            box-shadow: inset 0 0 0 1px #5a9bff;
        }
        .edit-quick-action {
            min-height: 76px;
            padding: 14px 18px;
        }
        .page-heading {
            margin-bottom: 2px;
        }
        .page-heading-title {
            font-weight: 800;
            letter-spacing: 0.01em;
        }
        .choice-card label,
        .workflow-tab label,
        .pill-button label,
        .status-pill {
            color: #ecf1f8;
        }
        .button-leading-icon {
            -gtk-icon-size: 18px;
            color: #8cc3ff;
        }
        .choice-card-icon {
            -gtk-icon-size: 38px;
            margin-bottom: 2px;
        }
        .icon-tone-cyan {
            color: #61d0ff;
        }
        .icon-tone-amber {
            color: #ffbf57;
        }
        .icon-tone-green {
            color: #67dc8b;
        }
        .icon-tone-coral {
            color: #ff8b7a;
        }
        .workflow-tab-active .button-leading-icon,
        .pill-button-active .button-leading-icon,
        .choice-card-active .button-leading-icon {
            color: #7fb2ff;
        }
        .preview-image,
        .timeline-picture {
            border-radius: 10px;
            background: #10151c;
        }
        .export-preview-fixed {
            min-height: 225px;
        }
        .export-summary-action-row {
            margin-top: 2px;
        }
        .settings-row {
            padding: 12px 0;
            border-bottom: 1px solid rgba(91, 115, 150, 0.18);
        }
        .settings-copy {
            margin-top: 0;
            margin-bottom: 0;
        }
        .settings-control {
            min-width: 0;
        }
        .settings-title {
            font-weight: 700;
            color: #f2f6fb;
        }
        .control-surface,
        entry,
        spinbutton,
        combobox,
        switch {
            color: #eaf2ff;
        }
        entry,
        spinbutton,
        combobox box button,
        combobox box.linked button,
        switch {
            background: #16263b;
            border: 1px solid #3c669e;
        }
        scale trough {
            background: #16263b;
            border: 1px solid #3c669e;
            min-height: 8px;
            border-radius: 999px;
        }
        scale highlight {
            background: #5a9bff;
            border-radius: 999px;
        }
        scale slider {
            background: #eef5ff;
            border: 1px solid #5a9bff;
        }
        switch:checked {
            background: #2d6cdf;
            border-color: #6fa7ff;
        }
        switch slider {
            background: #eef5ff;
        }
        .combined-input entry {
            border-top-right-radius: 0;
            border-bottom-right-radius: 0;
        }
        .combined-input button {
            border-top-left-radius: 0;
            border-bottom-left-radius: 0;
        }
        .menu-action {
            border-radius: 10px;
            background: #1b2b42;
            border: 1px solid #3c669e;
        }
        .dim-label {
            color: #9da9ba;
        }
        .summary-blurb {
            background: #202a36;
            border: 1px solid rgba(99, 131, 175, 0.24);
            border-radius: 10px;
            padding: 12px 14px;
            color: #cfd9e7;
        }
        .summary-key {
            color: #9da9ba;
        }
        .summary-value {
            color: #f2f6fb;
            font-weight: 600;
        }
        .export-summary-grid {
            margin-top: 2px;
        }
        .section-without-header {
            padding-top: 4px;
        }
        .export-action-title {
            font-size: 18px;
            font-weight: 800;
        }
        .export-action-detail {
            color: #c8d7ea;
        }
        .export-action-icon {
            -gtk-icon-size: 22px;
        }
        .export-action-info {
            color: #7fb2ff;
        }
        .export-action-success {
            color: #67dc8b;
        }
        .export-action-warning {
            color: #ffbf57;
        }
        .export-action-error {
            color: #ff8b7a;
        }
        .status-pill {
            background: #1a5c35;
            border-radius: 999px;
            padding: 6px 10px;
        }
        .timeline-tile {
            border-radius: 10px;
            padding: 6px 6px 4px;
            border: 2px solid transparent;
            background: #1b222c;
        }
        .timeline-tile-selected {
            background: #1b2b42;
            border-color: #4f8fe6;
            color: white;
        }
        .timeline-tile-selected label {
            color: white;
        }
        .timeline-drop-before {
            border-left-color: #4f8fe6;
            box-shadow: inset 4px 0 0 #2469d9;
        }
        .timeline-drop-after {
            border-right-color: #4f8fe6;
            box-shadow: inset -4px 0 0 #2469d9;
        }
        .timeline-badge,
        .timeline-check {
            background: #223148;
            border-radius: 999px;
            padding: 1px 7px;
            color: #d8e6f8;
            font-weight: 700;
        }
        .timeline-check {
            background: #2d6cdf;
            color: white;
        }
        .timeline-filename {
            color: #8694a7;
            font-size: 12px;
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

fn frame_effective_dimensions(frame: &FrameItem) -> Option<(u32, u32)> {
    let rotated = rotated_dimensions(
        frame.source_dimensions?,
        frame.transform_spec.rotate_quarter_turns,
    );
    let cropped = frame
        .transform_spec
        .crop
        .map(|crop| (crop.width, crop.height))
        .unwrap_or(rotated);
    Some(
        frame
            .transform_spec
            .resize
            .map(|resize| (resize.width, resize.height))
            .unwrap_or(cropped),
    )
}

fn scale_dimension_by_ratio(value: u32, numerator: u32, denominator: u32) -> Option<u32> {
    if value == 0 || numerator == 0 || denominator == 0 {
        return None;
    }
    let scaled = ((u128::from(value) * u128::from(numerator)) + (u128::from(denominator) / 2))
        / u128::from(denominator);
    Some(scaled.clamp(1, 8192) as u32)
}

fn scaled_resize_dimension(base: u32, multiplier: f64) -> u32 {
    ((f64::from(base.max(1)) * multiplier).round() as u32).clamp(1, 8192)
}

fn crop_rect_for_frame(
    frame: &FrameItem,
    preset: CropPreset,
    anchor: CropAnchor,
) -> Option<CropRect> {
    let (full_width, full_height) = rotated_dimensions(
        frame.source_dimensions?,
        frame.transform_spec.rotate_quarter_turns,
    );
    let base = frame.transform_spec.crop.unwrap_or(CropRect {
        x: 0,
        y: 0,
        width: full_width,
        height: full_height,
    });
    let (ratio_width, ratio_height) = crop_preset_ratio(preset);
    let base_width = u128::from(base.width.max(1));
    let base_height = u128::from(base.height.max(1));
    let ratio_width = u128::from(ratio_width);
    let ratio_height = u128::from(ratio_height);

    let (target_width, target_height) = if base_width * ratio_height >= base_height * ratio_width {
        let width = ((base_height * ratio_width) / ratio_height) as u32;
        (width.max(1), base.height.max(1))
    } else {
        let height = ((base_width * ratio_height) / ratio_width) as u32;
        (base.width.max(1), height.max(1))
    };

    let max_x = base.width.saturating_sub(target_width);
    let max_y = base.height.saturating_sub(target_height);
    let offset_x = crop_anchor_offset(max_x, anchor);
    let offset_y = crop_anchor_offset(max_y, anchor);

    let crop = CropRect {
        x: base.x.saturating_add(offset_x),
        y: base.y.saturating_add(offset_y),
        width: target_width,
        height: target_height,
    };

    if frame.transform_spec.crop.is_none()
        && crop.x == 0
        && crop.y == 0
        && crop.width == full_width
        && crop.height == full_height
    {
        None
    } else {
        Some(crop)
    }
}

fn crop_preset_ratio(preset: CropPreset) -> (u32, u32) {
    match preset {
        CropPreset::Square => (1, 1),
        CropPreset::Landscape16x9 => (16, 9),
        CropPreset::Portrait9x16 => (9, 16),
    }
}

fn rotated_dimensions(dimensions: (u32, u32), turns: i32) -> (u32, u32) {
    if turns.rem_euclid(4) % 2 == 1 {
        (dimensions.1, dimensions.0)
    } else {
        dimensions
    }
}

fn crop_anchor_offset(available: u32, anchor: CropAnchor) -> u32 {
    match anchor {
        CropAnchor::Start => 0,
        CropAnchor::Center => available / 2,
        CropAnchor::End => available,
    }
}

fn set_spin_if_needed(spin: &gtk::SpinButton, value: f64) {
    if (spin.value() - value).abs() > f64::EPSILON {
        spin.set_value(value);
    }
}

fn set_scale_if_needed(scale: &gtk::Scale, value: f64) {
    if (scale.value() - value).abs() > f64::EPSILON {
        scale.set_value(value);
    }
}

fn set_check_if_needed(check: &gtk::CheckButton, value: bool) {
    if check.is_active() != value {
        check.set_active(value);
    }
}

fn set_switch_if_needed(switch: &gtk::Switch, value: bool) {
    if switch.is_active() != value {
        switch.set_active(value);
    }
}

fn set_visible_if_needed(widget: &impl IsA<gtk::Widget>, visible: bool) {
    if widget.as_ref().is_visible() != visible {
        widget.as_ref().set_visible(visible);
    }
}

fn set_stack_visible_child_name_if_needed(stack: &gtk::Stack, name: &str) {
    if stack.visible_child_name().as_deref() != Some(name) {
        stack.set_visible_child_name(name);
    }
}

fn set_box_orientation_if_needed(box_widget: &gtk::Box, orientation: gtk::Orientation) {
    if box_widget.orientation() != orientation {
        box_widget.set_orientation(orientation);
    }
}

fn set_hexpand_if_needed(widget: &impl IsA<gtk::Widget>, expands: bool) {
    if widget.as_ref().hexpands() != expands {
        widget.as_ref().set_hexpand(expands);
    }
}

fn set_width_request_if_needed(widget: &impl IsA<gtk::Widget>, value: i32) {
    if widget.as_ref().width_request() != value {
        widget.as_ref().set_width_request(value);
    }
}

fn sync_workspace_split_widths(
    workspace_box: &gtk::Box,
    preview_panel: &gtk::Box,
    content_stack: &gtk::Stack,
) {
    let width = workspace_box.width();
    let compact = layout_mode_for_width(width) == LayoutMode::Compact;
    let preview_visible = preview_panel.is_visible();
    set_hexpand_if_needed(content_stack, compact || !preview_visible);

    if compact || !preview_visible || width <= 0 {
        set_width_request_if_needed(content_stack, -1);
        return;
    }

    let available = (width - workspace_box.spacing()).max(0);
    let min_preview_width = 360;
    let max_controls_width = (available - min_preview_width).max(320);
    let controls_width = (available / 3).clamp(320, max_controls_width);
    set_width_request_if_needed(content_stack, controls_width);
}

fn set_size_request_if_needed(widget: &impl IsA<gtk::Widget>, width: i32, height: i32) {
    if widget.as_ref().width_request() != width || widget.as_ref().height_request() != height {
        widget.as_ref().set_size_request(width, height);
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
                if let Some(item) = files.item(index)
                    && let Ok(file) = item.downcast::<gio::File>()
                    && let Some(path) = file.path()
                {
                    paths.push(path);
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
        if response == gtk::ResponseType::Accept
            && let Some(file) = dialog.file().and_then(|file| file.path())
        {
            sender.input(AppMsg::OpenProject(file));
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
        if response == gtk::ResponseType::Accept
            && let Some(file) = dialog.file().and_then(|file| file.path())
        {
            sender.input(AppMsg::SaveProject(file));
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
        if response == gtk::ResponseType::Accept
            && let Some(file) = dialog.file().and_then(|file| file.path())
        {
            sender.input(AppMsg::ChooseOutputPath(file));
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
    _playback_active: bool,
) -> PathBuf {
    if let Some(cached_preview_path) = cached_preview_path {
        return cached_preview_path;
    }

    if let Some(current_preview_path) = current_preview_path {
        return current_preview_path.clone();
    }

    frame.source_path.clone()
}

fn usable_preview_path(preview_path: Option<PathBuf>) -> Option<PathBuf> {
    preview_path.filter(|path| path.is_file())
}

fn preview_path_is_proxy(frame: &FrameItem, path: &Path) -> bool {
    path == frame.source_path
        || frame
            .thumbnail_path
            .as_ref()
            .is_some_and(|thumbnail_path| path == thumbnail_path)
}

fn should_refresh_preview(
    rendered_size: Option<PreviewRenderSize>,
    target_size: PreviewRenderSize,
) -> bool {
    rendered_size.is_none_or(|rendered_size| !rendered_size.covers(target_size))
}

fn preview_result_is_usable(current_frame_id: Option<u64>, frame_id: u64) -> bool {
    current_frame_id == Some(frame_id)
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
        CropAnchor, CropPreset, PreviewRenderSize, crop_rect_for_frame, following_frame_id,
        frame_effective_dimensions, immediate_preview_path, playback_start_frame_id,
        preview_render_size_for_dimensions, preview_render_size_from_values,
        preview_result_is_usable, scale_dimension_by_ratio, scaled_resize_dimension,
        should_refresh_preview, step_frame_id, timeline_tile_picture_size, usable_preview_path,
    };
    use crate::types::{CropRect, FitMode, FrameItem, ResizeTarget, TransformSpec};

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
    fn immediate_preview_uses_source_before_thumbnail_when_not_playing() {
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
            PathBuf::from("source.png")
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
    fn immediate_preview_keeps_current_render_before_proxy() {
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
            immediate_preview_path(
                &frame,
                None,
                Some(&PathBuf::from("existing-preview.png")),
                false,
            ),
            PathBuf::from("existing-preview.png")
        );
    }

    #[test]
    fn immediate_preview_keeps_current_render_during_playback() {
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
    fn immediate_preview_uses_source_before_thumbnail_during_playback() {
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
    fn immediate_preview_falls_back_to_source_when_thumbnail_is_unavailable() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec::default(),
            thumbnail_path: None,
            enabled: true,
            source_dimensions: None,
        };

        assert_eq!(
            immediate_preview_path(&frame, None, None, true),
            PathBuf::from("source.png")
        );
    }

    #[test]
    fn immediate_preview_keeps_current_render_for_transformed_playback_frame() {
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
    fn immediate_preview_keeps_existing_preview_for_transformed_playback_without_thumbnail() {
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
            thumbnail_path: None,
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
        assert!(preview_result_is_usable(Some(5), 5));
        assert!(!preview_result_is_usable(Some(5), 6));
        assert!(!preview_result_is_usable(None, 5));
    }

    #[test]
    fn usable_preview_path_rejects_missing_files() {
        assert_eq!(
            usable_preview_path(Some(PathBuf::from("definitely-missing-preview.png"))),
            None
        );
    }

    #[test]
    fn quick_crop_centers_square_within_landscape_frame() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec::default(),
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((1920, 1080)),
        };

        assert_eq!(
            crop_rect_for_frame(&frame, CropPreset::Square, CropAnchor::Center),
            Some(CropRect {
                x: 420,
                y: 0,
                width: 1080,
                height: 1080,
            })
        );
    }

    #[test]
    fn quick_crop_respects_existing_crop_and_end_anchor() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec {
                crop: Some(CropRect {
                    x: 100,
                    y: 200,
                    width: 1000,
                    height: 1000,
                }),
                ..TransformSpec::default()
            },
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((1920, 1080)),
        };

        assert_eq!(
            crop_rect_for_frame(&frame, CropPreset::Portrait9x16, CropAnchor::End),
            Some(CropRect {
                x: 538,
                y: 200,
                width: 562,
                height: 1000,
            })
        );
    }

    #[test]
    fn frame_effective_dimensions_follow_rotation_crop_and_resize() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec {
                rotate_quarter_turns: 1,
                flip_horizontal: false,
                flip_vertical: false,
                crop: Some(CropRect {
                    x: 10,
                    y: 20,
                    width: 900,
                    height: 600,
                }),
                resize: Some(ResizeTarget {
                    width: 450,
                    height: 300,
                }),
                fit_mode: FitMode::Contain,
            },
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((1920, 1080)),
        };

        assert_eq!(frame_effective_dimensions(&frame), Some((450, 300)));
    }

    #[test]
    fn resize_scaling_helpers_preserve_aspect_ratio() {
        assert_eq!(scale_dimension_by_ratio(400, 9, 16), Some(225));
        assert_eq!(scaled_resize_dimension(960, 1.5), 1440);
    }

    #[test]
    fn preview_render_size_clamps_to_frame_dimensions() {
        assert_eq!(
            preview_render_size_for_dimensions(
                Some((640, 360)),
                PreviewRenderSize {
                    width: 1280,
                    height: 720,
                },
            ),
            PreviewRenderSize {
                width: 640,
                height: 360,
            }
        );
    }

    #[test]
    fn timeline_tile_picture_size_uses_effective_dimensions() {
        let frame = FrameItem {
            id: 1,
            source_path: PathBuf::from("source.png"),
            duration_ms: 100,
            transform_spec: TransformSpec {
                resize: Some(ResizeTarget {
                    width: 300,
                    height: 600,
                }),
                ..TransformSpec::default()
            },
            thumbnail_path: None,
            enabled: true,
            source_dimensions: Some((1920, 1080)),
        };

        let (width, height) = timeline_tile_picture_size(&frame);
        assert_eq!(height, 78);
        assert!(width < 78);
    }
}
