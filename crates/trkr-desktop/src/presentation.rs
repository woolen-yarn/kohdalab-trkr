//! Python-compatible plot arrangement and coordinate presentation.
use crate::engine::Sample;
use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions, Vec2};
use egui_plot::{AxisHints, Line, Plot, PlotImage, PlotPoint, VPlacement};
use trkr_core::{Axis, DisplayMode, Plan, Workflow};

/// Per-channel raster state for a 2D scan.  The UI only appends samples (or
/// replaces the whole trace and advances its epoch), so the typed key lets us
/// keep both the target grid and GPU texture across repaint-only frames.
pub type MapCache = [Option<HeatmapCache>; 2];

pub struct HeatmapCache {
    key: HeatmapKey,
    xs: Vec<f64>,
    ys: Vec<f64>,
    extent: HeatmapExtent,
    texture: TextureHandle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HeatmapExtent {
    width: f64,
    height: f64,
    center: PlotPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SampleFingerprint {
    fast: u64,
    slow: Option<u64>,
    x: u64,
    y: u64,
    r: u64,
    theta: u64,
}

impl From<&Sample> for SampleFingerprint {
    fn from(sample: &Sample) -> Self {
        Self {
            fast: sample.target.fast.to_bits(),
            slow: sample.target.slow.map(f64::to_bits),
            x: sample.signal.x_v.to_bits(),
            y: sample.signal.y_v.to_bits(),
            r: sample.signal.r_v.to_bits(),
            theta: sample.signal.theta_deg.to_bits(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeatmapKey {
    epoch: u64,
    workflow: Workflow,
    mode: DisplayMode,
    fast: Axis,
    slow: Axis,
    signal: usize,
    voltage_scale: u64,
    sample_count: usize,
    last_sample: Option<SampleFingerprint>,
}

impl HeatmapKey {
    fn new(view: &PlotView<'_>, signal: usize) -> Self {
        Self {
            epoch: view.epoch,
            workflow: view.plan.workflow,
            mode: view.mode,
            fast: view.plan.fast,
            slow: view.plan.slow,
            signal,
            voltage_scale: view.voltage_scale.to_bits(),
            sample_count: view.samples.len(),
            last_sample: view.samples.last().map(SampleFingerprint::from),
        }
    }
}

struct PreparedHeatmap {
    xs: Vec<f64>,
    ys: Vec<f64>,
    extent: HeatmapExtent,
    image: egui::ColorImage,
}

pub struct PlotViewport<'a> {
    pub area: Rect,
    pub epoch: u64,
    pub y_log: bool,
    pub voltage_scale: f64,
    pub voltage_unit: &'a str,
    pub history: &'a crate::plot_history::History,
}

pub fn voltage_scale_from_sensitivity(sensitivity_v: f64) -> (f64, &'static str) {
    if sensitivity_v < 1e-6 {
        (1e9, "nV")
    } else if sensitivity_v < 1e-3 {
        (1e6, "uV")
    } else if sensitivity_v < 1. {
        (1e3, "mV")
    } else {
        (1., "V")
    }
}

pub fn axis_value(sample: &Sample, axis: Axis, plan: &Plan) -> f64 {
    let physical = sample.position.physical;
    let corrected: [f64; 3] = std::array::from_fn(|i| {
        physical[i]
            .map(|v| v - plan.zero[i])
            .unwrap_or(sample.target.corrected[i])
    });
    let (s, c) = plan.theta_deg.to_radians().sin_cos();
    match axis {
        Axis::T => corrected[0],
        Axis::X => corrected[1],
        Axis::Y => corrected[2],
        Axis::U => corrected[1] * c + corrected[2] * s,
        Axis::V => -corrected[1] * s + corrected[2] * c,
    }
}
fn offset(axis: Axis, plan: &Plan) -> f64 {
    match axis {
        Axis::T => plan.zero[0],
        Axis::X => plan.zero[1],
        Axis::Y => plan.zero[2],
        _ => 0.,
    }
}
fn color(i: usize) -> Color32 {
    if i == 0 {
        Color32::from_rgb(102, 174, 255)
    } else {
        Color32::from_rgb(245, 183, 82)
    }
}
pub fn plot_count(workflow: Workflow) -> usize {
    workflow.plot_count()
}
struct PlotView<'a> {
    epoch: u64,
    y_log: bool,
    samples: &'a [Sample],
    plan: &'a Plan,
    mode: DisplayMode,
    voltage_scale: f64,
    voltage_unit: &'a str,
}

struct LineSpec {
    signal: usize,
    axis: Axis,
    id: &'static str,
    current_line: bool,
    show_data: bool,
}

impl PlotView<'_> {
    fn signal_scale(&self, signal: usize) -> f64 {
        if self.mode == DisplayMode::RTheta && signal == 1 {
            1.
        } else {
            self.voltage_scale
        }
    }
    fn signal_label(&self, signal: usize) -> String {
        match (self.mode, signal) {
            (DisplayMode::RTheta, 0) => format!("R ({})", self.voltage_unit),
            (DisplayMode::RTheta, _) => "Theta (deg)".into(),
            (_, 0) => format!("X ({})", self.voltage_unit),
            (_, _) => format!("Y ({})", self.voltage_unit),
        }
    }
    fn line(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        geometry: &mut crate::precision::Geometry,
        name: &str,
        spec: LineSpec,
    ) {
        crate::precision::record(geometry, name, rect);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| self.draw_line(ui, rect, spec),
        );
    }
    fn draw_line(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        spec: LineSpec,
    ) -> egui_plot::PlotResponse<()> {
        let monitor = self.plan.workflow == Workflow::SignalMonitor;
        let label = if monitor {
            "elapsed (s)".to_owned()
        } else {
            format!("{}_cor ({})", spec.axis.name(), spec.axis.unit())
        };
        let zero = offset(spec.axis, self.plan);
        let mut plot = Plot::new((
            self.plan.workflow.key(),
            spec.id,
            spec.signal,
            spec.axis.name(),
        ))
        .width(rect.width())
        .height(rect.height())
        .x_axis_label(&label)
        .y_axis_label(self.signal_label(spec.signal));
        let log = self.y_log && self.plan.workflow == Workflow::Trkr;
        let last_line = self.samples.last().map(|sample| sample.target.line);
        let points: Vec<[f64; 2]> = if spec.show_data {
            self.samples
                .iter()
                .filter(|sample| !spec.current_line || Some(sample.target.line) == last_line)
                .map(|sample| {
                    [
                        if monitor {
                            sample.elapsed
                        } else {
                            axis_value(sample, spec.axis, self.plan)
                        },
                        self.mode.values(sample.signal)[spec.signal]
                            * self.signal_scale(spec.signal),
                    ]
                })
                .collect()
        } else {
            vec![]
        };
        let mut segments: Vec<Vec<[f64; 2]>> = vec![Vec::new()];
        for [x, y] in points {
            if let Some(y) = plotted_y(y, log) {
                segments.last_mut().unwrap().push([x, y]);
            } else if !segments.last().unwrap().is_empty() {
                segments.push(Vec::new());
            }
        }
        let has_data = segments.iter().any(|s| !s.is_empty());
        // Empty placeholders have fixed bounds. Their memory must not become
        // the initial zoom of a live/data plot. Modes and new runs likewise
        // start with bounds in their own units, while preserving manual zoom
        // during a run.
        plot = plot.id(egui::Id::new((
            self.plan.workflow.key(),
            spec.id,
            spec.signal,
            spec.axis.name(),
            self.mode == DisplayMode::RTheta,
            self.voltage_scale.to_bits(),
            self.epoch,
            has_data,
            log,
        )));
        plot = if has_data {
            plot.auto_bounds(true)
        } else {
            plot.default_x_bounds(0., 1.)
                .default_y_bounds(0., 1.)
                .auto_bounds(false)
        };
        if log {
            plot = plot
                .y_axis_formatter(|mark, _| format_log_tick(mark.value))
                .label_formatter(|name, point| format!("{name}: {:.6e}", 10_f64.powf(point.y)));
        }
        if !monitor {
            plot = plot.custom_x_axes(vec![
                AxisHints::new_x().label(&label),
                AxisHints::new_x()
                    .placement(VPlacement::Top)
                    .label(format!("{} ({})", spec.axis.name(), spec.axis.unit()))
                    .formatter(move |mark, _| {
                        if has_data {
                            format!("{:.3}", mark.value + zero)
                        } else {
                            String::new()
                        }
                    }),
            ]);
        }
        plot.show(ui, |p| {
            for segment in segments.into_iter().filter(|s| !s.is_empty()) {
                p.line(
                    Line::new(self.signal_label(spec.signal), segment)
                        .color(color(spec.signal))
                        .width(1.7_f32),
                );
            }
            if log && !has_data {
                p.text(egui_plot::Text::new(
                    "log-empty",
                    PlotPoint::new(0.5, 0.5),
                    "Log: no positive values",
                ));
            }
        })
    }
}
fn plotted_y(value: f64, log: bool) -> Option<f64> {
    if !value.is_finite() || (log && value <= 0.) {
        None
    } else {
        Some(if log { value.log10() } else { value })
    }
}
fn format_log_tick(exponent: f64) -> String {
    if (exponent - exponent.round()).abs() < 1e-7 {
        format!("1e{:.0}", exponent)
    } else {
        format!("{:.2e}", 10_f64.powf(exponent))
    }
}
const RDBU_R_LUT: &[u8; 768] = include_bytes!("../assets/rdbu_r_lut.rgb");
fn diverging(value: f64, max: f64) -> Color32 {
    let v = if max > 0. {
        (value / max).clamp(-1., 1.)
    } else {
        0.
    };
    // pyqtgraph makeARGB uses 256/(upper-lower), truncated and clipped.
    let index = ((v + 1.) * 128.).clamp(0., 255.) as usize;
    let rgb = &RDBU_R_LUT[index * 3..index * 3 + 3];
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn unique(values: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| *a == *b);
    values
}

fn heatmap_extent(xs: &[f64], ys: &[f64]) -> HeatmapExtent {
    let width = if xs.len() > 1 {
        xs[xs.len() - 1] - xs[0]
    } else {
        1.
    };
    let height = if ys.len() > 1 {
        ys[ys.len() - 1] - ys[0]
    } else {
        1.
    };
    HeatmapExtent {
        width,
        height,
        center: PlotPoint::new(xs[0] + width * 0.5, ys[0] + height * 0.5),
    }
}

fn prepare_heatmap(
    samples: &[Sample],
    mode: DisplayMode,
    signal: usize,
    signal_scale: f64,
) -> Option<PreparedHeatmap> {
    let xs = unique(samples.iter().map(|sample| sample.target.fast));
    let ys = unique(samples.iter().filter_map(|sample| sample.target.slow));
    if xs.is_empty()
        || ys.is_empty()
        || xs.len().saturating_mul(ys.len()) > trkr_core::MAX_TOTAL_POINTS
    {
        return None;
    }
    let max = samples
        .iter()
        .map(|sample| (mode.values(sample.signal)[signal] * signal_scale).abs())
        .fold(0., f64::max);
    let mut image = egui::ColorImage::filled([xs.len(), ys.len()], Color32::TRANSPARENT);
    for sample in samples {
        if let Some(y) = sample.target.slow
            && let (Ok(xi), Ok(yi)) = (
                xs.binary_search_by(|v| v.total_cmp(&sample.target.fast)),
                ys.binary_search_by(|v| v.total_cmp(&y)),
            )
        {
            image.pixels[(ys.len() - 1 - yi) * xs.len() + xi] =
                diverging(mode.values(sample.signal)[signal] * signal_scale, max);
        }
    }
    Some(PreparedHeatmap {
        extent: heatmap_extent(&xs, &ys),
        xs,
        ys,
        image,
    })
}

fn heatmap_needs_rebuild(cached_key: Option<HeatmapKey>, key: HeatmapKey) -> bool {
    cached_key != Some(key)
}

impl PlotView<'_> {
    fn heatmap(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        geometry: &mut crate::precision::Geometry,
        name: &str,
        signal: usize,
        cache: &mut Option<HeatmapCache>,
    ) {
        crate::precision::record(geometry, name, rect);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| self.draw_heatmap(ui, rect, signal, cache),
        );
    }
    fn draw_heatmap(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        signal: usize,
        cache: &mut Option<HeatmapCache>,
    ) {
        let key = HeatmapKey::new(self, signal);
        if heatmap_needs_rebuild(cache.as_ref().map(|cache| cache.key), key) {
            match prepare_heatmap(self.samples, self.mode, signal, self.signal_scale(signal)) {
                Some(prepared) => match cache {
                    Some(cache) => {
                        cache.texture.set(prepared.image, TextureOptions::NEAREST);
                        cache.key = key;
                        cache.xs = prepared.xs;
                        cache.ys = prepared.ys;
                        cache.extent = prepared.extent;
                    }
                    None => {
                        let texture = ui.ctx().load_texture(
                            format!("map-{signal}"),
                            prepared.image,
                            TextureOptions::NEAREST,
                        );
                        *cache = Some(HeatmapCache {
                            key,
                            xs: prepared.xs,
                            ys: prepared.ys,
                            extent: prepared.extent,
                            texture,
                        });
                    }
                },
                None => *cache = None,
            }
        }
        let has_data = cache.is_some();
        let mut plot = Plot::new((self.plan.workflow.key(), "map", signal))
            .id(egui::Id::new((
                self.plan.workflow.key(),
                "map",
                signal,
                self.plan.fast.name(),
                self.plan.slow.name(),
                self.mode == DisplayMode::RTheta,
                self.voltage_scale.to_bits(),
                self.epoch,
                has_data,
            )))
            .width(rect.width())
            .height(rect.height())
            .x_axis_label(format!(
                "{}_cor ({})",
                self.plan.fast.name(),
                self.plan.fast.unit()
            ))
            .y_axis_label(format!(
                "{}_cor ({})",
                self.plan.slow.name(),
                self.plan.slow.unit()
            ));
        let zero = offset(self.plan.fast, self.plan);
        plot = plot.custom_x_axes(vec![
            AxisHints::new_x().label(format!(
                "{}_cor ({})",
                self.plan.fast.name(),
                self.plan.fast.unit()
            )),
            AxisHints::new_x()
                .placement(VPlacement::Top)
                .label(format!(
                    "{} ({})",
                    self.plan.fast.name(),
                    self.plan.fast.unit()
                ))
                .formatter(move |mark, _| {
                    if has_data {
                        format!("{:.3}", mark.value + zero)
                    } else {
                        String::new()
                    }
                }),
        ]);
        if self.plan.fast != Axis::T && self.plan.slow != Axis::T {
            plot = plot.data_aspect(1.);
        }
        if !has_data {
            plot = plot
                .default_x_bounds(0., 1.)
                .default_y_bounds(0., 1.)
                .auto_bounds(false);
            plot.show(ui, |_| {});
            return;
        }
        plot = plot.auto_bounds(true);
        let cache = cache
            .as_ref()
            .expect("valid heatmap cache when data exists");
        // Match Python ImageItem.setRect(min_fast,min_slow,max-min,max-min).
        let size = Vec2::new(cache.extent.width as f32, cache.extent.height as f32);
        plot.show(ui, |p| {
            p.image(PlotImage::new(
                self.signal_label(signal),
                cache.texture.id(),
                cache.extent.center,
                size,
            ))
        })
        .response
        .on_hover_text(format!(
            "{} · target grid · normalized ±1",
            self.signal_label(signal)
        ));
    }
}
pub fn plots(
    ui: &mut egui::Ui,
    samples: &[Sample],
    plan: &Plan,
    mode: DisplayMode,
    cache: &mut MapCache,
    geometry: &mut crate::precision::Geometry,
    viewport: PlotViewport<'_>,
) {
    let view = PlotView {
        epoch: viewport.epoch,
        y_log: viewport.y_log,
        samples,
        plan,
        mode,
        voltage_scale: viewport.voltage_scale,
        voltage_unit: viewport.voltage_unit,
    };
    let gap = 8.;
    let first_height = ((viewport.area.height() - gap).max(0.) * 0.5).ceil();
    let second_height = (viewport.area.height() - gap - first_height)
        .max(0.)
        .floor();
    let row_rect = |signal: usize| {
        if signal == 0 {
            Rect::from_min_size(
                viewport.area.min,
                Vec2::new(viewport.area.width(), first_height),
            )
        } else {
            Rect::from_min_size(
                egui::pos2(
                    viewport.area.min.x,
                    viewport.area.min.y + first_height + gap,
                ),
                Vec2::new(viewport.area.width(), second_height),
            )
        }
    };
    for (signal, map_cache) in cache.iter_mut().enumerate() {
        let row = row_rect(signal);
        if matches!(plan.workflow, Workflow::SignalMonitor | Workflow::Trkr) {
            view.line(
                ui,
                row,
                geometry,
                if signal == 0 { "plot1" } else { "plot2" },
                LineSpec {
                    signal,
                    axis: Axis::T,
                    id: "standard",
                    current_line: false,
                    show_data: true,
                },
            );
        } else if plan.workflow == Workflow::Srkr {
            let axes = if matches!(plan.fast, Axis::U | Axis::V) {
                [Axis::U, Axis::V]
            } else {
                [Axis::X, Axis::Y]
            };
            let left_width = ((row.width() - gap).max(0.) * 0.5).ceil();
            let right_width = (row.width() - gap - left_width).max(0.).floor();
            for (column, (axis, width, x)) in [
                (axes[0], left_width, row.min.x),
                (axes[1], right_width, row.min.x + left_width + gap),
            ]
            .into_iter()
            .enumerate()
            {
                let rect =
                    Rect::from_min_size(egui::pos2(x, row.min.y), Vec2::new(width, row.height()));
                let retained = (axis != plan.fast)
                    .then(|| viewport.history.srkr_axis(plan, axis))
                    .flatten();
                let axis_view = PlotView {
                    samples: retained
                        .map(|trace| trace.samples.as_slice())
                        .unwrap_or(view.samples),
                    plan: retained.map(|trace| &trace.plan).unwrap_or(view.plan),
                    ..view
                };
                axis_view.line(
                    ui,
                    rect,
                    geometry,
                    &format!("srkr_{}_{}", axis.name().to_ascii_lowercase(), signal + 1),
                    LineSpec {
                        signal,
                        axis,
                        id: if column == 0 {
                            "srkr-left"
                        } else {
                            "srkr-right"
                        },
                        current_line: false,
                        show_data: axis == plan.fast || retained.is_some(),
                    },
                );
            }
        } else {
            let left_width = ((row.width() - gap).max(0.) * 0.5).ceil();
            let right_width = (row.width() - gap - left_width).max(0.).floor();
            let line_rect = Rect::from_min_size(row.min, Vec2::new(left_width, row.height()));
            let map_rect = Rect::from_min_size(
                egui::pos2(row.min.x + left_width + gap, row.min.y),
                Vec2::new(right_width, row.height()),
            );
            view.line(
                ui,
                line_rect,
                geometry,
                &format!("scan2d_line_{}", signal + 1),
                LineSpec {
                    signal,
                    axis: plan.fast,
                    id: "latest-line",
                    current_line: true,
                    show_data: true,
                },
            );
            view.heatmap(
                ui,
                map_rect,
                geometry,
                &format!("scan2d_map_{}", signal + 1),
                signal,
                map_cache,
            );
        }
    }
}
/// Acceptance evidence: verify the rendered data views actually contain their
/// samples. A sample count alone cannot detect a stale empty-plot viewport.
pub fn verify_visible_data(
    ctx: &egui::Context,
    samples: &[Sample],
    plan: &Plan,
    mode: DisplayMode,
    scale: f64,
    epoch: u64,
    y_log: bool,
) -> Result<(), String> {
    if samples.is_empty() {
        return Err("No replay plot samples".into());
    }
    let log = y_log && plan.workflow == Workflow::Trkr;
    for signal in 0..2 {
        let spec_id = match plan.workflow {
            Workflow::SignalMonitor | Workflow::Trkr => "standard",
            Workflow::Srkr if matches!(plan.fast, Axis::Y | Axis::V) => "srkr-right",
            Workflow::Srkr => "srkr-left",
            _ => "latest-line",
        };
        let axis = if matches!(plan.workflow, Workflow::SignalMonitor | Workflow::Trkr) {
            Axis::T
        } else {
            plan.fast
        };
        let id = egui::Id::new((
            plan.workflow.key(),
            spec_id,
            signal,
            axis.name(),
            mode == DisplayMode::RTheta,
            scale.to_bits(),
            epoch,
            true,
            log,
        ));
        let memory = egui_plot::PlotMemory::load(ctx, id).ok_or("Missing line plot state")?;
        let bounds = memory.bounds();
        let last_line = samples.last().unwrap().target.line;
        for sample in samples
            .iter()
            .filter(|s| !plan.workflow.is_2d() || s.target.line == last_line)
        {
            let x = if plan.workflow == Workflow::SignalMonitor {
                sample.elapsed
            } else {
                axis_value(sample, axis, plan)
            };
            let y = mode.values(sample.signal)[signal]
                * if mode == DisplayMode::RTheta && signal == 1 {
                    1.
                } else {
                    scale
                };
            let Some(y) = plotted_y(y, log) else {
                continue;
            };
            for (dimension, value) in [x, y].into_iter().enumerate() {
                if value < bounds.min()[dimension] - 1e-8 || value > bounds.max()[dimension] + 1e-8
                {
                    return Err(format!(
                        "Replay data outside line viewport: {value} in dimension {dimension}, {bounds:?}"
                    ));
                }
            }
        }
        if plan.workflow.is_2d() {
            let id = egui::Id::new((
                plan.workflow.key(),
                "map",
                signal,
                plan.fast.name(),
                plan.slow.name(),
                mode == DisplayMode::RTheta,
                scale.to_bits(),
                epoch,
                true,
            ));
            let memory = egui_plot::PlotMemory::load(ctx, id).ok_or("Missing map plot state")?;
            let bounds = memory.bounds();
            for sample in samples {
                for (dimension, value) in [sample.target.fast, sample.target.slow.unwrap()]
                    .into_iter()
                    .enumerate()
                {
                    if value < bounds.min()[dimension] - 1e-8
                        || value > bounds.max()[dimension] + 1e-8
                    {
                        return Err(format!(
                            "Replay data outside map viewport: {value} in dimension {dimension}, {bounds:?}"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Sample;
    use trkr_core::{Signal, Target};
    use trkr_motion::Position;
    #[test]
    fn logarithmic_display_keeps_only_positive_values_without_mutating_signals() {
        assert_eq!(plotted_y(1e-6, true), Some(-6.));
        assert_eq!(plotted_y(0., true), None);
        assert_eq!(plotted_y(-1., true), None);
        assert_eq!(plotted_y(-1., false), Some(-1.));
        assert_eq!(format_log_tick(-6.), "1e-6");
    }
    #[test]
    fn srkr_renders_both_retained_axes_with_each_runs_original_coordinates() {
        for (first, second, theta) in [(Axis::X, Axis::Y, 0.), (Axis::U, Axis::V, 45.)] {
            let ctx = egui::Context::default();
            let x = Plan {
                workflow: Workflow::Srkr,
                fast: first,
                theta_deg: theta,
                zero: [100., 200., 300.],
                fast_range: trkr_core::Range {
                    min: -40.,
                    max: 40.,
                    step: 40.,
                },
                ..Default::default()
            };
            let y = Plan {
                fast: second,
                zero: [400., 500., 600.],
                ..x.clone()
            };
            let samples = |plan: &Plan| {
                plan.targets()
                    .unwrap()
                    .into_iter()
                    .map(|target| Sample {
                        captured_at: None,
                        elapsed: 0.,
                        signal: Signal::new(1e-6, 2e-6, 3e-6, 75.).unwrap(),
                        position: Position {
                            physical: target.physical.map(Some),
                            ..Default::default()
                        },
                        target,
                    })
                    .collect::<Vec<_>>()
            };
            let mut history = crate::plot_history::History::default();
            history.select(
                Some(crate::plot_history::Trace {
                    plan: x.clone(),
                    samples: samples(&x),
                    progress: (3, 3),
                    output: None,
                }),
                &y,
            );
            let data = samples(&y);
            let mut cache = [None, None];
            let mut geometry = crate::precision::Geometry::new();
            for mode in [DisplayMode::Xy, DisplayMode::RTheta] {
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(900., 600.),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                plots(
                                    ui,
                                    &data,
                                    &y,
                                    mode,
                                    &mut cache,
                                    &mut geometry,
                                    PlotViewport {
                                        area: ui.max_rect(),
                                        epoch: 1,
                                        y_log: false,
                                        voltage_scale: 1e6,
                                        voltage_unit: "uV",
                                        history: &history,
                                    },
                                );
                            });
                        },
                    );
                }
                verify_visible_data(&ctx, &data, &y, mode, 1e6, 1, false).unwrap();
                let retained = history.srkr_axis(&y, first).unwrap();
                verify_visible_data(&ctx, &retained.samples, &retained.plan, mode, 1e6, 1, false)
                    .unwrap();
            }
        }
    }
    #[test]
    fn data_bounds_replace_empty_placeholder_and_mode_has_independent_units() {
        let ctx = egui::Context::default();
        let plan = Plan {
            workflow: Workflow::Trkr,
            ..Plan::default()
        };
        let samples: Vec<_> = [-50., 100., 250.]
            .into_iter()
            .map(|t| Sample {
                captured_at: None,
                elapsed: 0.,
                target: Target {
                    fast: t,
                    slow: None,
                    corrected: [t, 0., 0.],
                    physical: [t, 0., 0.],
                    line: 0,
                },
                signal: Signal {
                    x_v: 8e-6,
                    y_v: 2e-6,
                    r_v: 9e-6,
                    theta_deg: 75.,
                },
                position: Position::default(),
            })
            .collect();
        let mut bounds = None;
        for (data, mode) in [
            (&[][..], DisplayMode::Xy),
            (&samples[..], DisplayMode::Xy),
            (&samples[..], DisplayMode::RTheta),
        ] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(700., 400.))),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let view = PlotView {
                        epoch: 0,
                        y_log: false,
                        samples: data,
                        plan: &plan,
                        mode,
                        voltage_scale: 1e6,
                        voltage_unit: "uV",
                    };
                    let response = view.draw_line(
                        ui,
                        ui.max_rect(),
                        LineSpec {
                            signal: 1,
                            axis: Axis::T,
                            id: "regression",
                            current_line: false,
                            show_data: true,
                        },
                    );
                    bounds = Some(*response.transform.bounds());
                });
            });
            let b = bounds.unwrap();
            if data.is_empty() {
                assert_eq!(b.min()[0], 0.);
            } else {
                assert!(
                    b.min()[0] <= -50. && b.max()[0] >= 250.,
                    "data outside X bounds: {b:?}"
                );
                let expected = if mode == DisplayMode::RTheta { 75. } else { 2. };
                assert!(
                    b.min()[1] <= expected && b.max()[1] >= expected,
                    "signal outside Y bounds: {b:?}"
                );
            }
        }
    }
    #[test]
    fn python_plot_arrangements() {
        assert_eq!(plot_count(Workflow::Trkr), 2);
        assert_eq!(plot_count(Workflow::SignalMonitor), 2);
        for w in [Workflow::Srkr, Workflow::Strkr, Workflow::Srkr2d] {
            assert_eq!(plot_count(w), 4);
        }
    }
    #[test]
    fn readback_coordinates_precede_targets() {
        let plan = Plan {
            zero: [1., 2., 3.],
            theta_deg: 90.,
            ..Plan::default()
        };
        let sample = Sample {
            captured_at: None,
            elapsed: 0.,
            target: Target {
                fast: 999.,
                slow: None,
                corrected: [999.; 3],
                physical: [999.; 3],
                line: 0,
            },
            signal: Signal::default(),
            position: Position {
                physical: [Some(11.), Some(22.), Some(33.)],
                ..Position::default()
            },
        };
        assert_eq!(axis_value(&sample, Axis::T, &plan), 10.);
        assert_eq!(axis_value(&sample, Axis::X, &plan), 20.);
        assert!((axis_value(&sample, Axis::U, &plan) - 30.).abs() < 1e-10);
    }
    #[test]
    fn heatmap_is_signed_with_zero_center() {
        assert_eq!(diverging(0., 2.), Color32::from_rgb(246, 246, 245));
        assert_eq!(diverging(-1., 1.), Color32::from_rgb(33, 102, 172));
        assert_eq!(diverging(1., 1.), Color32::from_rgb(178, 24, 43));
        assert_ne!(diverging(-1., 1.), diverging(1., 1.));
    }
    #[test]
    fn heatmap_key_skips_unchanged_frames_and_invalidates_append_inputs() {
        let key = HeatmapKey {
            epoch: 7,
            workflow: Workflow::Strkr,
            mode: DisplayMode::Xy,
            fast: Axis::X,
            slow: Axis::T,
            signal: 0,
            voltage_scale: 1e6f64.to_bits(),
            sample_count: 12,
            last_sample: Some(SampleFingerprint {
                fast: 2f64.to_bits(),
                slow: Some(3f64.to_bits()),
                x: 4f64.to_bits(),
                y: 5f64.to_bits(),
                r: 6f64.to_bits(),
                theta: 7f64.to_bits(),
            }),
        };
        assert!(!heatmap_needs_rebuild(Some(key), key));
        for changed in [
            HeatmapKey { epoch: 8, ..key },
            HeatmapKey {
                mode: DisplayMode::RTheta,
                ..key
            },
            HeatmapKey {
                voltage_scale: 1e3f64.to_bits(),
                ..key
            },
            HeatmapKey {
                fast: Axis::U,
                ..key
            },
            HeatmapKey {
                sample_count: 13,
                ..key
            },
            HeatmapKey {
                last_sample: None,
                ..key
            },
        ] {
            assert!(heatmap_needs_rebuild(Some(key), changed));
        }
    }
    #[test]
    fn heatmap_preparation_preserves_sorted_grid_and_python_extent() {
        let sample = |fast, slow, x| Sample {
            captured_at: None,
            elapsed: 0.,
            target: Target {
                fast,
                slow: Some(slow),
                corrected: [0.; 3],
                physical: [0.; 3],
                line: 0,
            },
            signal: Signal {
                x_v: x,
                ..Signal::default()
            },
            position: Position::default(),
        };
        let prepared = prepare_heatmap(
            &[sample(2., 4., 1.), sample(0., 1., -1.), sample(2., 1., 0.)],
            DisplayMode::Xy,
            0,
            1.,
        )
        .expect("valid 2D target grid");
        assert_eq!(prepared.xs, vec![0., 2.]);
        assert_eq!(prepared.ys, vec![1., 4.]);
        assert_eq!(prepared.image.size, [2, 2]);
        assert_eq!(
            prepared.image.pixels,
            vec![
                Color32::TRANSPARENT,
                diverging(1., 1.),
                diverging(-1., 1.),
                diverging(0., 1.)
            ]
        );
        assert_eq!(prepared.extent.width, 2.);
        assert_eq!(prepared.extent.height, 3.);
        assert_eq!(prepared.extent.center, PlotPoint::new(1., 2.5));
    }
}
