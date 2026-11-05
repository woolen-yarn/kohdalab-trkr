//! Alignment controls rendered in the central work area.
//!
//! This view deliberately owns no transport.  Every movement is represented as
//! an engine command, so the regular worker retains exclusive serial access.

use super::*;
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
    delay_panels: [Option<String>; 2],
    scanner_panels: [Option<String>; 4],
    delay_unit: AlignmentUnit,
    scanner_unit: [AlignmentUnit; 4],
    delay_step: [f64; 2],
    scanner_step: [[f64; 2]; 4],
    scanner_hold: [bool; 4],
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
            delay_panels: Default::default(),
            scanner_panels: Default::default(),
            delay_unit: AlignmentUnit::Physical,
            scanner_unit: [AlignmentUnit::Physical; 4],
            delay_step: [1.; 2],
            scanner_step: [[1.; 2]; 4],
            scanner_hold: [false; 4],
            held: None,
            initialize: None,
            skip_home: false,
            move_to_coordinate_zero: false,
            preview: false,
        }
    }
}

impl AlignmentUi {
    pub(super) fn highlights(&self, kind: &str, id: &str) -> bool {
        match (self.page, kind) {
            (AlignmentPage::Delay, "delay") => self
                .delay_panels
                .iter()
                .flatten()
                .any(|selected| selected == id),
            (AlignmentPage::Scanner, "scanner") => self
                .scanner_panels
                .iter()
                .flatten()
                .any(|selected| selected == id),
            _ => false,
        }
    }
    fn delay_id(&mut self, slot: usize, available: &[DeviceOption]) -> Option<String> {
        if self.delay_panels[slot]
            .as_ref()
            .is_some_and(|id| !available.iter().any(|option| &option.id == id))
        {
            self.delay_panels[slot] = None;
        }
        if self.delay_panels[slot].is_none() {
            self.delay_panels[slot] = available.get(slot).map(|option| option.id.clone());
        }
        self.delay_panels[slot].clone()
    }

    fn scanner_id(&mut self, slot: usize, available: &[DeviceOption]) -> Option<String> {
        if self.scanner_panels[slot]
            .as_ref()
            .is_some_and(|id| !available.iter().any(|option| &option.id == id))
        {
            self.scanner_panels[slot] = None;
        }
        if self.scanner_panels[slot].is_none() {
            self.scanner_panels[slot] = available.get(slot).map(|option| option.id.clone());
        }
        self.scanner_panels[slot].clone()
    }
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
    Reverse {
        kind: MotionKind,
        id: String,
        axis: usize,
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
    kind: MotionKind,
    id: &'a str,
    axis: usize,
    amount: f64,
    unit: AlignmentUnit,
    positive: bool,
    locked: bool,
}

impl App {
    /// The root layout calls this instead of the plot area while Alignment is
    /// selected.  It intentionally keeps all configuration and device cards
    /// in their normal left/right panels.
    pub(super) fn alignment_view(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let locked = self.demo_viewer
            || self.snapshot.running
            || self.controller.has_pending()
            || self.python_locked();
        let delay_options: Vec<_> = self
            .inventory
            .delays
            .iter()
            .map(|entry| DeviceOption {
                id: entry.id.clone(),
                name: entry.name.clone(),
            })
            .collect();
        let scanner_options: Vec<_> = self
            .inventory
            .scanners
            .iter()
            .map(|entry| DeviceOption {
                id: entry.id.clone(),
                name: entry.name.clone(),
            })
            .collect();
        let delays: Vec<_> = self
            .inventory
            .delays
            .iter()
            .map(|entry| DelayPanel {
                option: DeviceOption {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                },
                entry: entry.clone(),
                state: self.device_state(MotionKind::Delay, &entry.id),
            })
            .collect();
        let scanners: Vec<_> = self
            .inventory
            .scanners
            .iter()
            .map(|entry| ScannerPanel {
                option: DeviceOption {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
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
                    area.right_top() + Vec2::new(-228., 14.),
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
                                let height = ((content.height() - gap) * 0.5).max(264.);
                                for slot in 0..2 {
                                    let rect = egui::Rect::from_min_size(
                                        egui::pos2(
                                            content.min.x,
                                            content.min.y + slot as f32 * (height + gap),
                                        ),
                                        Vec2::new(content.width(), height),
                                    );
                                    let id = render.alignment.delay_id(slot, &delay_options);
                                    let panel = id.as_deref().and_then(|id| {
                                        delays.iter().find(|panel| panel.option.id == id)
                                    });
                                    draw_delay_card(&mut render, rect, slot, &delay_options, panel);
                                    render.ui.allocate_rect(rect, egui::Sense::hover());
                                }
                            }
                            AlignmentPage::Scanner => {
                                let gap = 8.;
                                let width = ((content.width() - gap) * 0.5).max(240.);
                                let height = ((content.height() - gap) * 0.5).max(332.);
                                for slot in 0..4 {
                                    let row = slot / 2;
                                    let column = slot % 2;
                                    let rect = egui::Rect::from_min_size(
                                        egui::pos2(
                                            content.min.x + column as f32 * (width + gap),
                                            content.min.y + row as f32 * (height + gap),
                                        ),
                                        Vec2::new(width, height),
                                    );
                                    let id = render.alignment.scanner_id(slot, &scanner_options);
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
        self.apply_alignment_intents(intents);
        self.alignment_initialize_dialog(ui.ctx());
    }

    /// Called on tab changes, window focus loss, close, and pointer release.
    pub(super) fn alignment_cancel_hold(&mut self) {
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
        self.alignment.delay_panels = [Some("delay-1".into()), Some("delay-2".into())];
        self.alignment.scanner_panels =
            std::array::from_fn(|index| Some(format!("scanner-{}", index + 1)));

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
            selected: DeviceIds::default(),
        };
        self.inventory
            .project_selected(&mut self.settings.raw)
            .expect("valid preview inventory");
        self.hydrate();
        self.snapshot.running = false;
        self.snapshot.phase = "PREVIEW — no hardware".into();
        self.snapshot.registered_devices = preview_device_states(&self.inventory);
    }

    fn device_state(&self, kind: MotionKind, id: &str) -> Option<DeviceState> {
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
                Intent::Initialize(target) => {
                    self.alignment.initialize = Some(target);
                    self.alignment.skip_home = false;
                    self.alignment.move_to_coordinate_zero = true;
                }
                Intent::Reverse { kind, id, axis } => {
                    if let Err(error) = self.reverse_device_direction(kind, &id, axis) {
                        self.note(error);
                    }
                }
                Intent::Stop => {
                    self.alignment.held.take();
                    // Unlike tab/focus cleanup, an explicit Stop must cancel
                    // an ordinary Step or End operation as well as a hold.
                    self.controller.release_hold();
                    self.note("Alignment stop requested");
                }
            }
        }
    }

    fn alignment_initialize_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.alignment.initialize.clone() else {
            return;
        };
        egui::Modal::new(egui::Id::new("alignment-initialize-motion")).show(ctx, |ui| {
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
                if ui.button("Initialize").clicked() {
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

fn draw_delay_card(
    render: &mut RenderState<'_>,
    rect: egui::Rect,
    slot: usize,
    options: &[DeviceOption],
    panel: Option<&DelayPanel>,
) {
    card(render.ui, rect, &format!("Delay {}", slot + 1));
    let id = &mut render.alignment.delay_panels[slot];
    selector(
        render.ui,
        rect,
        ("delay-select", slot),
        id,
        options,
        render.locked,
    );
    let Some(panel) = panel else {
        label(
            render.ui,
            rect.min + Vec2::new(14., 62.),
            "Choose a delay stage",
            MUTED,
        );
        return;
    };
    let connected = panel
        .state
        .as_ref()
        .is_some_and(|device| device.connected[0]);
    status(
        render.ui,
        rect,
        connected,
        panel
            .state
            .as_ref()
            .map_or("Not connected", |device| &device.phase),
    );
    let native = panel
        .state
        .as_ref()
        .and_then(|device| device.position.delay_stage_mm);
    let microstep_division = panel
        .state
        .as_ref()
        .and_then(|device| device.delay_calibration.as_ref())
        .and_then(|calibration| calibration.microstep_division)
        .unwrap_or(1);
    let t = native.and_then(|millimetres| {
        panel
            .entry
            .config
            .measurement_ps_from_mm_with_micro(millimetres, microstep_division)
            .ok()
    });
    let corrected = t.map(|value| value - panel.entry.zero_ps);
    rail(
        render.ui,
        egui::Rect::from_min_size(
            rect.min + Vec2::new(14., 48.),
            Vec2::new(rect.width() - 28., 90.),
        ),
        native,
        &panel.entry,
        microstep_division,
    );
    label(
        render.ui,
        rect.min + Vec2::new(14., 145.),
        &format!("Position  {}", value(native, "mm")),
        TEXT,
    );
    label(
        render.ui,
        rect.min + Vec2::new(rect.width() * 0.34, 145.),
        &format!("t  {}", value(t, "ps")),
        TEXT,
    );
    label(
        render.ui,
        rect.min + Vec2::new(rect.width() * 0.66, 145.),
        &format!("t_cor  {}", value(corrected, "ps")),
        TEXT,
    );
    let unit_rect =
        egui::Rect::from_min_size(rect.min + Vec2::new(14., 175.), Vec2::new(138., 26.));
    unit_selector(
        render.ui,
        unit_rect,
        ("delay-unit", slot),
        &mut render.alignment.delay_unit,
        !render.locked,
        true,
    );
    number(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(158., 175.), Vec2::new(78., 26.)),
        &mut render.alignment.delay_step[slot],
        !render.locked,
    );
    label(
        render.ui,
        rect.min + Vec2::new(240., 181.),
        "increment",
        MUTED,
    );
    let action_y = 213.;
    let amount = render.alignment.delay_step[slot];
    let unit = render.alignment.delay_unit;
    let locked = render.locked || !connected;
    step_hold_pair(
        render,
        StepControl {
            position: rect.min + Vec2::new(14., action_y),
            kind: MotionKind::Delay,
            id: &panel.option.id,
            axis: 0,
            amount,
            unit,
            positive: false,
            locked,
        },
    );
    step_hold_pair(
        render,
        StepControl {
            position: rect.min + Vec2::new(154., action_y),
            kind: MotionKind::Delay,
            id: &panel.option.id,
            axis: 0,
            amount,
            unit,
            positive: true,
            locked,
        },
    );
    let end_left = button(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(294., action_y), Vec2::new(62., 26.)),
        "End <",
        !render.locked && connected,
    );
    let end_right = button(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(360., action_y), Vec2::new(62., 26.)),
        "End >",
        !render.locked && connected,
    );
    if end_left.clicked() {
        render
            .intents
            .push(end(MotionKind::Delay, &panel.option.id, 0, false));
    }
    if end_right.clicked() {
        render
            .intents
            .push(end(MotionKind::Delay, &panel.option.id, 0, true));
    }
    let init = button(
        render.ui,
        egui::Rect::from_min_size(
            rect.min + Vec2::new(rect.width() - 160., action_y),
            Vec2::new(78., 26.),
        ),
        "Initialize",
        !render.locked && connected,
    );
    let stop = button(
        render.ui,
        egui::Rect::from_min_size(
            rect.min + Vec2::new(rect.width() - 78., action_y),
            Vec2::new(64., 26.),
        ),
        "Stop",
        true,
    );
    if init.clicked() {
        render.intents.push(Intent::Initialize(InitializeTarget {
            kind: MotionKind::Delay,
            id: panel.option.id.clone(),
            axis: 0,
            name: panel.option.name.clone(),
            default_home: true,
        }));
    }
    if stop.clicked() {
        render.intents.push(Intent::Stop);
    }
    let desired = direction_selector(
        render.ui,
        egui::Rect::from_min_size(
            rect.min + Vec2::new(rect.width() - 160., 175.),
            Vec2::new(146., 26.),
        ),
        panel.entry.config.direction != 0,
        !render.locked && connected,
    );
    if desired.is_some_and(|reversed| reversed != (panel.entry.config.direction != 0)) {
        render.intents.push(Intent::Reverse {
            kind: MotionKind::Delay,
            id: panel.option.id.clone(),
            axis: 0,
        });
    }
}

fn draw_scanner_card(
    render: &mut RenderState<'_>,
    rect: egui::Rect,
    slot: usize,
    options: &[DeviceOption],
    panel: Option<&ScannerPanel>,
) {
    card(render.ui, rect, &format!("Scanner {}", slot + 1));
    selector(
        render.ui,
        rect,
        ("scanner-select", slot),
        &mut render.alignment.scanner_panels[slot],
        options,
        render.locked,
    );
    let Some(panel) = panel else {
        label(
            render.ui,
            rect.min + Vec2::new(14., 62.),
            "Choose a scanner",
            MUTED,
        );
        return;
    };
    let axes = panel.state.as_ref().map_or([false; 3], |s| s.connected);
    status(
        render.ui,
        rect,
        axes[1] || axes[2],
        panel.state.as_ref().map_or("Not connected", |s| &s.phase),
    );
    let position = panel.state.as_ref().map(|s| &s.position);
    let x = position.and_then(|p| p.physical[1]);
    let y = position.and_then(|p| p.physical[2]);
    crosshair(
        render.ui,
        egui::Rect::from_min_size(rect.min + Vec2::new(14., 46.), Vec2::new(82., 74.)),
        x,
        y,
    );
    for (offset, name, value, zero) in [
        (50., "X", x, panel.entry.zero_um[0]),
        (80., "Y", y, panel.entry.zero_um[1]),
    ] {
        label(
            render.ui,
            rect.min + Vec2::new(108., offset),
            &format!("{name}  {} µm", compact(value)),
            TEXT,
        );
        label(
            render.ui,
            rect.min + Vec2::new(108., offset + 15.),
            &format!("{name}_cor  {} µm", compact(value.map(|v| v - zero))),
            MUTED,
        );
    }
    let base = rect.min;
    unit_selector(
        render.ui,
        egui::Rect::from_min_size(base + Vec2::new(14., 139.), Vec2::new(84., 26.)),
        ("scanner-unit", slot),
        &mut render.alignment.scanner_unit[slot],
        !render.locked,
        false,
    );
    for (axis, offset) in [(0, 111.), (1, 236.)] {
        label(
            render.ui,
            base + Vec2::new(offset, 124.),
            if axis == 0 { "X step" } else { "Y step" },
            MUTED,
        );
        number(
            render.ui,
            egui::Rect::from_min_size(base + Vec2::new(offset, 139.), Vec2::new(96., 26.)),
            &mut render.alignment.scanner_step[slot][axis],
            !render.locked,
        );
    }
    for (axis, offset, config) in [
        (1, 14., &panel.entry.x),
        (2, rect.width() - 160., &panel.entry.y),
    ] {
        let reversed = config.as_ref().is_some_and(|c| c.direction != 0);
        label(
            render.ui,
            base + Vec2::new(offset, 174.),
            if axis == 1 {
                "X direction"
            } else {
                "Y direction"
            },
            MUTED,
        );
        if direction_selector(
            render.ui,
            egui::Rect::from_min_size(base + Vec2::new(offset, 190.), Vec2::new(146., 24.)),
            reversed,
            !render.locked && axes[axis],
        )
        .is_some_and(|v| v != reversed)
        {
            render.intents.push(Intent::Reverse {
                kind: MotionKind::Scanner,
                id: panel.option.id.clone(),
                axis,
            });
        }
    }
    // The selected mode applies to the same spatially arranged direction buttons.
    for (hold, title, x) in [(false, "Step", 14.), (true, "Hold", 76.)] {
        let r = egui::Rect::from_min_size(base + Vec2::new(x, 228.), Vec2::new(58., 26.));
        if render
            .ui
            .add_enabled_ui(!render.locked, |ui| {
                ui.put(
                    r,
                    egui::Button::new(title).selected(render.alignment.scanner_hold[slot] == hold),
                )
            })
            .inner
            .clicked()
        {
            render.alignment.scanner_hold[slot] = hold;
        }
    }
    let hold = render.alignment.scanner_hold[slot];
    let unit = render.alignment.scanner_unit[slot];
    let center_x = rect.width() - 115.;
    for (axis, positive, x, y, title) in [
        (1, false, center_x - 66., 260., "< X"),
        (1, true, center_x + 66., 260., "X >"),
        (2, true, center_x, 230., "Y +"),
        (2, false, center_x, 290., "Y -"),
    ] {
        let amount = render.alignment.scanner_step[slot][axis - 1];
        let control = StepControl {
            position: base + Vec2::new(x - 29., y),
            kind: MotionKind::Scanner,
            id: &panel.option.id,
            axis,
            amount,
            unit,
            positive,
            locked: render.locked || !axes[axis],
        };
        motion_button(render, &control, title, hold);
    }
    if button(
        render.ui,
        egui::Rect::from_min_size(base + Vec2::new(14., 260.), Vec2::new(120., 26.)),
        "Stop",
        true,
    )
    .clicked()
    {
        render.intents.push(Intent::Stop);
    }
    for (axis, offset, config) in [(1, 14., &panel.entry.x), (2, 76., &panel.entry.y)] {
        if button(
            render.ui,
            egui::Rect::from_min_size(base + Vec2::new(offset, 290.), Vec2::new(58., 26.)),
            if axis == 1 { "Init X" } else { "Init Y" },
            !render.locked && axes[axis],
        )
        .clicked()
        {
            render.intents.push(Intent::Initialize(InitializeTarget {
                kind: MotionKind::Scanner,
                id: panel.option.id.clone(),
                axis,
                name: format!(
                    "{} {}",
                    panel.option.name,
                    if axis == 1 { "X" } else { "Y" }
                ),
                default_home: config
                    .as_ref()
                    .is_some_and(|c| c.controller.eq_ignore_ascii_case("CONEXCC")),
            }));
        }
    }
}

fn valid_increment(amount: f64) -> bool {
    amount.is_finite() && amount > 0.
}

fn step_hold_pair(render: &mut RenderState<'_>, mut control: StepControl<'_>) {
    let sign = if control.positive { "+" } else { "-" };
    motion_button(render, &control, &format!("Step {sign}"), false);
    control.position.x += 62.;
    motion_button(render, &control, &format!("Hold {sign}"), true);
}

fn motion_button(
    render: &mut RenderState<'_>,
    control: &StepControl<'_>,
    title: &str,
    hold_mode: bool,
) {
    let held = HeldControl {
        kind: control.kind,
        id: control.id.into(),
        axis: control.axis,
        positive: control.positive,
    };
    let active = hold_mode && render.alignment.held.as_ref() == Some(&held);
    let enabled = !control.locked && valid_increment(control.amount);
    let response = button(
        render.ui,
        egui::Rect::from_min_size(control.position, Vec2::new(58., 26.)),
        title,
        enabled || active,
    );
    if !valid_increment(control.amount) {
        response
            .clone()
            .on_disabled_hover_text("Enter a finite, positive increment");
    }
    if !hold_mode && response.clicked() && enabled {
        render.intents.push(Intent::Command {
            kind: control.kind,
            id: control.id.into(),
            action: AlignmentAction::Step {
                axis: control.axis,
                amount: if control.positive {
                    control.amount
                } else {
                    -control.amount
                },
                unit: control.unit,
            },
        });
    }
    let pointer_pressed = render.ui.ctx().input(|i| i.pointer.any_pressed());
    let down = response.is_pointer_button_down_on();
    if hold_mode && down && pointer_pressed && render.alignment.held.is_none() && enabled {
        render.intents.push(Intent::Hold {
            control: held,
            amount: control.amount,
            unit: control.unit,
        });
    }
    if active && down {
        *render.held_down = true;
    }
}

fn end(kind: MotionKind, id: &str, axis: usize, positive: bool) -> Intent {
    Intent::Command {
        kind,
        id: id.into(),
        action: AlignmentAction::End { axis, positive },
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
    label(ui, rect.min + Vec2::new(14., 12.), title, TEXT);
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
    ui.painter()
        .circle_filled(rect.right_top() + Vec2::new(-105., 20.), 3., color);
    let text_rect =
        egui::Rect::from_min_size(rect.right_top() + Vec2::new(-96., 13.), Vec2::new(88., 22.));
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(text_rect))
        .text(
            text_rect.min,
            egui::Align2::LEFT_TOP,
            if phase.starts_with("Error") {
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
        rect.min + Vec2::new(78., 7.),
        Vec2::new((rect.width() * 0.38).min(230.), 28.),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
        ui.add_enabled_ui(!locked, |ui| {
            ui.spacing_mut().combo_width = field.width();
            egui::ComboBox::from_id_salt(id)
                .width(field.width())
                .selected_text(
                    selected
                        .as_deref()
                        .and_then(|value| options.iter().find(|option| option.id == value))
                        .map(|option| option.name.as_str())
                        .unwrap_or("—"),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(selected, None, "—");
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

/// Compact segmented state selector. The caller applies the conversion through
/// `reverse_device_direction`, which preserves the selected coordinate without
/// emitting a motion command.
fn direction_selector(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    reversed: bool,
    enabled: bool,
) -> Option<bool> {
    let width = (rect.width() - 3.) * 0.5;
    let normal = egui::Rect::from_min_size(rect.min, Vec2::new(width, rect.height()));
    let reverse = egui::Rect::from_min_size(
        egui::pos2(normal.max.x + 3., rect.min.y),
        Vec2::new(width, rect.height()),
    );
    let normal = ui
        .add_enabled_ui(enabled, |ui| {
            ui.put(
                normal,
                egui::Button::new("Normal")
                    .selected(!reversed)
                    .min_size(normal.size())
                    .corner_radius(5),
            )
        })
        .inner;
    let reverse = ui
        .add_enabled_ui(enabled, |ui| {
            ui.put(
                reverse,
                egui::Button::new("Reverse")
                    .selected(reversed)
                    .min_size(reverse.size())
                    .corner_radius(5),
            )
        })
        .inner;
    if normal.clicked() {
        Some(false)
    } else if reverse.clicked() {
        Some(true)
    } else {
        None
    }
}
fn number(ui: &mut egui::Ui, rect: egui::Rect, value: &mut f64, enabled: bool) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.put(
            rect,
            egui::DragValue::new(value)
                .speed(0.1)
                .range(-1e12..=1e12)
                .max_decimals(4),
        );
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
fn rail(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    position: Option<f64>,
    entry: &DelayEntry,
    micro: u32,
) {
    let start = rect.left() + 112.;
    let end = rect.right() - 40.;
    let y = rect.top() + 10.;
    let track = egui::Rect::from_min_max(egui::pos2(start, y), egui::pos2(end, y + 6.));
    ui.painter().rect_filled(track, 3., BORDER);
    let Ok((minimum, maximum)) = entry
        .config
        .normalized()
        .and_then(|c| c.native_limits_mm(micro))
    else {
        label(ui, rect.left_top(), "Configure stage travel", MUTED);
        return;
    };
    if let Some(p) = position {
        let x = start + ((p - minimum) / (maximum - minimum)).clamp(0., 1.) as f32 * (end - start);
        ui.painter().rect_filled(
            egui::Rect::from_center_size(egui::pos2(x, y), Vec2::new(30., 16.)),
            4.,
            ACCENT,
        );
        ui.painter().line_segment(
            [egui::pos2(x, y + 9.), egui::pos2(x, rect.bottom())],
            Stroke::new(1_f32, ACCENT),
        );
    }
    for (row, name) in ["mm", "t (ps)", "t_cor (ps)"].into_iter().enumerate() {
        let axis_y = rect.top() + 32. + row as f32 * 20.;
        label(ui, egui::pos2(rect.left(), axis_y), name, MUTED);
        for (fraction, native) in [
            (0., minimum),
            (0.5, (minimum + maximum) / 2.),
            (1., maximum),
        ] {
            let value = match row {
                0 => Some(native),
                _ => entry
                    .config
                    .measurement_ps_from_mm_with_micro(native, micro)
                    .ok()
                    .map(|v| if row == 2 { v - entry.zero_ps } else { v }),
            };
            ui.painter().text(
                egui::pos2(start + fraction * (end - start), axis_y),
                egui::Align2::CENTER_TOP,
                compact(value),
                egui::FontId::proportional(11.),
                MUTED,
            );
        }
    }
}

fn crosshair(ui: &mut egui::Ui, rect: egui::Rect, x: Option<f64>, y: Option<f64>) {
    ui.painter().rect(
        rect,
        5.,
        Color32::from_rgb(21, 22, 27),
        Stroke::new(1_f32, BORDER),
        egui::StrokeKind::Inside,
    );
    let center = rect.center();
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 8., center.y),
            egui::pos2(rect.right() - 8., center.y),
        ],
        Stroke::new(1_f32, BORDER),
    );
    ui.painter().line_segment(
        [
            egui::pos2(center.x, rect.top() + 8.),
            egui::pos2(center.x, rect.bottom() - 8.),
        ],
        Stroke::new(1_f32, BORDER),
    );
    label(ui, rect.right_center() - Vec2::new(20., 16.), "+X", MUTED);
    label(ui, rect.center_top() + Vec2::new(5., 2.), "+Y", MUTED);
    if x.is_some() && y.is_some() {
        ui.painter().circle_filled(center, 4., ACCENT);
    }
}
fn compact(value: Option<f64>) -> String {
    value
        .map(|value| format!("{:.3}", if value.abs() < 0.0005 { 0. } else { value }))
        .unwrap_or_else(|| "—".into())
}
fn value(value: Option<f64>, unit: &str) -> String {
    format!("{} {unit}", compact(value))
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
    fn panel_binding_uses_stable_id_not_display_order() {
        let mut state = AlignmentUi::default();
        state.scanner_panels[0] = Some("scanner-2".into());
        let mut reversed = devices();
        reversed.reverse();
        assert_eq!(state.scanner_id(0, &reversed).as_deref(), Some("scanner-2"));
    }

    #[test]
    fn stale_panel_binding_falls_back_to_available_device() {
        let mut state = AlignmentUi::default();
        state.delay_panels[0] = Some("removed".into());
        let delay = vec![DeviceOption {
            id: "delay-2".into(),
            name: "Delay 2".into(),
        }];
        assert_eq!(state.delay_id(0, &delay).as_deref(), Some("delay-2"));
    }
    fn pointer_frame(
        context: &egui::Context,
        held: Option<HeldControl>,
        amount: f64,
        locked: bool,
        hold_mode: bool,
        pressed: bool,
    ) -> (Vec<Intent>, bool) {
        let pointer = egui::pos2(40., 40.);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(400., 300.),
            )),
            events: vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let mut alignment = AlignmentUi {
            held,
            ..Default::default()
        };
        let mut intents = Vec::new();
        let mut down = false;
        let _ = context.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut render = RenderState {
                    ui,
                    alignment: &mut alignment,
                    locked,
                    intents: &mut intents,
                    held_down: &mut down,
                };
                motion_button(
                    &mut render,
                    &StepControl {
                        position: egui::pos2(20., 20.),
                        kind: MotionKind::Scanner,
                        id: "scanner-2",
                        axis: 1,
                        amount,
                        unit: AlignmentUnit::Physical,
                        positive: false,
                        locked,
                    },
                    "Move",
                    hold_mode,
                );
            });
        });
        (intents, down)
    }

    #[test]
    fn pointer_step_preserves_exact_requested_distance_and_rejects_invalid_input() {
        for (amount, locked, allowed) in [
            (2.5, false, true),
            (0., false, false),
            (-1., false, false),
            (f64::NAN, false, false),
            (f64::INFINITY, false, false),
            (2.5, true, false),
        ] {
            let ctx = egui::Context::default();
            pointer_frame(&ctx, None, amount, locked, false, false);
            pointer_frame(&ctx, None, amount, locked, false, true);
            let (intents, _) = pointer_frame(&ctx, None, amount, locked, false, false);
            assert_eq!(intents.len(), usize::from(allowed));
            if allowed {
                assert!(
                    matches!(&intents[0], Intent::Command {id, action: AlignmentAction::Step {axis: 1, amount, ..}, ..}
                    if id == "scanner-2" && *amount == -2.5)
                );
            }
        }
    }

    #[test]
    fn held_pointer_remains_active_while_worker_is_busy_then_releases() {
        let ctx = egui::Context::default();
        pointer_frame(&ctx, None, 1., false, true, false);
        let (intents, _) = pointer_frame(&ctx, None, 1., false, true, true);
        assert!(matches!(intents.first(), Some(Intent::Hold { .. })));
        let held = HeldControl {
            kind: MotionKind::Scanner,
            id: "scanner-2".into(),
            axis: 1,
            positive: false,
        };
        let (_, down) = pointer_frame(&ctx, Some(held.clone()), 1., true, true, true);
        assert!(down);
        let (intents, down) = pointer_frame(&ctx, Some(held), 1., true, true, false);
        assert!(!down);
        assert!(intents.is_empty());
    }
}
