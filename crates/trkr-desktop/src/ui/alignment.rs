//! Alignment controls rendered in the central work area.
//!
//! This view deliberately owns no transport.  Every movement is represented as
//! an engine command, so the regular worker retains exclusive serial access.

use super::*;
use crate::alignment_config::{AlignmentLayout, Panel};
use crate::{
    engine::{AlignmentAction, AlignmentUnit, Command, DeviceState, MotionKind},
    inventory::{DelayEntry, DeviceIds, Inventory, LockinEntry, ScannerEntry},
};
use trkr_motion::{DelayConfig, Position, ScannerConfig};

const SURFACE: Color32 = Color32::from_rgb(26, 27, 32);
const BORDER: Color32 = Color32::from_rgb(56, 58, 67);
const MUTED: Color32 = Color32::from_rgb(153, 157, 168);
const ACCENT: Color32 = Color32::from_rgb(65, 129, 229);
const READY: Color32 = Color32::from_rgb(93, 196, 146);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum AlignmentPage {
    #[default]
    Delay,
    Scanner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HeldControl {
    kind: MotionKind,
    id: String,
    axis: usize,
    positive: bool,
}

const HOLD_DELAY_S: f64 = 0.35;

#[derive(Clone, Debug)]
struct PressedControl {
    widget: egui::Id,
    control: HeldControl,
    since: f64,
    amount: f64,
    unit: AlignmentUnit,
    hold_started: bool,
}

#[derive(Clone, Debug)]
struct OptionsTarget {
    kind: MotionKind,
    id: String,
}

struct OptionsAxis {
    axis: usize,
    label: &'static str,
    direction: i32,
    home: bool,
}

fn scanner_options_axes(entry: &ScannerEntry) -> Vec<OptionsAxis> {
    [(1, "X direction", &entry.x), (2, "Y direction", &entry.y)]
        .into_iter()
        .filter_map(|(axis, label, config)| {
            config.as_ref().map(|config| OptionsAxis {
                axis,
                label,
                direction: config.direction,
                home: devices::scanner_initialization_homes(&config.controller),
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct InitializeTarget {
    kind: MotionKind,
    id: String,
    axis: usize,
    name: String,
    default_home: bool,
}

/// UI-only state.  Device IDs remain stable even when a user changes their
/// display name or reorders the inventory.
pub struct AlignmentUi {
    page: AlignmentPage,
    layout: AlignmentLayout,
    delete_panel: Option<(MotionKind, String)>,
    pressed: Option<PressedControl>,
    options: Option<OptionsTarget>,
    held: Option<HeldControl>,
    initialize: Option<InitializeTarget>,
    skip_home: bool,
    move_to_coordinate_zero: bool,
    preview: bool,
}

impl Default for AlignmentUi {
    fn default() -> Self {
        Self {
            page: AlignmentPage::Delay,
            layout: AlignmentLayout::default(),
            delete_panel: None,
            pressed: None,
            options: None,
            held: None,
            initialize: None,
            skip_home: false,
            move_to_coordinate_zero: false,
            preview: false,
        }
    }
}

impl AlignmentUi {
    #[cfg(test)]
    pub(super) fn test_start_hold(&mut self) {
        self.held = Some(HeldControl {
            kind: MotionKind::Delay,
            id: "test-delay".into(),
            axis: 0,
            positive: true,
        });
    }

    #[cfg(test)]
    pub(super) fn test_holding(&self) -> bool {
        self.held.is_some()
    }

    pub(super) fn highlights(&self, kind: &str, id: &str) -> bool {
        let panels = match (self.page, kind) {
            (AlignmentPage::Delay, "delay") => &self.layout.delays,
            (AlignmentPage::Scanner, "scanner") => &self.layout.scanners,
            _ => return false,
        };
        panels
            .iter()
            .any(|panel| panel.device_id.as_deref() == Some(id))
    }
}

fn panel_options(available: &[DeviceOption], panels: &[Panel], slot: usize) -> Vec<DeviceOption> {
    available
        .iter()
        .filter(|option| {
            !panels.iter().enumerate().any(|(other, panel)| {
                other != slot && panel.device_id.as_deref() == Some(option.id.as_str())
            })
        })
        .cloned()
        .collect()
}

#[derive(Clone)]
struct DeviceOption {
    id: String,
    name: String,
}

#[derive(Clone)]
struct DelayPanel {
    option: DeviceOption,
    entry: DelayEntry,
    state: Option<DeviceState>,
}

#[derive(Clone)]
struct ScannerPanel {
    option: DeviceOption,
    entry: ScannerEntry,
    state: Option<DeviceState>,
}

enum Intent {
    Command {
        kind: MotionKind,
        id: String,
        action: AlignmentAction,
    },
    Hold {
        control: HeldControl,
        amount: f64,
        unit: AlignmentUnit,
    },
    Initialize(InitializeTarget),
    Options(OptionsTarget),
    Reverse {
        kind: MotionKind,
        id: String,
        axis: usize,
    },
    DeletePanel {
        kind: MotionKind,
        id: String,
    },
    Stop,
}

/// Shared mutable context for one alignment frame. Grouping this state keeps
/// card renderers focused on their layout instead of threading UI ownership,
/// intent collection, and hold tracking through every control.
struct RenderState<'a> {
    ui: &'a mut egui::Ui,
    alignment: &'a mut AlignmentUi,
    locked: bool,
    intents: &'a mut Vec<Intent>,
    held_down: &'a mut bool,
}

struct StepControl<'a> {
    position: egui::Pos2,
    size: Vec2,
    kind: MotionKind,
    id: &'a str,
    axis: usize,
    amount: f64,
    unit: AlignmentUnit,
    positive: bool,
    locked: bool,
}

impl App {
    pub(super) fn hydrate_alignment(&mut self) {
        match AlignmentLayout::from_profile(&self.settings.raw, &self.inventory) {
            Ok(layout) => self.alignment.layout = layout,
            Err(error) => {
                self.alignment.layout = AlignmentLayout::default();
                self.note(error);
            }
        }
    }
    pub(super) fn sync_alignment(&mut self) -> Result<(), String> {
        self.alignment.layout.validate(&self.inventory)?;
        self.settings.raw["alignment"] = self.alignment.layout.value();
        Ok(())
    }
    pub(super) fn alignment_layout_value(&mut self) -> Result<Value, String> {
        self.sync_alignment()?;
        Ok(self.alignment.layout.value())
    }
    pub(super) fn set_alignment_layout_value(&mut self, value: &Value) -> Result<Value, String> {
        let layout: AlignmentLayout =
            serde_json::from_value(value.clone()).map_err(|error| format!("Alignment: {error}"))?;
        layout.validate(&self.inventory)?;
        self.alignment_cancel_hold();
        self.alignment.options = None;
        self.alignment.initialize = None;
        self.alignment.delete_panel = None;
        self.settings.raw["alignment"] = layout.value();
        self.alignment.layout = layout;
        Ok(self.alignment.layout.value())
    }

    fn alignment_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some((kind, id)) = self.alignment.delete_panel.clone() else {
            return;
        };
        let editable = !self.demo_viewer
            && !self.snapshot.running
            && !self.controller.has_pending()
            && !self.python_locked();
        let mut remove = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("delete-alignment-panel")).show(ctx, |ui| {
            ui.set_width(300.);
            ui.heading("Delete alignment panel?");
            ui.label("The device remains registered. Its connection is unchanged.");
            ui.add_space(10.);
            ui.horizontal(|ui| {
                cancel = ui.button("Cancel").clicked();
                remove = ui
                    .add_enabled(editable, egui::Button::new("Delete"))
                    .clicked();
            });
        });
        if remove {
            self.alignment.layout.remove(kind, &id);
            self.alignment.delete_panel = None;
        } else if cancel || response.should_close() {
            self.alignment.delete_panel = None;
        }
    }

    /// The root layout calls this instead of the plot area while Alignment is
    /// selected.  It intentionally keeps all configuration and device cards
    /// in their normal left/right panels.
    pub(super) fn alignment_view(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let locked = !self.inventory_editable()
            || self.alignment.options.is_some()
            || self.alignment.initialize.is_some()
            || self.alignment.delete_panel.is_some();
        let previous_layout = self.alignment.layout.clone();
        let delay_options: Vec<_> = self
            .inventory
            .delays
            .iter()
            .map(|entry| DeviceOption {
                id: entry.id.clone(),
                name: entry.display_name(),
            })
            .collect();
        let scanner_options: Vec<_> = self
            .inventory
            .scanners
            .iter()
            .map(|entry| DeviceOption {
                id: entry.id.clone(),
                name: entry.display_name(),
            })
            .collect();
        let delays: Vec<_> = self
            .inventory
            .delays
            .iter()
            .filter(|entry| {
                self.alignment.page == AlignmentPage::Delay
                    && self.alignment.highlights("delay", &entry.id)
            })
            .map(|entry| DelayPanel {
                option: DeviceOption {
                    id: entry.id.clone(),
                    name: entry.display_name(),
                },
                entry: entry.clone(),
                state: self.device_state(MotionKind::Delay, &entry.id),
            })
            .collect();
        let scanners: Vec<_> = self
            .inventory
            .scanners
            .iter()
            .filter(|entry| {
                self.alignment.page == AlignmentPage::Scanner
                    && self.alignment.highlights("scanner", &entry.id)
            })
            .map(|entry| ScannerPanel {
                option: DeviceOption {
                    id: entry.id.clone(),
                    name: entry.display_name(),
                },
                entry: entry.clone(),
                state: self.device_state(MotionKind::Scanner, &entry.id),
            })
            .collect();

        precision::record(&mut self.geometry, "alignment_widget", area);
        ui.painter().rect(
            area,
            10.,
            SURFACE,
            Stroke::new(1_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        let mut intents = Vec::new();
        let mut held_down = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            if self.alignment.preview {
                label(
                    ui,
                    area.min + Vec2::new(200., 14.),
                    "PREVIEW — synthetic positions · no hardware",
                    Color32::from_rgb(237, 174, 74),
                );
            }
            let tabs =
                egui::Rect::from_min_size(area.min + Vec2::new(10., 8.), Vec2::new(180., 26.));
            for (page, label, offset) in [
                (AlignmentPage::Delay, "Delay stages", 0.),
                (AlignmentPage::Scanner, "Scanners", 89.),
            ] {
                let rect = egui::Rect::from_min_size(
                    tabs.min + Vec2::new(offset, 0.),
                    Vec2::new(85., 26.),
                );
                if ui
                    .put(
                        rect,
                        egui::Button::new(label)
                            .selected(self.alignment.page == page)
                            .min_size(rect.size())
                            .corner_radius(6),
                    )
                    .clicked()
                {
                    self.alignment.page = page;
                }
            }
            let kind = if self.alignment.page == AlignmentPage::Delay {
                MotionKind::Delay
            } else {
                MotionKind::Scanner
            };
            let can_add =
                !locked && self.alignment.layout.panels(kind).len() < AlignmentLayout::limit(kind);
            if button(
                ui,
                egui::Rect::from_min_size(
                    area.right_top() + Vec2::new(-112., 8.),
                    Vec2::new(100., 26.),
                ),
                "+ Add panel",
                can_add,
            )
            .clicked()
                && let Err(error) = self.alignment.layout.add(kind)
            {
                self.note(error);
            }
            let content = egui::Rect::from_min_max(
                area.min + Vec2::new(10., 42.),
                area.max - Vec2::new(10., 10.),
            );
            let page = self.alignment.page;
            ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(content));
                ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                egui::ScrollArea::vertical()
                    .id_salt("alignment-panels")
                    .max_height(content.height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let content = egui::Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(ui.available_width(), content.height()),
                        );
                        let mut render = RenderState {
                            ui,
                            alignment: &mut self.alignment,
                            locked,
                            intents: &mut intents,
                            held_down: &mut held_down,
                        };
                        match page {
                            AlignmentPage::Delay => {
                                let gap = 8.;
                                let mut y = content.min.y;
                                for slot in 0..render.alignment.layout.delays.len() {
                                    let height = 216.;
                                    let rect = egui::Rect::from_min_size(
                                        egui::pos2(content.min.x, y),
                                        Vec2::new(content.width(), height),
                                    );
                                    let id = render.alignment.layout.delays[slot].device_id.clone();
                                    let panel = id.as_deref().and_then(|id| {
                                        delays.iter().find(|panel| panel.option.id == id)
                                    });
                                    draw_delay_card(&mut render, rect, slot, &delay_options, panel);
                                    render.ui.allocate_rect(rect, egui::Sense::hover());
                                    y += height + gap;
                                }
                            }
                            AlignmentPage::Scanner => {
                                let gap = 8.;
                                let width = ((content.width() - gap) * 0.5).max(240.);
                                let mut y = content.min.y;
                                for row in 0..render.alignment.layout.scanners.len().div_ceil(2) {
                                    let first = row * 2;
                                    let height = 282.;
                                    for column in 0..2 {
                                        let slot = first + column;
                                        if slot >= render.alignment.layout.scanners.len() {
                                            break;
                                        }
                                        let rect = egui::Rect::from_min_size(
                                            egui::pos2(
                                                content.min.x + column as f32 * (width + gap),
                                                y,
                                            ),
                                            Vec2::new(width, height),
                                        );
                                        let id = render.alignment.layout.scanners[slot]
                                            .device_id
                                            .clone();
                                        let panel = id.as_deref().and_then(|id| {
                                            scanners.iter().find(|panel| panel.option.id == id)
                                        });
                                        draw_scanner_card(
                                            &mut render,
                                            rect,
                                            slot,
                                            &scanner_options,
                                            panel,
                                        );
                                        render.ui.allocate_rect(rect, egui::Sense::hover());
                                    }
                                    y += height + gap;
                                }
                            }
                        }
                    });
            });
        });

        let focused = ui.ctx().input(|input| input.focused);
        if self.alignment.held.is_some() && (!held_down || !focused) {
            self.alignment_cancel_hold();
        }
        if held_down && focused && self.alignment.held.is_some() {
            self.controller.renew_hold();
            ui.ctx().request_repaint_after(Duration::from_millis(20));
        }
        if !focused || !ui.ctx().input(|input| input.pointer.primary_down()) {
            self.alignment.pressed = None;
        }
        self.apply_alignment_intents(intents);
        self.alignment_options_dialog(ui.ctx());
        self.alignment_initialize_dialog(ui.ctx());
        self.alignment_delete_dialog(ui.ctx());
        if self.alignment.layout != previous_layout {
            self.settings.raw["alignment"] = self.alignment.layout.value();
        }
    }

    /// Called on tab changes, window focus loss, close, and pointer release.
    pub(super) fn alignment_cancel_hold(&mut self) {
        self.alignment.pressed = None;
        if self.alignment.held.take().is_some() {
            self.controller.release_hold();
            self.note("Alignment hold stopped");
        }
    }

    /// Populate the central alignment page for screenshot/layout capture.
    /// This turns on the existing demo guard before installing any synthetic
    /// state, so none of the displayed controls can enqueue hardware I/O.
    pub(super) fn enable_alignment_preview(&mut self, scanner: bool) {
        self.demo_viewer = true;
        self.source_demo = true;
        self.alignment_tab = true;
        self.alignment.page = if scanner {
            AlignmentPage::Scanner
        } else {
            AlignmentPage::Delay
        };
        self.alignment.preview = true;
        self.alignment.held = None;
        self.alignment.initialize = None;

        let delays: Vec<_> = (1..=2)
            .map(|index| {
                let config = DelayConfig {
                    stage: Some("SGSP46-500".into()),
                    port: format!("PREVIEW-D{index}"),
                    travel_mm: Some(500.),
                    zero_pos_mm: Some(250.),
                    direction: 1,
                    ..DelayConfig::default()
                };
                DelayEntry::new(
                    format!("delay-{index}"),
                    format!("Delay {index}"),
                    config,
                    12.5 * index as f64,
                )
            })
            .collect();
        let scanners: Vec<_> = (1..=4)
            .map(|index| {
                let x = preview_scanner(format!("PREVIEW-X{index}"), 6.0, 582.);
                let y = preview_scanner(format!("PREVIEW-Y{index}"), 6.0, 412.);
                ScannerEntry::new(
                    format!("scanner-{index}"),
                    format!("Scanner {index}"),
                    Some(x),
                    Some(y),
                    [index as f64 * 8., -(index as f64 * 6.)],
                )
            })
            .collect();
        self.inventory = Inventory {
            lockins: vec![LockinEntry::new("lockin-1", "Lock-in 1", Model::Sr7265, 12)],
            delays,
            scanners,
            power_meters: Vec::new(),
            selected: DeviceIds::default(),
            panel_order: Vec::new(),
        };
        self.inventory
            .project_selected(&mut self.settings.raw)
            .expect("valid preview inventory");
        self.settings
            .raw
            .as_object_mut()
            .unwrap()
            .remove("alignment");
        self.hydrate();
        self.snapshot.running = false;
        self.snapshot.phase = "PREVIEW — no hardware".into();
        self.snapshot.registered_devices = preview_device_states(&self.inventory);
    }

    pub(super) fn enable_alignment_options_preview(&mut self) {
        if self.alignment.page == AlignmentPage::Scanner {
            // Show both states in the no-I/O Options fixture. Use the same
            // coordinate-preserving transform as the worker acknowledgement.
            let raw = self
                .device_state(MotionKind::Scanner, "scanner-1")
                .and_then(|state| state.position.x_scanner_mm)
                .expect("preview scanner position");
            let x = self.inventory.scanners[0]
                .x
                .as_ref()
                .unwrap()
                .reversed_preserving_display_at_raw(raw)
                .unwrap();
            self.apply_direction(
                MotionKind::Scanner,
                "scanner-1",
                trkr_motion::MotionConfig {
                    delay: None,
                    x: Some(x),
                    y: None,
                },
            );
        }
        self.alignment.options = Some(match self.alignment.page {
            AlignmentPage::Delay => OptionsTarget {
                kind: MotionKind::Delay,
                id: "delay-1".into(),
            },
            AlignmentPage::Scanner => OptionsTarget {
                kind: MotionKind::Scanner,
                id: "scanner-1".into(),
            },
        });
    }

    pub(super) fn device_state(&self, kind: MotionKind, id: &str) -> Option<DeviceState> {
        let kind = match kind {
            MotionKind::Delay => "delay",
            MotionKind::Scanner => "scanner",
        };
        self.snapshot
            .registered_devices
            .iter()
            .find(|state| state.kind == kind && state.id == id)
            .cloned()
    }

    fn apply_alignment_intents(&mut self, intents: Vec<Intent>) {
        for intent in intents {
            match intent {
                Intent::Command { kind, id, action } => {
                    if let Err(error) =
                        self.controller
                            .send(Command::Alignment { kind, id, action })
                    {
                        self.note(error);
                    }
                }
                Intent::Hold {
                    control,
                    amount,
                    unit,
                } => {
                    if self.alignment.held.is_none() {
                        let action = AlignmentAction::Jog {
                            axis: control.axis,
                            amount: if control.positive {
                                amount.abs()
                            } else {
                                -amount.abs()
                            },
                            unit,
                        };
                        // The worker can begin before the command call returns.
                        // Renew first so the first bounded jog sees a valid lease.
                        self.controller.renew_hold();
                        match self.controller.send(Command::Alignment {
                            kind: control.kind,
                            id: control.id.clone(),
                            action,
                        }) {
                            Ok(()) => {
                                self.alignment.held = Some(control);
                            }
                            Err(error) => {
                                self.controller.clear_hold_lease();
                                self.note(error)
                            }
                        }
                    }
                }
                Intent::Options(target) => {
                    self.alignment_cancel_hold();
                    self.alignment.options = Some(target);
                }
                Intent::Initialize(target) => {
                    self.alignment_cancel_hold();
                    self.alignment.options = None;
                    self.alignment.initialize = Some(target);
                    self.alignment.skip_home = false;
                    self.alignment.move_to_coordinate_zero = true;
                }
                Intent::Reverse { kind, id, axis } => {
                    if let Err(error) = self.reverse_device_direction(kind, &id, axis) {
                        self.note(error);
                    }
                }
                Intent::DeletePanel { kind, id } => {
                    self.alignment_cancel_hold();
                    self.alignment.delete_panel = Some((kind, id));
                }
                Intent::Stop => {
                    self.alignment.pressed = None;
                    self.alignment.held.take();
                    // Unlike tab/focus cleanup, an explicit Stop must cancel
                    // an ordinary Step or End operation as well as a hold.
                    self.controller.release_hold();
                    self.note("Alignment stop requested");
                }
            }
        }
    }

    fn alignment_options_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.alignment.options.clone() else {
            return;
        };
        let state = self.device_state(target.kind, &target.id);
        let connected = state.as_ref().map_or([false; 3], |d| d.connected);
        let (name, axes) = match target.kind {
            MotionKind::Delay => {
                let Some(entry) = self.inventory.delays.iter().find(|e| e.id == target.id) else {
                    self.alignment.options = None;
                    return;
                };
                (
                    entry.display_name(),
                    vec![OptionsAxis {
                        axis: 0,
                        label: "Direction",
                        direction: entry.config.direction,
                        home: true,
                    }],
                )
            }
            MotionKind::Scanner => {
                let Some(entry) = self.inventory.scanners.iter().find(|e| e.id == target.id) else {
                    self.alignment.options = None;
                    return;
                };
                (entry.display_name(), scanner_options_axes(entry))
            }
        };
        let locked = !self.inventory_editable();
        let mut intents = Vec::new();
        let response = egui::Modal::new(egui::Id::new("alignment-options")).show(ctx, |ui| {
            ui.set_width(310.);
            ui.heading(format!("Options · {name}"));
            ui.label("Direction keeps the displayed coordinate; no motion.");
            ui.add_space(8.);
            for OptionsAxis {
                axis,
                label,
                direction,
                home,
            } in axes
            {
                ui.horizontal(|ui| {
                    ui.label(label);
                    for (value, text) in devices::DIRECTION_CHOICES {
                        if ui
                            .add_enabled(
                                !locked && connected[axis],
                                egui::Button::new(text).selected(value == direction),
                            )
                            .clicked()
                            && value != direction
                        {
                            intents.push(Intent::Reverse {
                                kind: target.kind,
                                id: target.id.clone(),
                                axis,
                            });
                        }
                    }
                });
                if let Some((text, color, help)) =
                    self.device_feedback
                        .direction_caption(target.kind, &target.id, axis)
                {
                    ui.colored_label(color, text).on_hover_text(help);
                }
                let label = if target.kind == MotionKind::Delay {
                    "Initialize"
                } else if axis == 1 {
                    "Initialize X"
                } else {
                    "Initialize Y"
                };
                if ui
                    .add_enabled(!locked && connected[axis], egui::Button::new(label))
                    .clicked()
                {
                    intents.push(Intent::Initialize(InitializeTarget {
                        kind: target.kind,
                        id: target.id.clone(),
                        axis,
                        name: name.clone(),
                        default_home: home,
                    }));
                }
                ui.add_space(6.);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Stop").clicked() {
                    intents.push(Intent::Stop);
                }
                if ui.button("Close").clicked() {
                    self.alignment.options = None;
                }
            });
        });
        if response.should_close() {
            self.alignment.options = None;
        }
        self.apply_alignment_intents(intents);
    }

    fn alignment_initialize_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.alignment.initialize.clone() else {
            return;
        };
        let ready = self.alignment_initialize_ready(&target);
        let response = egui::Modal::new(egui::Id::new("alignment-initialize-motion")).show(ctx, |ui| {
            ui.heading(format!("Initialize {}", target.name));
            ui.label("Initialization uses the configured reference procedure. It does not start until confirmed.");
            if target.default_home {
                ui.checkbox(&mut self.alignment.skip_home, "Skip HOME (already referenced)");
            } else {
                ui.label("This controller has no HOME procedure; initialization prepares its configured axis.");
            }
            ui.checkbox(
                &mut self.alignment.move_to_coordinate_zero,
                "Move to coordinate zero after initialization",
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.alignment.initialize = None;
                }
                if ui.add_enabled(ready, egui::Button::new("Initialize")).clicked() {
                    let action = AlignmentAction::Initialize {
                        axis: target.axis,
                        home: target.default_home && !self.alignment.skip_home,
                        move_to_origin: self.alignment.move_to_coordinate_zero,
                    };
                    match self.controller.send(Command::Alignment {
                        kind: target.kind,
                        id: target.id,
                        action,
                    }) {
                        Ok(()) => self.alignment.initialize = None,
                        Err(error) => self.note(error),
                    }
                }
            });
        });
        if response.should_close() {
            self.alignment.initialize = None;
        }
    }

    fn alignment_initialize_ready(&self, target: &InitializeTarget) -> bool {
        self.inventory_editable()
            && self
                .device_state(target.kind, &target.id)
                .is_some_and(|state| state.connected[target.axis])
    }
}

fn preview_scanner(port: String, origin: f64, scale: f64) -> ScannerConfig {
    ScannerConfig {
        controller: "CONEXCC".into(),
        actuator: Some("TRA12CC".into()),
        port,
        axis: "1".into(),
        origin_pos: Some(origin),
        min_pos: Some(0.),
        max_pos: Some(12.),
        sample_um_per_unit: scale,
        ..ScannerConfig::default()
    }
}

fn preview_device_states(inventory: &Inventory) -> Vec<DeviceState> {
    let mut devices = Vec::new();
    for (index, entry) in inventory.delays.iter().enumerate() {
        let native = 210. + index as f64 * 46.;
        let measurement = entry
            .config
            .measurement_ps_from_mm(native)
            .ok()
            .unwrap_or_default();
        devices.push(DeviceState {
            id: entry.id.clone(),
            kind: "delay".into(),
            connected: [true, false, false],
            position: Position {
                physical: [Some(measurement), None, None],
                delay_stage_mm: Some(native),
                delay_stage_pulse: None,
                ..Position::default()
            },
            delay_calibration: None,
            phase: "PREVIEW".into(),
        });
    }
    for (index, entry) in inventory.scanners.iter().enumerate() {
        let x = entry.zero_um[0] + 12. - index as f64 * 2.;
        let y = entry.zero_um[1] - 8. + index as f64 * 1.5;
        let x_raw = entry.x.as_ref().map(|config| config.raw_from_sample_um(x));
        let y_raw = entry.y.as_ref().map(|config| config.raw_from_sample_um(y));
        devices.push(DeviceState {
            id: entry.id.clone(),
            kind: "scanner".into(),
            connected: [false, true, true],
            position: Position {
                physical: [None, Some(x), Some(y)],
                x_scanner_mm: x_raw,
                y_scanner_mm: y_raw,
                ..Position::default()
            },
            delay_calibration: None,
            phase: "PREVIEW".into(),
        });
    }
    devices
}

fn options_button(render: &mut RenderState<'_>, rect: egui::Rect, kind: MotionKind, id: &str) {
    if button(
        render.ui,
        egui::Rect::from_min_size(rect.right_top() + Vec2::new(-144., 8.), Vec2::new(70., 26.)),
        "Options",
        !render.locked || render.alignment.preview,
    )
    .clicked()
    {
        render.intents.push(Intent::Options(OptionsTarget {
            kind,
            id: id.into(),
        }));
    }
}

fn delete_panel_button(
    render: &mut RenderState<'_>,
    rect: egui::Rect,
    kind: MotionKind,
    slot: usize,
) {
    if button(
        render.ui,
        egui::Rect::from_min_size(rect.right_top() + Vec2::new(-68., 8.), Vec2::new(54., 26.)),
        "Delete",
        !render.locked,
    )
    .clicked()
    {
        render.intents.push(Intent::DeletePanel {
            kind,
            id: render.alignment.layout.panels(kind)[slot].id.clone(),
        });
    }
}

fn draw_delay_card(
    render: &mut RenderState<'_>,
    rect: egui::Rect,
    slot: usize,
    options: &[DeviceOption],
    panel: Option<&DelayPanel>,
) {
    card(render.ui, rect, "");
    delete_panel_button(render, rect, MotionKind::Delay, slot);
    let options = panel_options(options, &render.alignment.layout.delays, slot);
    let previous = render.alignment.layout.delays[slot].device_id.clone();
    selector(
        render.ui,
        rect,
        (
            "delay-select",
            render.alignment.layout.delays[slot].id.clone(),
        ),
        &mut render.alignment.layout.delays[slot].device_id,
        &options,
        render.locked,
    );
    if previous != render.alignment.layout.delays[slot].device_id {
        return;
    }
    let Some(panel) = panel else {
        label(
            render.ui,
            rect.min + Vec2::new(14., 62.),
            "Choose a delay stage",
            MUTED,
        );
        return;
    };
    let connected = panel.state.as_ref().is_some_and(|d| d.connected[0]);
    status(
        render.ui,
        rect,
        connected,
        panel.state.as_ref().map_or("Not connected", |d| &d.phase),
    );
    options_button(render, rect, MotionKind::Delay, &panel.option.id);
    label(render.ui, rect.min + Vec2::new(14., 54.), "Unit", MUTED);
    unit_selector(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(46., 48.), Vec2::new(138., 26.)),
        (
            "delay-unit",
            render.alignment.layout.delays[slot].id.clone(),
        ),
        &mut render.alignment.layout.delays[slot].unit,
        !render.locked,
        true,
    );
    label(render.ui, rect.min + Vec2::new(200., 54.), "Step", MUTED);
    number(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(238., 48.), Vec2::new(100., 26.)),
        &mut render.alignment.layout.delays[slot].step,
        !render.locked,
    );
    let unit = render.alignment.layout.delays[slot].unit;
    let display = delay_display(panel, unit);
    render.ui.painter().text(
        rect.center_top() + Vec2::new(0., 86.),
        egui::Align2::CENTER_TOP,
        format!("{}  {}", display.label, compact(display.current)),
        egui::FontId::proportional(20.),
        TEXT,
    );
    let rail_rect = egui::Rect::from_min_max(
        rect.min + Vec2::new(112., 126.),
        rect.right_top() + Vec2::new(-112., 162.),
    );
    position_rail(render.ui, rail_rect, &display);
    let amount = render.alignment.layout.delays[slot].step;
    for (positive, x) in [(false, 60.), (true, rect.width() - 100.)] {
        motion_arrow_button(
            render,
            &StepControl {
                position: rect.min + Vec2::new(x, 120.),
                size: Vec2::new(40., 38.),
                kind: MotionKind::Delay,
                id: &panel.option.id,
                axis: 0,
                amount,
                unit,
                positive,
                locked: render.locked || !connected,
            },
            if positive { (1., 0.) } else { (-1., 0.) },
        );
    }
    for (positive, x) in [(false, 14.), (true, rect.width() - 54.)] {
        let r = egui::Rect::from_min_size(rect.min + Vec2::new(x, 120.), Vec2::new(36., 38.));
        let response = button(
            render.ui,
            r,
            "",
            !render.locked && connected && display.limits.is_some(),
        );
        paint_arrow(
            render.ui,
            r.center(),
            if positive { (1., 0.) } else { (-1., 0.) },
            MUTED,
        );
        let tip = r.center() + Vec2::new(if positive { 12. } else { -12. }, 0.);
        render.ui.painter().line_segment(
            [tip - Vec2::new(0., 10.), tip + Vec2::new(0., 10.)],
            Stroke::new(1.5_f32, MUTED),
        );
        if response
            .on_hover_text(if positive {
                "Move to upper travel end"
            } else {
                "Move to lower travel end"
            })
            .clicked()
        {
            render.intents.push(end(
                MotionKind::Delay,
                &panel.option.id,
                0,
                positive,
                &display,
            ));
        }
    }
    if button(
        render.ui,
        egui::Rect::from_center_size(rect.center_top() + Vec2::new(0., 188.), Vec2::new(76., 28.)),
        "Stop",
        true,
    )
    .clicked()
    {
        render.intents.push(Intent::Stop);
    }
    label(
        render.ui,
        rect.min + Vec2::new(14., 184.),
        "Click: step · Hold: move",
        MUTED,
    );
}

fn draw_scanner_card(
    render: &mut RenderState<'_>,
    rect: egui::Rect,
    slot: usize,
    options: &[DeviceOption],
    panel: Option<&ScannerPanel>,
) {
    card(render.ui, rect, "");
    delete_panel_button(render, rect, MotionKind::Scanner, slot);
    let options = panel_options(options, &render.alignment.layout.scanners, slot);
    let previous = render.alignment.layout.scanners[slot].device_id.clone();
    selector(
        render.ui,
        rect,
        (
            "scanner-select",
            render.alignment.layout.scanners[slot].id.clone(),
        ),
        &mut render.alignment.layout.scanners[slot].device_id,
        &options,
        render.locked,
    );
    if previous != render.alignment.layout.scanners[slot].device_id {
        return;
    }
    let Some(panel) = panel else {
        label(
            render.ui,
            rect.min + Vec2::new(14., 62.),
            "Choose a scanner",
            MUTED,
        );
        return;
    };
    let axes = panel.state.as_ref().map_or([false; 3], |d| d.connected);
    status(
        render.ui,
        rect,
        axes[1] || axes[2],
        panel.state.as_ref().map_or("Not connected", |d| &d.phase),
    );
    options_button(render, rect, MotionKind::Scanner, &panel.option.id);
    label(render.ui, rect.min + Vec2::new(14., 54.), "Unit", MUTED);
    let unit_width = ((rect.width() - 28.) * 0.38).min(132.);
    unit_selector(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(46., 48.), Vec2::new(unit_width, 26.)),
        (
            "scanner-unit",
            render.alignment.layout.scanners[slot].id.clone(),
        ),
        &mut render.alignment.layout.scanners[slot].unit,
        !render.locked,
        false,
    );
    let step_x = 46. + unit_width + 12.;
    label(render.ui, rect.min + Vec2::new(step_x, 54.), "Step", MUTED);
    number(
        render.ui,
        egui::Rect::from_min_size(
            rect.min + Vec2::new(step_x + 36., 48.),
            Vec2::new((rect.width() - step_x - 50.).min(100.), 26.),
        ),
        &mut render.alignment.layout.scanners[slot].step,
        !render.locked,
    );
    let unit = render.alignment.layout.scanners[slot].unit;
    let display = [
        scanner_display(panel, unit, 0),
        scanner_display(panel, unit, 1),
    ];
    let layout = scanner_card_layout(rect);
    for (axis, readout) in layout.readouts.into_iter().enumerate() {
        // Values may be much wider than a compact card. The fixed readout
        // rectangle prevents visual overlap, while the hover keeps all digits
        // available instead of silently losing scientific detail.
        let text = format!(
            "{}  {}",
            display[axis].label,
            compact(display[axis].current)
        );
        render
            .ui
            .put(
                readout,
                egui::Label::new(egui::RichText::new(&text).font(egui::FontId::proportional(12.)))
                    .truncate(),
            )
            .on_hover_text(text);
    }
    position_graph(render.ui, layout.graph, &display);
    for (axis, positive, offset, direction) in [
        (1, false, Vec2::new(-(layout.pad.x + 4.), 0.), (-1., 0.)),
        (1, true, Vec2::new(layout.pad.x + 4., 0.), (1., 0.)),
        (2, true, Vec2::new(0., -(layout.pad.y + 4.)), (0., -1.)),
        (2, false, Vec2::new(0., layout.pad.y + 4.), (0., 1.)),
    ] {
        motion_arrow_button(
            render,
            &StepControl {
                position: layout.pad_center + offset - layout.pad * 0.5,
                size: layout.pad,
                kind: MotionKind::Scanner,
                id: &panel.option.id,
                axis,
                amount: render.alignment.layout.scanners[slot].step,
                unit,
                positive,
                locked: render.locked || !axes[axis],
            },
            direction,
        );
    }
    if button(
        render.ui,
        egui::Rect::from_center_size(layout.pad_center, layout.pad),
        "Stop",
        true,
    )
    .clicked()
    {
        render.intents.push(Intent::Stop);
    }
    label(
        render.ui,
        rect.min + Vec2::new(14., 264.),
        "Click: step · Hold: move",
        MUTED,
    );
}

/// Geometry for the scanner's compact graph-and-control region.  The graph
/// keeps a square footprint on the left; the readouts and cross pad share a
/// separate right column.  Keeping these bounds together prevents a long
/// coordinate from growing back into either interactive region.
#[derive(Clone, Copy, Debug)]
struct ScannerCardLayout {
    graph: egui::Rect,
    readouts: [egui::Rect; 2],
    pad_center: egui::Pos2,
    pad: Vec2,
}

impl ScannerCardLayout {
    #[cfg(test)]
    fn pad_bounds(self) -> egui::Rect {
        egui::Rect::from_center_size(
            self.pad_center,
            Vec2::new(self.pad.x * 3. + 8., self.pad.y * 3. + 8.),
        )
    }
}

fn scanner_card_layout(rect: egui::Rect) -> ScannerCardLayout {
    const MARGIN: f32 = 14.;
    const TOP: f32 = 88.;
    const GAP: f32 = 10.;
    const MIN_CONTROL_WIDTH: f32 = 96.;
    const MAX_GRAPH_SIZE: f32 = 172.;

    let content = egui::Rect::from_min_max(
        rect.min + Vec2::new(MARGIN, TOP),
        rect.right_bottom() - Vec2::new(MARGIN, 0.),
    );
    let graph_size = (content.width() - GAP - MIN_CONTROL_WIDTH).clamp(72., MAX_GRAPH_SIZE);
    let graph = egui::Rect::from_min_size(content.min, Vec2::splat(graph_size));
    let controls = egui::Rect::from_min_max(
        egui::pos2(graph.right() + GAP, content.top()),
        content.right_top(),
    );
    let readout_height = 18.;
    let readouts = [
        egui::Rect::from_min_size(
            controls.min + Vec2::new(0., 2.),
            Vec2::new(controls.width(), readout_height),
        ),
        egui::Rect::from_min_size(
            controls.min + Vec2::new(0., 22.),
            Vec2::new(controls.width(), readout_height),
        ),
    ];
    // The 8 px allowance is the total inter-button gap in the pad bounds.
    let pad_width = ((controls.width() - 8.) / 3.).clamp(26., 42.);
    let pad = Vec2::new(pad_width, (pad_width - 2.).clamp(28., 40.));
    let pad_center = egui::pos2(controls.center().x, rect.top() + 198.);
    ScannerCardLayout {
        graph,
        readouts,
        pad_center,
        pad,
    }
}

fn valid_increment(amount: f64) -> bool {
    amount.is_finite() && amount > 0.
}

fn motion_button(
    render: &mut RenderState<'_>,
    control: &StepControl<'_>,
    title: &str,
) -> egui::Response {
    let held = HeldControl {
        kind: control.kind,
        id: control.id.into(),
        axis: control.axis,
        positive: control.positive,
    };
    let active = render.alignment.held.as_ref() == Some(&held);
    let focused = render.ui.ctx().input(|i| i.focused);
    let enabled = !control.locked && valid_increment(control.amount) && focused;
    let response = button(
        render.ui,
        egui::Rect::from_min_size(control.position, control.size),
        title,
        enabled || active,
    );
    if !valid_increment(control.amount) {
        response
            .clone()
            .on_disabled_hover_text("Enter a finite, positive step");
    }
    // Once Jog owns this gesture, keep it captured even if the pointer leaves
    // the click-only button. The lease ends on actual release or focus loss.
    let captured = active
        && render
            .alignment
            .pressed
            .as_ref()
            .is_some_and(|p| p.widget == response.id);
    let down = response.is_pointer_button_down_on()
        || (captured && focused && render.ui.ctx().input(|i| i.pointer.primary_down()));
    let (pointer_pressed, time) = render
        .ui
        .ctx()
        .input(|i| (i.pointer.primary_pressed(), i.time));
    if down && pointer_pressed && enabled && render.alignment.held.is_none() {
        render.alignment.pressed = Some(PressedControl {
            widget: response.id,
            control: held.clone(),
            since: time,
            amount: control.amount,
            unit: control.unit,
            hold_started: false,
        });
    }
    if down
        && focused
        && let Some(press) = render
            .alignment
            .pressed
            .as_mut()
            .filter(|p| p.widget == response.id)
    {
        render
            .ui
            .ctx()
            .request_repaint_after(Duration::from_millis(20));
        if enabled && !press.hold_started && time - press.since >= HOLD_DELAY_S {
            press.hold_started = true;
            render.intents.push(Intent::Hold {
                control: press.control.clone(),
                amount: press.amount,
                unit: press.unit,
            });
        }
    }
    if response.clicked() && enabled && !active {
        let press = render
            .alignment
            .pressed
            .as_ref()
            .filter(|p| p.widget == response.id);
        // A held gesture must never append a Step when its pointer is released.
        if press.is_none_or(|p| !p.hold_started) {
            let (amount, unit) =
                press.map_or((control.amount, control.unit), |p| (p.amount, p.unit));
            render.intents.push(Intent::Command {
                kind: control.kind,
                id: control.id.into(),
                action: AlignmentAction::Step {
                    axis: control.axis,
                    amount: if control.positive { amount } else { -amount },
                    unit,
                },
            });
        }
    }
    if active && down {
        *render.held_down = true;
    }
    if !down
        && render
            .alignment
            .pressed
            .as_ref()
            .is_some_and(|p| p.widget == response.id)
    {
        render.alignment.pressed = None;
    }
    response
}

fn motion_arrow_button(
    render: &mut RenderState<'_>,
    control: &StepControl<'_>,
    direction: (f32, f32),
) {
    let axis = match (control.kind, control.axis) {
        (MotionKind::Delay, _) => "Delay",
        (_, 1) => "X",
        _ => "Y",
    };
    let sign = if control.positive { "+" } else { "−" };
    motion_button(render, control, "").on_hover_text(format!(
        "{axis}{sign} · Click: one step · Hold: move · Release: stop"
    ));
    paint_arrow(
        render.ui,
        control.position + control.size * 0.5,
        direction,
        if control.locked || !valid_increment(control.amount) {
            MUTED
        } else {
            TEXT
        },
    );
}
fn paint_arrow(ui: &egui::Ui, center: egui::Pos2, direction: (f32, f32), color: Color32) {
    let forward = Vec2::new(direction.0, direction.1) * 9.;
    let side = Vec2::new(-direction.1, direction.0) * 6.;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + forward,
            center - forward * 0.62 + side,
            center - forward * 0.62 - side,
        ],
        color,
        Stroke::NONE,
    ));
}

fn end(
    kind: MotionKind,
    id: &str,
    axis: usize,
    positive: bool,
    display: &PositionDisplay,
) -> Intent {
    Intent::Command {
        kind,
        id: id.into(),
        action: AlignmentAction::End {
            axis,
            positive: positive == display.native_increases,
        },
    }
}

fn card(ui: &mut egui::Ui, rect: egui::Rect, title: &str) {
    ui.painter().rect(
        rect,
        8.,
        SURFACE,
        Stroke::new(1_f32, BORDER),
        egui::StrokeKind::Inside,
    );
    if !title.is_empty() {
        label(ui, rect.min + Vec2::new(14., 12.), title, TEXT);
    }
}

fn label(ui: &mut egui::Ui, position: egui::Pos2, text: &str, color: Color32) {
    ui.painter().text(
        position,
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::proportional(12.),
        color,
    );
}
fn status(ui: &mut egui::Ui, rect: egui::Rect, connected: bool, phase: &str) {
    let color = if connected { READY } else { MUTED };
    let left = rect.left() + 14. + ((rect.width() - 28.) * 0.36).min(230.) + 12.;
    ui.painter()
        .circle_filled(egui::pos2(left, rect.top() + 20.), 3., color);
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(left + 8., rect.top() + 13.),
        egui::pos2(rect.right() - 152., rect.top() + 35.),
    );
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(text_rect))
        .text(
            text_rect.min,
            egui::Align2::LEFT_TOP,
            if !connected {
                "Offline"
            } else if phase.starts_with("Error") {
                "Error"
            } else {
                phase
            },
            egui::FontId::proportional(12.),
            color,
        );
    ui.interact(
        text_rect,
        ui.id()
            .with(("device-phase", rect.min.x.to_bits(), rect.min.y.to_bits())),
        egui::Sense::hover(),
    )
    .on_hover_text(phase);
}
fn selector(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash,
    selected: &mut Option<String>,
    options: &[DeviceOption],
    locked: bool,
) {
    let field = egui::Rect::from_min_size(
        rect.min + Vec2::new(14., 7.),
        Vec2::new(((rect.width() - 28.) * 0.36).min(230.), 28.),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
        ui.add_enabled_ui(!locked, |ui| {
            ui.spacing_mut().combo_width = field.width();
            egui::ComboBox::from_id_salt(id)
                .width(field.width())
                .truncate()
                .selected_text(
                    selected
                        .as_deref()
                        .and_then(|value| options.iter().find(|option| option.id == value))
                        .map(|option| option.name.as_str())
                        .unwrap_or("—"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(selected, None, "None");
                    for option in options {
                        ui.selectable_value(selected, Some(option.id.clone()), &option.name);
                    }
                });
        });
    });
}
fn button(ui: &mut egui::Ui, rect: egui::Rect, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        ui.put(
            rect,
            egui::Button::new(text)
                .min_size(rect.size())
                .corner_radius(6),
        )
    })
    .inner
}

fn number(ui: &mut egui::Ui, rect: egui::Rect, value: &mut f64, enabled: bool) {
    // DragValue uses the style's text-edit minimum width unless it is scoped.
    // Constrain it to the recorded rectangle so adjacent fields never overlap.
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.spacing_mut().text_edit_width = (rect.width() - 20.).max(20.);
        ui.add_enabled_ui(enabled, |ui| {
            ui.put(
                rect,
                egui::DragValue::new(value)
                    .speed(0.1)
                    .range(0.0..=1e12)
                    .custom_parser(|text| {
                        text.parse::<f64>().ok().filter(|value| value.is_finite())
                    })
                    .max_decimals(4),
            );
        });
    });
}
fn unit_selector(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash,
    unit: &mut AlignmentUnit,
    enabled: bool,
    delay: bool,
) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            ui.spacing_mut().combo_width = rect.width();
            egui::ComboBox::from_id_salt(id)
                .width(rect.width())
                .truncate()
                .selected_text(match unit {
                    AlignmentUnit::Physical if delay => "t (ps)",
                    AlignmentUnit::Corrected if delay => "t_cor (ps)",
                    AlignmentUnit::Native if delay => "mm",
                    AlignmentUnit::Native => "native",
                    AlignmentUnit::Physical => "µm",
                    AlignmentUnit::Corrected => "cor (µm)",
                })
                .show_ui(ui, |ui| {
                    if delay {
                        ui.selectable_value(unit, AlignmentUnit::Physical, "t (ps)");
                        ui.selectable_value(unit, AlignmentUnit::Corrected, "t_cor (ps)");
                        ui.selectable_value(unit, AlignmentUnit::Native, "mm");
                    } else {
                        ui.selectable_value(unit, AlignmentUnit::Physical, "µm");
                        ui.selectable_value(unit, AlignmentUnit::Corrected, "cor (µm)");
                        ui.selectable_value(unit, AlignmentUnit::Native, "native");
                    }
                });
        });
    });
}
struct PositionDisplay {
    current: Option<f64>,
    limits: Option<[f64; 2]>,
    label: String,
    native_increases: bool,
}

fn delay_display(panel: &DelayPanel, unit: AlignmentUnit) -> PositionDisplay {
    let micro = panel
        .state
        .as_ref()
        .and_then(|d| d.delay_calibration.as_ref())
        .and_then(|c| c.microstep_division)
        .unwrap_or(1);
    let convert = |native| match unit {
        AlignmentUnit::Native => Some(native),
        _ => panel
            .entry
            .config
            .measurement_ps_from_mm_with_micro(native, micro)
            .ok()
            .map(|v| {
                if unit == AlignmentUnit::Corrected {
                    v - panel.entry.zero_ps
                } else {
                    v
                }
            }),
    };
    let endpoints = panel
        .entry
        .config
        .normalized()
        .and_then(|c| c.native_limits_mm(micro))
        .ok()
        .and_then(|(low, high)| Some([convert(low)?, convert(high)?]));
    PositionDisplay {
        current: panel
            .state
            .as_ref()
            .and_then(|d| d.position.delay_stage_mm)
            .and_then(convert),
        limits: endpoints.map(|[a, b]| [a.min(b), a.max(b)]),
        label: match unit {
            AlignmentUnit::Native => "Position (mm)",
            AlignmentUnit::Physical => "t (ps)",
            AlignmentUnit::Corrected => "t_cor (ps)",
        }
        .into(),
        native_increases: endpoints.is_none_or(|[a, b]| b > a),
    }
}

fn scanner_display(panel: &ScannerPanel, unit: AlignmentUnit, axis: usize) -> PositionDisplay {
    let config = if axis == 0 {
        &panel.entry.x
    } else {
        &panel.entry.y
    };
    let config = config.as_ref().and_then(|c| c.clone().normalized().ok());
    let position = panel.state.as_ref().map(|d| &d.position);
    let native = position.and_then(|p| {
        if axis == 0 {
            p.x_scanner_mm.or(p.x_scanner_deg)
        } else {
            p.y_scanner_mm.or(p.y_scanner_deg)
        }
    });
    let convert = |raw| {
        if unit == AlignmentUnit::Native {
            Some(raw)
        } else {
            config.as_ref().map(|c| c.sample_um_from_raw(raw)).map(|v| {
                if unit == AlignmentUnit::Corrected {
                    v - panel.entry.zero_um[axis]
                } else {
                    v
                }
            })
        }
    };
    let endpoints = config
        .as_ref()
        .and_then(|c| Some([convert(c.min_pos?)?, convert(c.max_pos?)?]));
    let name = if axis == 0 { "X" } else { "Y" };
    let label = match unit {
        AlignmentUnit::Native => format!(
            "{name} ({})",
            if config.as_ref().is_some_and(|c| c.controller == "CONEXAGAP") {
                "°"
            } else {
                "mm"
            }
        ),
        AlignmentUnit::Physical => format!("{name} (µm)"),
        AlignmentUnit::Corrected => format!("{name}_cor (µm)"),
    };
    let current = if unit == AlignmentUnit::Native {
        native
    } else {
        position.and_then(|p| p.physical[axis + 1]).map(|v| {
            if unit == AlignmentUnit::Corrected {
                v - panel.entry.zero_um[axis]
            } else {
                v
            }
        })
    };
    PositionDisplay {
        current,
        limits: endpoints.map(|[a, b]| [a.min(b), a.max(b)]),
        label,
        native_increases: endpoints.is_none_or(|[a, b]| b > a),
    }
}

fn position_rail(ui: &mut egui::Ui, rect: egui::Rect, display: &PositionDisplay) {
    let y = rect.top() + 9.;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.left(), y), egui::pos2(rect.right(), y + 6.)),
        3.,
        BORDER,
    );
    let Some([low, high]) = display
        .limits
        .filter(|[a, b]| a.is_finite() && b.is_finite() && b > a)
    else {
        label(
            ui,
            rect.left_bottom() - Vec2::new(0., 12.),
            "Travel unavailable",
            MUTED,
        );
        return;
    };
    if let Some(value) = display.current {
        let x = rect.left() + ((value - low) / (high - low)).clamp(0., 1.) as f32 * rect.width();
        ui.painter().rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y + 3.), Vec2::new(24., 16.)),
            4.,
            ACCENT,
        );
    }
    for (x, align, value) in [
        (rect.left(), egui::Align2::LEFT_TOP, low),
        (rect.right(), egui::Align2::RIGHT_TOP, high),
    ] {
        ui.painter().text(
            egui::pos2(x, y + 18.),
            align,
            compact(Some(value)),
            egui::FontId::proportional(11.),
            MUTED,
        );
    }
}

fn position_graph(ui: &mut egui::Ui, rect: egui::Rect, display: &[PositionDisplay; 2]) {
    ui.painter().rect(
        rect,
        5.,
        Color32::from_rgb(21, 22, 27),
        Stroke::new(1_f32, BORDER),
        egui::StrokeKind::Inside,
    );
    let area = rect.shrink(16.);
    let limits: [[f64; 2]; 2] = std::array::from_fn(|axis| {
        display[axis]
            .limits
            .filter(|[a, b]| a.is_finite() && b.is_finite() && b > a)
            .unwrap_or_else(|| {
                let span = display[axis].current.unwrap_or(0.).abs().max(1.) * 1.2;
                [-span, span]
            })
    });
    let map = |axis: usize, value: f64| {
        ((value - limits[axis][0]) / (limits[axis][1] - limits[axis][0])).clamp(0., 1.) as f32
    };
    for fraction in [0., 0.5, 1.] {
        let x = area.left() + fraction * area.width();
        let y = area.top() + fraction * area.height();
        ui.painter().line_segment(
            [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())],
            Stroke::new(0.5_f32, BORDER),
        );
        ui.painter().line_segment(
            [egui::pos2(area.left(), y), egui::pos2(area.right(), y)],
            Stroke::new(0.5_f32, BORDER),
        );
    }
    for (axis, bounds) in limits.iter().enumerate() {
        if bounds[0] <= 0. && bounds[1] >= 0. {
            let fraction = map(axis, 0.);
            let points = if axis == 0 {
                let x = area.left() + fraction * area.width();
                [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())]
            } else {
                let y = area.bottom() - fraction * area.height();
                [egui::pos2(area.left(), y), egui::pos2(area.right(), y)]
            };
            ui.painter().line_segment(points, Stroke::new(1_f32, MUTED));
        }
    }
    label(ui, rect.right_center() - Vec2::new(18., 14.), "X", MUTED);
    label(ui, rect.center_top() + Vec2::new(3., 1.), "Y", MUTED);
    if let [Some(x), Some(y)] = display.each_ref().map(|d| d.current) {
        ui.painter().circle_filled(
            egui::pos2(
                area.left() + map(0, x) * area.width(),
                area.bottom() - map(1, y) * area.height(),
            ),
            4.,
            ACCENT,
        );
    }
    let help = format!(
        "{}: {} to {}\n{}: {} to {}",
        display[0].label,
        compact(Some(limits[0][0])),
        compact(Some(limits[0][1])),
        display[1].label,
        compact(Some(limits[1][0])),
        compact(Some(limits[1][1]))
    );
    ui.interact(
        rect,
        ui.id().with((
            "alignment-position",
            rect.left().to_bits(),
            rect.top().to_bits(),
        )),
        egui::Sense::hover(),
    )
    .on_hover_text(help);
}

fn compact(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.3}", if value.abs() < 0.0005 { 0. } else { value }))
        .unwrap_or_else(|| "—".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<DeviceOption> {
        vec![
            DeviceOption {
                id: "scanner-1".into(),
                name: "Scanner 1".into(),
            },
            DeviceOption {
                id: "scanner-2".into(),
                name: "Scanner 2".into(),
            },
        ]
    }

    #[test]
    fn panel_candidates_follow_stable_device_ids_and_release_after_removal() {
        let mut panels: Vec<Panel> = devices()
            .iter()
            .map(|device| Panel {
                device_id: Some(device.id.clone()),
                ..Panel::empty()
            })
            .collect();
        panels.push(Panel::empty());
        let mut available = devices();
        available.reverse();
        assert!(panel_options(&available, &panels, 2).is_empty());
        assert_eq!(panel_options(&available, &panels, 0)[0].id, "scanner-1");
        panels[1].device_id = None;
        assert_eq!(panel_options(&available, &panels, 2)[0].id, "scanner-2");
        panels.remove(0);
        assert_eq!(panel_options(&available, &panels, 1).len(), 2);
    }

    #[test]
    fn scanner_card_keeps_long_coordinate_readouts_clear_of_graph_and_cross_pad() {
        // These are deliberately wider than the preview values: the geometry,
        // rather than a lucky short value, reserves the readout column.
        let displays = [
            PositionDisplay {
                label: "X_cor (µm)".into(),
                current: Some(-123_456.789),
                limits: None,
                native_increases: true,
            },
            PositionDisplay {
                label: "Y_cor (µm)".into(),
                current: Some(987_654.321),
                limits: None,
                native_increases: true,
            },
        ];
        assert!(displays.iter().all(|display| {
            format!("{}  {}", display.label, compact(display.current)).len() > 20
        }));

        for width in [240., 300., 440.] {
            let card = precision::rect(0., 0., width, 282.);
            let layout = scanner_card_layout(card);
            let pad_bounds = layout.pad_bounds();
            assert_eq!(layout.graph.width(), layout.graph.height());
            assert!(layout.graph.width() >= 72.);
            assert!(card.contains_rect(layout.graph));
            assert!(card.contains_rect(pad_bounds));
            assert!(!layout.graph.intersects(pad_bounds));
            for readout in layout.readouts {
                assert!(card.contains_rect(readout));
                assert!(!readout.intersects(layout.graph));
                assert!(!readout.intersects(pad_bounds));
            }
        }
    }

    #[test]
    fn panel_layout_roundtrips_through_profile_and_api_rejects_invalid_replacement() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let original = app.alignment_layout_value().unwrap();
        let mut changed = original.clone();
        changed["delays"] = json!([]);
        changed["scanners"][0]["unit"] = json!("corrected");
        changed["scanners"][0]["step"] = json!(2.5);
        let accepted = app.set_alignment_layout_value(&changed).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        app.sync_all_plans().unwrap();
        app.settings.save(&path).unwrap();
        app.settings = Settings::load(&path).unwrap();
        app.hydrate();
        assert_eq!(app.alignment_layout_value().unwrap(), accepted);
        changed["scanners"][0]["device_id"] = json!("missing");
        assert!(app.set_alignment_layout_value(&changed).is_err());
        assert_eq!(app.alignment_layout_value().unwrap(), accepted);
        assert_eq!(app.settings.raw["alignment"], accepted);
    }

    #[test]
    fn deleting_panel_does_not_delete_instrument_or_change_workflow_assignment() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let devices = serde_json::to_value(&app.inventory).unwrap();
        let assignments: Vec<_> = Workflow::ALL
            .into_iter()
            .map(|w| app.settings.workflow_device_ids(w).unwrap())
            .collect();
        let key = app.alignment.layout.scanners[0].id.clone();
        app.alignment.layout.remove(MotionKind::Scanner, &key);
        app.sync_alignment().unwrap();
        app.hydrate();
        assert!(app.alignment.layout.scanners.is_empty());
        assert_eq!(serde_json::to_value(&app.inventory).unwrap(), devices);
        assert_eq!(
            Workflow::ALL
                .into_iter()
                .map(|w| app.settings.workflow_device_ids(w).unwrap())
                .collect::<Vec<_>>(),
            assignments
        );
    }

    #[test]
    fn step_editor_clamps_negative_input_without_changing_positive_steps() {
        let context = egui::Context::default();
        let mut value = 1.;
        let mut frame = |events: Vec<egui::Event>, modifiers: egui::Modifiers| {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(precision::rect(0., 0., 400., 300.)),
                    events,
                    modifiers,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        number(ui, precision::rect(20., 20., 100., 26.), &mut value, true);
                    });
                },
            );
            value
        };
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        for (text, expected) in [("-5", 0.), ("0.125", 0.125), ("NaN", 0.125)] {
            frame(vec![], Default::default());
            for pressed in [true, false] {
                frame(
                    vec![
                        egui::Event::PointerMoved(egui::pos2(60., 32.)),
                        egui::Event::PointerButton {
                            pos: egui::pos2(60., 32.),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    Default::default(),
                );
            }
            frame(vec![], Default::default());
            frame(
                vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
                egui::Modifiers::COMMAND,
            );
            frame(vec![egui::Event::Text(text.into())], Default::default());
            assert_eq!(
                frame(
                    vec![key(egui::Key::Enter, Default::default())],
                    Default::default()
                ),
                expected,
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn pointer_frame(
        context: &egui::Context,
        alignment: &mut AlignmentUi,
        time: f64,
        pressed: Option<bool>,
        amount: f64,
        locked: bool,
        focused: bool,
        inside: bool,
    ) -> (Vec<Intent>, bool) {
        let pointer = if inside {
            egui::pos2(40., 40.)
        } else {
            egui::pos2(350., 250.)
        };
        let mut events = vec![egui::Event::PointerMoved(pointer)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let input = egui::RawInput {
            screen_rect: Some(precision::rect(0., 0., 400., 300.)),
            time: Some(time),
            events,
            focused,
            ..Default::default()
        };
        let mut intents = Vec::new();
        let mut down = false;
        let _ = context.run_ui(input, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let mut render = RenderState {
                    ui,
                    alignment,
                    locked,
                    intents: &mut intents,
                    held_down: &mut down,
                };
                motion_button(
                    &mut render,
                    &StepControl {
                        position: egui::pos2(20., 20.),
                        size: Vec2::new(58., 26.),
                        kind: MotionKind::Scanner,
                        id: "scanner-2",
                        axis: 1,
                        amount,
                        unit: AlignmentUnit::Corrected,
                        positive: false,
                        locked,
                    },
                    "Move",
                );
            });
        });
        (intents, down)
    }
    #[test]
    fn short_click_moves_exact_step_and_invalid_inputs_do_not_move() {
        for (amount, locked, allowed) in [
            (2.5, false, true),
            (0., false, false),
            (-1., false, false),
            (f64::NAN, false, false),
            (f64::INFINITY, false, false),
            (2.5, true, false),
        ] {
            let ctx = egui::Context::default();
            let mut state = AlignmentUi::default();
            pointer_frame(&ctx, &mut state, 0., None, amount, locked, true, true);
            assert!(
                pointer_frame(
                    &ctx,
                    &mut state,
                    0.1,
                    Some(true),
                    amount,
                    locked,
                    true,
                    true
                )
                .0
                .is_empty()
            );
            let (intents, _) = pointer_frame(
                &ctx,
                &mut state,
                0.2,
                Some(false),
                amount,
                locked,
                true,
                true,
            );
            assert_eq!(intents.len(), usize::from(allowed));
            if allowed {
                assert!(
                    matches!(&intents[0],Intent::Command {id,action:AlignmentAction::Step {axis:1,amount,unit:AlignmentUnit::Corrected},..} if id=="scanner-2" && *amount == -2.5)
                );
            }
        }
    }
    #[test]
    fn long_press_becomes_one_hold_and_release_never_adds_a_step() {
        let ctx = egui::Context::default();
        let mut state = AlignmentUi::default();
        pointer_frame(&ctx, &mut state, 0., None, 2.5, false, true, true);
        pointer_frame(&ctx, &mut state, 0.1, Some(true), 2.5, false, true, true);
        assert!(
            pointer_frame(&ctx, &mut state, 0.3, None, 2.5, false, true, true)
                .0
                .is_empty()
        );
        let (intents, _) = pointer_frame(&ctx, &mut state, 0.46, None, 2.5, false, true, true);
        let [
            Intent::Hold {
                control,
                amount,
                unit,
            },
        ] = intents.as_slice()
        else {
            panic!("expected one Hold");
        };
        assert_eq!(*amount, 2.5);
        assert_eq!(*unit, AlignmentUnit::Corrected);
        state.held = Some(control.clone());
        let (intents, down) = pointer_frame(&ctx, &mut state, 0.6, None, 2.5, true, true, true);
        assert!(intents.is_empty());
        assert!(down);
        let (intents, down) =
            pointer_frame(&ctx, &mut state, 0.7, Some(false), 2.5, false, true, true);
        assert!(intents.is_empty());
        assert!(!down);
        assert!(state.pressed.is_none());
    }
    #[test]
    fn held_pointer_can_leave_the_button_and_release_outside_without_a_step() {
        let ctx = egui::Context::default();
        let mut state = AlignmentUi::default();
        pointer_frame(&ctx, &mut state, 0., None, 2.5, false, true, true);
        pointer_frame(&ctx, &mut state, 0.1, Some(true), 2.5, false, true, true);
        assert!(
            pointer_frame(&ctx, &mut state, 0.3, None, 2.5, false, true, true)
                .0
                .is_empty()
        );
        let (intents, _) = pointer_frame(&ctx, &mut state, 0.46, None, 2.5, false, true, true);
        let [
            Intent::Hold {
                control,
                amount,
                unit,
            },
        ] = intents.as_slice()
        else {
            panic!("expected one Hold");
        };
        assert_eq!(*amount, 2.5);
        assert_eq!(*unit, AlignmentUnit::Corrected);
        state.held = Some(control.clone());
        let (intents, down) = pointer_frame(&ctx, &mut state, 0.6, None, 2.5, true, true, false);
        assert!(intents.is_empty());
        assert!(down);
        let (intents, down) =
            pointer_frame(&ctx, &mut state, 0.7, Some(false), 2.5, false, true, false);
        assert!(intents.is_empty());
        assert!(!down);
        assert!(state.pressed.is_none());
    }
    #[test]
    fn release_outside_or_focus_loss_before_hold_does_not_move() {
        for focused in [true, false] {
            let ctx = egui::Context::default();
            let mut state = AlignmentUi::default();
            pointer_frame(&ctx, &mut state, 0., None, 1., false, true, true);
            pointer_frame(&ctx, &mut state, 0.1, Some(true), 1., false, true, true);
            assert!(
                pointer_frame(
                    &ctx,
                    &mut state,
                    0.2,
                    Some(false),
                    1.,
                    false,
                    focused,
                    false
                )
                .0
                .is_empty()
            );
            assert!(state.pressed.is_none());
        }
    }
    #[test]
    fn selected_coordinates_and_limits_follow_latest_offsets() {
        let mut app = App::with_context(&egui::Context::default(), true);
        app.enable_alignment_preview(false);
        let mut delay = DelayPanel {
            option: DeviceOption {
                id: "delay-1".into(),
                name: "Delay 1".into(),
            },
            entry: app.inventory.delays[0].clone(),
            state: app.device_state(MotionKind::Delay, "delay-1"),
        };
        let physical = delay_display(&delay, AlignmentUnit::Physical);
        let corrected = delay_display(&delay, AlignmentUnit::Corrected);
        assert_eq!(
            delay_display(&delay, AlignmentUnit::Native).current,
            Some(210.)
        );
        assert!(!physical.native_increases);
        delay.entry.zero_ps += 10.;
        assert_eq!(
            delay_display(&delay, AlignmentUnit::Physical).current,
            physical.current
        );
        let changed = delay_display(&delay, AlignmentUnit::Corrected);
        assert_eq!(changed.current, corrected.current.map(|v| v - 10.));
        assert_eq!(changed.limits, corrected.limits.map(|a| a.map(|v| v - 10.)));
        let mut scanner = ScannerPanel {
            option: DeviceOption {
                id: "scanner-1".into(),
                name: "Scanner 1".into(),
            },
            entry: app.inventory.scanners[0].clone(),
            state: app.device_state(MotionKind::Scanner, "scanner-1"),
        };
        let x = scanner_display(&scanner, AlignmentUnit::Corrected, 0);
        let y = scanner_display(&scanner, AlignmentUnit::Corrected, 1);
        scanner.entry.zero_um[0] += 5.;
        assert_eq!(
            scanner_display(&scanner, AlignmentUnit::Corrected, 0).current,
            x.current.map(|v| v - 5.)
        );
        assert_eq!(
            scanner_display(&scanner, AlignmentUnit::Corrected, 0).limits,
            x.limits.map(|a| a.map(|v| v - 5.))
        );
        assert_eq!(
            scanner_display(&scanner, AlignmentUnit::Corrected, 1).current,
            y.current
        );
    }
    #[test]
    fn delay_end_targets_follow_selected_coordinate_direction_not_offset() {
        let mut app = App::with_context(&egui::Context::default(), true);
        app.enable_alignment_preview(false);
        let mut panel = DelayPanel {
            option: DeviceOption {
                id: "delay-1".into(),
                name: "Delay 1".into(),
            },
            entry: app.inventory.delays[0].clone(),
            state: app.device_state(MotionKind::Delay, "delay-1"),
        };
        for direction in [0, 1] {
            panel.entry.config.direction = direction;
            for offset in [0., 100.] {
                panel.entry.zero_ps = offset;
                for unit in [
                    AlignmentUnit::Native,
                    AlignmentUnit::Physical,
                    AlignmentUnit::Corrected,
                ] {
                    let display = delay_display(&panel, unit);
                    for upper in [false, true] {
                        let expected = if unit == AlignmentUnit::Native || direction == 0 {
                            upper
                        } else {
                            !upper
                        };
                        let Intent::Command {
                            action: AlignmentAction::End { positive, .. },
                            ..
                        } = end(MotionKind::Delay, "delay-1", 0, upper, &display)
                        else {
                            panic!("expected End");
                        };
                        assert_eq!(positive, expected);
                    }
                }
            }
        }
    }
    #[test]
    fn initialization_confirmation_rechecks_live_state() {
        let mut app = App::with_context(&egui::Context::default(), true);
        app.enable_alignment_preview(false);
        let target = InitializeTarget {
            kind: MotionKind::Delay,
            id: "delay-1".into(),
            axis: 0,
            name: "Delay 1".into(),
            default_home: true,
        };
        assert!(!app.alignment_initialize_ready(&target));
        app.demo_viewer = false;
        assert!(app.alignment_initialize_ready(&target));
        app.snapshot.running = true;
        assert!(!app.alignment_initialize_ready(&target));
        app.snapshot.running = false;
        app.measurement_pending = true;
        assert!(!app.alignment_initialize_ready(&target));
        app.measurement_pending = false;
        app.snapshot.registered_devices[0].connected = [false; 3];
        assert!(!app.alignment_initialize_ready(&target));
    }
    #[test]
    fn alignment_add_waits_for_queued_measurement() {
        for pending in [false, true] {
            let ctx = egui::Context::default();
            let mut app = App::with_context(&ctx, false);
            app.measurement_pending = pending;
            let before = app.alignment.layout.delays.len();
            let area = precision::rect(0., 0., 1000., 600.);
            for pressed in [None, Some(true), Some(false)] {
                let pos = egui::pos2(938., 21.);
                let mut events = vec![egui::Event::PointerMoved(pos)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(area),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default()
                            .show_inside(ui, |ui| app.alignment_view(ui, area));
                    },
                );
            }
            assert_eq!(
                app.alignment.layout.delays.len(),
                before + usize::from(!pending)
            );
            assert!(!app.controller.has_pending());
        }
    }

    #[test]
    fn options_and_left_editor_follow_confirmed_direction_on_both_scanner_axes() {
        let mut app = App::with_context(&egui::Context::default(), false);
        let id = app.inventory.selected.scanner.clone();
        for axis in [1, 2] {
            // A legacy negative scale must not be mistaken for the direction flag.
            for scale in [-582., 582.] {
                for direction in [1, 0] {
                    let entry = app.inventory.scanner().unwrap().unwrap();
                    let mut updated = if axis == 1 { &entry.x } else { &entry.y }.clone().unwrap();
                    updated.direction = direction;
                    updated.sample_um_per_unit = scale;
                    app.device_feedback
                        .pending_direction(MotionKind::Scanner, &id, axis);
                    app.apply_direction(
                        MotionKind::Scanner,
                        &id,
                        trkr_motion::MotionConfig {
                            delay: None,
                            x: (axis == 1).then_some(updated.clone()),
                            y: (axis == 2).then_some(updated),
                        },
                    );
                    let axes = scanner_options_axes(app.inventory.scanner().unwrap().unwrap());
                    assert_eq!(axes.len(), 2);
                    assert_eq!(axes[axis - 1].direction, direction);
                    assert_eq!(axes[axis - 1].direction, app.scanner_direction[axis - 1]);
                    assert_eq!(
                        app.device_feedback
                            .direction_caption(MotionKind::Scanner, &id, axis)
                            .unwrap()
                            .0,
                        "Direction updated"
                    );
                    assert!(!app.controller.has_pending());
                }
            }
        }
    }

    #[test]
    fn options_initialization_matches_left_editor_for_controller_aliases() {
        let app = App::with_context(&egui::Context::default(), false);
        let mut entry = app.inventory.scanner().unwrap().unwrap().clone();
        for (controller, homes) in [
            ("CONEXCC", true),
            ("CONEX-CC", true),
            ("conex_cc", true),
            ("CONEXAGAP", false),
            ("conex-agap", false),
        ] {
            entry.x.as_mut().unwrap().controller = controller.into();
            entry.y = None;
            let axes = scanner_options_axes(&entry);
            assert_eq!(axes.len(), 1);
            assert_eq!(axes[0].axis, 1);
            assert_eq!(axes[0].home, homes);
            assert_eq!(
                axes[0].home,
                initialization_homes(1, &[controller.into(), String::new()])
            );
        }
    }
    #[test]
    fn options_initialize_still_requires_confirmation_without_io() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, true);
        app.alignment.options = Some(OptionsTarget {
            kind: MotionKind::Delay,
            id: "delay-1".into(),
        });
        app.apply_alignment_intents(vec![Intent::Initialize(InitializeTarget {
            kind: MotionKind::Delay,
            id: "delay-1".into(),
            axis: 0,
            name: "Delay: 1".into(),
            default_home: true,
        })]);
        assert!(app.alignment.options.is_none());
        assert!(app.alignment.initialize.is_some());
        assert!(!app.controller.has_pending());
    }
}
