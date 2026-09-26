//! Native GPUI editor for pitch, pair fit, and scale-wide exploration.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use toybox::gpui::{
    self as gpui, canvas, div, fill, point, prelude::*, px, rgba, Bounds, Context, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, Window,
};

use crate::analysis::{pair_fit, Analysis};
use crate::library;
use crate::scale::{nearest_lattice, Scale, MAX_DEGREES};
use crate::shared::{MasterStatus, Shared};
use crate::synth::{ATTACK_PARAM, LEVEL_PARAM, RELEASE_PARAM, WAVEFORM_PARAM};

/// Fixed editor width.
pub const WIDTH: u32 = 1536;
/// Fixed editor height.
pub const HEIGHT: u32 = 1024;

type GraphBounds = Rc<RefCell<Option<Bounds<Pixels>>>>;

struct Editor {
    shared: Arc<Shared>,
    pair: (usize, usize),
    dragging: Option<usize>,
    pair_drag: Option<(usize, f32, Scale)>,
    pitch_bounds: GraphBounds,
    matrix_bounds: GraphBounds,
    palette_bounds: GraphBounds,
    root_bounds: GraphBounds,
    period_bounds: GraphBounds,
    max_move_bounds: GraphBounds,
    level_bounds: GraphBounds,
    attack_bounds: GraphBounds,
    release_bounds: GraphBounds,
    control_drag: Option<usize>,
    slot: usize,
    message: String,
    variations: Vec<Scale>,
    max_move: f32,
    last_played_revision: u64,
    recent_played: Option<(u8, Instant)>,
    snap_period: bool,
    show_settings: bool,
    show_help: bool,
    analysis_cache: Option<(Scale, Analysis)>,
}

impl Editor {
    fn new(shared: Arc<Shared>) -> Self {
        let initial = shared.snapshot();
        Self {
            shared,
            pair: (0, 4),
            dragging: None,
            pair_drag: None,
            pitch_bounds: Rc::new(RefCell::new(None)),
            matrix_bounds: Rc::new(RefCell::new(None)),
            palette_bounds: Rc::new(RefCell::new(None)),
            root_bounds: Rc::new(RefCell::new(None)),
            period_bounds: Rc::new(RefCell::new(None)),
            max_move_bounds: Rc::new(RefCell::new(None)),
            level_bounds: Rc::new(RefCell::new(None)),
            attack_bounds: Rc::new(RefCell::new(None)),
            release_bounds: Rc::new(RefCell::new(None)),
            control_drag: None,
            slot: 1,
            message: String::new(),
            variations: variations(initial, 72.0),
            max_move: 72.0,
            last_played_revision: 0,
            recent_played: None,
            snap_period: false,
            show_settings: false,
            show_help: false,
            analysis_cache: None,
        }
    }

    #[cfg(feature = "screenshot-test")]
    fn screenshot(shared: Arc<Shared>) -> Self {
        Self::new(shared)
    }

    fn fewer(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(self.shared.snapshot().count.saturating_sub(1), cx);
        self.pair = (0, 1);
    }

    fn more(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count((self.shared.snapshot().count + 1).min(MAX_DEGREES), cx);
        self.pair = (0, 1);
    }

    fn equal(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared.edit(|scale| scale.set_equal(scale.count));
        cx.notify();
    }

    fn just(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared.edit(Scale::set_just_major);
        self.pair = (0, 4);
        cx.notify();
    }

    fn count_12(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(12, cx);
    }

    fn count_7(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(7, cx);
    }

    fn count_24(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(24, cx);
    }

    fn count_36(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(36, cx);
    }

    fn count_48(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_note_count(48, cx);
    }

    fn set_note_count(&mut self, count: usize, cx: &mut Context<Self>) {
        self.shared.edit(|scale| scale.set_count(count));
        self.variations = variations(self.shared.snapshot(), self.max_move);
        cx.notify();
    }

    fn triangle(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared.set_synth_param(WAVEFORM_PARAM, 0.0);
        cx.notify();
    }

    fn square(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared.set_synth_param(WAVEFORM_PARAM, 0.5);
        cx.notify();
    }

    fn saw(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared.set_synth_param(WAVEFORM_PARAM, 1.0);
        cx.notify();
    }

    fn root_previous(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .edit(|scale| scale.root_degree = (scale.root_degree + scale.count - 1) % scale.count);
        cx.notify();
    }

    fn root_next(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .edit(|scale| scale.root_degree = (scale.root_degree + 1) % scale.count);
        cx.notify();
    }

    fn key_previous(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .edit(|scale| scale.root_key = scale.root_key.saturating_sub(1));
        cx.notify();
    }

    fn key_next(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .edit(|scale| scale.root_key = scale.root_key.saturating_add(1).min(127));
        cx.notify();
    }

    fn previous_slot(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.slot = if self.slot == 1 {
            library::SLOT_COUNT
        } else {
            self.slot - 1
        };
        cx.notify();
    }

    fn next_slot(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.slot = self.slot % library::SLOT_COUNT + 1;
        cx.notify();
    }

    fn save(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.message = match library::save_slot(self.slot, self.shared.snapshot()) {
            Ok(()) => format!("Saved scale to slot {}", self.slot),
            Err(error) => error,
        };
        cx.notify();
    }

    fn load(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.message = match library::load_slot(self.slot) {
            Ok(scale) if self.shared.replace(scale) => {
                self.variations = variations(scale, self.max_move);
                format!("Loaded scale from slot {}", self.slot)
            }
            Ok(_) => "Saved scale is invalid".into(),
            Err(error) => error,
        };
        cx.notify();
    }

    fn regenerate(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.variations = variations(self.shared.snapshot(), self.max_move);
        self.message = "New variations from current scale".into();
        cx.notify();
    }

    fn toggle_snap_period(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.snap_period = !self.snap_period;
        cx.notify();
    }

    fn toggle_settings(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_settings = !self.show_settings;
        self.show_help = false;
        cx.notify();
    }

    fn toggle_help(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_help = !self.show_help;
        self.show_settings = false;
        cx.notify();
    }

    fn snap(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let index = self.pair.1;
        self.shared.edit(|scale| {
            if index > 0 && index < scale.count {
                let (point, _) = nearest_lattice(scale.cents[index]);
                scale.move_degree(index, point.cents);
            }
        });
        cx.notify();
    }

    fn graph_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let scale = self.shared.snapshot();
        let x = f32::from(event.position.x);
        let y = f32::from(event.position.y);
        if let Some(bounds) = *self.pitch_bounds.borrow() {
            if contains(bounds, x, y) {
                let index = (0..scale.count)
                    .min_by(|a, b| {
                        (x - pitch_x(bounds, scale.cents[*a] / scale.period_cents * 1200.0))
                            .abs()
                            .total_cmp(
                                &(x - pitch_x(
                                    bounds,
                                    scale.cents[*b] / scale.period_cents * 1200.0,
                                ))
                                .abs(),
                            )
                    })
                    .unwrap_or(0);
                if (x - pitch_x(bounds, scale.cents[index] / scale.period_cents * 1200.0)).abs()
                    < 20.0
                {
                    self.pair = (scale.root_degree, index);
                    if index > 0 {
                        self.dragging = Some(index);
                    }
                }
                cx.notify();
                return;
            }
        }
        if let Some(bounds) = *self.matrix_bounds.borrow() {
            if contains(bounds, x, y) {
                let (left, top, step_x, step_y, _) = matrix_area(bounds, scale.count);
                let column = ((x - left) / step_x).round() as isize;
                let row = ((y - top) / step_y).round() as isize;
                if row >= 0
                    && column >= 0
                    && row < scale.count as isize
                    && column < scale.count as isize
                {
                    self.pair = (row as usize, column as usize);
                    if row != column {
                        let movable = if column == 0 {
                            row as usize
                        } else {
                            column as usize
                        };
                        self.pair_drag = Some((movable, y, scale));
                    }
                }
                cx.notify();
                return;
            }
        }
        if let Some(bounds) = *self.palette_bounds.borrow() {
            if contains(bounds, x, y) {
                let frame = palette_frame(&self.variations);
                if let Some(candidate) = self.variations.iter().min_by(|a, b| {
                    palette_distance(bounds, x, y, **a, frame)
                        .total_cmp(&palette_distance(bounds, x, y, **b, frame))
                }) {
                    if palette_distance(bounds, x, y, *candidate, frame) < 32.0 {
                        self.shared.replace(*candidate);
                    }
                }
                cx.notify();
            }
        }
    }

    fn graph_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some((index, start_y, base)) = self.pair_drag {
            let other = if self.pair.0 == index {
                self.pair.1
            } else {
                self.pair.0
            };
            let start_fit = pair_fit((base.cents[index] - base.cents[other]).abs());
            let target =
                (start_fit + (start_y - f32::from(event.position.y)) / 190.0).clamp(0.0, 1.0);
            let mut best = base;
            let mut best_cost = f32::INFINITY;
            for step in -80..=80 {
                let mut candidate = base;
                candidate.move_degree(index, base.cents[index] + step as f32);
                let fit = pair_fit((candidate.cents[index] - candidate.cents[other]).abs());
                let cost = (fit - target).abs()
                    + (candidate.cents[index] - base.cents[index]).abs() * 0.0002;
                if cost < best_cost {
                    best_cost = cost;
                    best = candidate;
                }
            }
            self.shared.replace(best);
            cx.notify();
            return;
        }
        let Some(index) = self.dragging else { return };
        let Some(bounds) = *self.pitch_bounds.borrow() else {
            return;
        };
        let cents = ((f32::from(event.position.x) - f32::from(bounds.left()) - 45.0)
            / (f32::from(bounds.size.width) - 90.0))
            .clamp(0.0, 1.0)
            * self.shared.snapshot().period_cents;
        self.shared.edit(|scale| {
            scale.move_degree(index, cents);
        });
        cx.notify();
    }

    fn graph_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging = None;
        self.pair_drag = None;
        cx.notify();
    }

    fn root_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(0, event, cx);
    }

    fn period_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(1, event, cx);
    }

    fn max_move_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(2, event, cx);
    }

    fn level_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(3, event, cx);
    }

    fn attack_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(4, event, cx);
    }

    fn release_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_control(5, event, cx);
    }

    fn control_bounds(&self, index: usize) -> Option<Bounds<Pixels>> {
        match index {
            0 => *self.root_bounds.borrow(),
            1 => *self.period_bounds.borrow(),
            2 => *self.max_move_bounds.borrow(),
            3 => *self.level_bounds.borrow(),
            4 => *self.attack_bounds.borrow(),
            5 => *self.release_bounds.borrow(),
            _ => None,
        }
    }

    fn begin_control(&mut self, index: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.control_drag = Some(index);
        if let Some(bounds) = self.control_bounds(index) {
            self.apply_control(event.position.x.into(), event.modifiers.shift, bounds);
            cx.notify();
        }
    }

    fn controls_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button == Some(MouseButton::Left) {
            if let Some(index) = self.control_drag {
                if let Some(bounds) = self.control_bounds(index) {
                    self.apply_control(event.position.x.into(), event.modifiers.shift, bounds);
                    cx.notify();
                }
            }
        }
    }

    fn controls_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.control_drag = None;
    }

    fn apply_control(&mut self, x: f32, shift: bool, bounds: Bounds<Pixels>) {
        let t = ((x - f32::from(bounds.left()) - 8.0) / (f32::from(bounds.size.width) - 16.0))
            .clamp(0.0, 1.0);
        match self.control_drag {
            Some(0) => self
                .shared
                .edit(|scale| scale.root_hz = 55.0 * 2.0_f64.powf(f64::from(t) * 5.0)),
            Some(1) => {
                let cents = 600.0 + t * 1800.0;
                let snapped = if shift || self.snap_period {
                    [
                        1200.0 * (3.0_f32 / 2.0).log2(),
                        1200.0,
                        1200.0 * (5.0_f32 / 2.0).log2(),
                        1200.0 * 3.0_f32.log2(),
                        2400.0,
                    ]
                    .into_iter()
                    .min_by(|a, b| (a - cents).abs().total_cmp(&(b - cents).abs()))
                    .unwrap_or(cents)
                } else {
                    cents
                };
                self.shared.edit(|scale| scale.set_period(snapped));
            }
            Some(2) => {
                let next = 10.0 + t * 190.0;
                if (self.max_move - next).abs() >= 0.25 {
                    self.max_move = next;
                    self.variations = variations(self.shared.snapshot(), next);
                }
            }
            Some(3) => {
                self.shared.set_synth_param(LEVEL_PARAM, f64::from(t));
            }
            Some(4) => {
                self.shared.set_synth_param(ATTACK_PARAM, f64::from(t));
            }
            Some(5) => {
                self.shared.set_synth_param(RELEASE_PARAM, f64::from(t));
            }
            _ => {}
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        window.request_animation_frame();
        let scale = self.shared.snapshot();
        let (revision, last_key) = self.shared.last_played();
        if revision != self.last_played_revision {
            self.last_played_revision = revision;
            if let Ok(key) = u8::try_from(last_key) {
                self.recent_played = Some((key, Instant::now()));
            }
        }
        let held_degrees = played_degrees(scale, self.shared.held_keys());
        let recent_degree = self.recent_played.and_then(|(key, when)| {
            (when.elapsed() < Duration::from_millis(160)).then(|| degree_for_key(scale, key))
        });
        let settings = self.shared.synth_settings();
        let analysis = if let Some((cached_scale, cached)) = self.analysis_cache {
            if cached_scale == scale {
                cached
            } else {
                let next = Analysis::new(scale);
                self.analysis_cache = Some((scale, next));
                next
            }
        } else {
            let next = Analysis::new(scale);
            self.analysis_cache = Some((scale, next));
            next
        };
        let pair = (
            self.pair.0.min(scale.count - 1),
            self.pair.1.min(scale.count - 1),
        );
        let pitch_bounds = Rc::clone(&self.pitch_bounds);
        let matrix_bounds = Rc::clone(&self.matrix_bounds);
        let palette_bounds = Rc::clone(&self.palette_bounds);
        let root_bounds = Rc::clone(&self.root_bounds);
        let period_bounds = Rc::clone(&self.period_bounds);
        let max_move_bounds = Rc::clone(&self.max_move_bounds);
        let level_bounds = Rc::clone(&self.level_bounds);
        let attack_bounds = Rc::clone(&self.attack_bounds);
        let release_bounds = Rc::clone(&self.release_bounds);
        let candidates = self.variations.clone();
        let max_move = self.max_move;
        let matrix_graph = div()
            .id("pair-fit-graph")
            .w_full()
            .h(px(300.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::graph_down))
            .on_mouse_move(cx.listener(Self::graph_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::graph_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::graph_up))
            .child(
                canvas(
                    move |bounds, _, _| *matrix_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, cx| {
                        paint_matrix(bounds, scale, pair, analysis, window, cx)
                    },
                )
                .size_full(),
            );
        let palette_graph = div()
            .id("scale-space-graph")
            .w_full()
            .h(px(300.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::graph_down))
            .on_mouse_move(cx.listener(Self::graph_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::graph_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::graph_up))
            .child(
                canvas(
                    move |bounds, _, _| *palette_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| paint_palette(bounds, scale, &candidates, window),
                )
                .size_full(),
            );
        let pitch_graph = div()
            .id("pitch-line-graph")
            .w_full()
            .h(px(111.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::graph_down))
            .on_mouse_move(cx.listener(Self::graph_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::graph_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::graph_up))
            .child(
                canvas(
                    move |bounds, _, _| *pitch_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, cx| {
                        paint_pitch(bounds, scale, pair, held_degrees, recent_degree, window, cx)
                    },
                )
                .size_full(),
            );
        let root_slider = div()
            .id("root-frequency-slider")
            .w_full()
            .h(px(24.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::root_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *root_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(bounds, ((scale.root_hz / 55.0).log2() / 5.0) as f32, window)
                    },
                )
                .size_full(),
            );
        let period_slider = div()
            .id("octave-size-slider")
            .w_full()
            .h(px(24.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::period_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *period_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(bounds, (scale.period_cents - 600.0) / 1800.0, window)
                    },
                )
                .size_full(),
            );
        let max_move_slider = div()
            .id("variation-reach-slider")
            .w_full()
            .h(px(24.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::max_move_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *max_move_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(bounds, (max_move - 10.0) / 190.0, window)
                    },
                )
                .size_full(),
            );
        let level_slider = div()
            .id("synth-level-slider")
            .w_full()
            .h(px(20.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::level_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *level_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| paint_slider(bounds, settings.level, window),
                )
                .size_full(),
            );
        let attack_slider = div()
            .id("synth-attack-slider")
            .w_full()
            .h(px(20.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::attack_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *attack_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(bounds, (settings.attack_ms - 2.0) / 498.0, window)
                    },
                )
                .size_full(),
            );
        let release_slider = div()
            .id("synth-release-slider")
            .w_full()
            .h(px(20.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::release_down))
            .on_mouse_move(cx.listener(Self::controls_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::controls_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::controls_up))
            .child(
                canvas(
                    move |bounds, _, _| *release_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(bounds, (settings.release_ms - 100.0) / 2900.0, window)
                    },
                )
                .size_full(),
            );
        let (status, status_color) = match self.shared.status() {
            MasterStatus::Connecting => ("connecting".to_owned(), 0x777a78ff),
            MasterStatus::Active => (
                format!("active  ·  {} clients", self.shared.clients()),
                0xff6057ff,
            ),
            MasterStatus::Blocked => ("another master active".to_owned(), 0xc18c67ff),
            MasterStatus::Unavailable => ("library unavailable".to_owned(), 0x777a78ff),
        };
        let shapes = analysis.root_shapes(scale);
        let root_shape = shapes[0];
        let selected_fit = analysis.pair_fit[pair.0][pair.1];
        let settings_popover = if self.show_settings {
            panel()
                .id("settings-popover")
                .absolute()
                .top(px(68.0))
                .right(px(62.0))
                .w(px(245.0))
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(9.0))
                .child(div().text_size(px(12.0)).child("SCALE ACTIONS"))
                .child(
                    control_button("Equal divisions", false)
                        .id("equal")
                        .on_click(cx.listener(Self::equal)),
                )
                .child(
                    control_button("Just major", false)
                        .id("just")
                        .on_click(cx.listener(Self::just)),
                )
                .child(
                    control_button("Snap selected note", false)
                        .id("snap")
                        .on_click(cx.listener(Self::snap)),
                )
        } else {
            div().id("settings-popover").hidden()
        };
        let help_popover = if self.show_help {
            panel()
                .id("help-popover")
                .absolute()
                .top(px(68.0))
                .right(px(22.0))
                .w(px(330.0))
                .p(px(16.0))
                .flex()
                .flex_col()
                .gap(px(9.0))
                .text_size(px(12.0))
                .child("DRAG NOTES ON THE PITCH LINE TO RETUNE")
                .child("PLAY MIDI TO HEAR AND HIGHLIGHT NOTES")
                .child("DRAG LATTICE DOTS TO CHANGE PAIR FIT")
                .child("CHOOSE A LANDSCAPE POINT TO RETUNE THE SCALE")
                .child("SHIFT-DRAG THE REPEAT SLIDER TO SNAP RATIOS")
        } else {
            div().id("help-popover").hidden()
        };
        div()
            .relative()
            .w_full()
            .h_full()
            .p(px(22.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .bg(rgba(0x0b0e0eff))
            .text_color(rgba(0xe5e5e1ff))
            .child(
                div()
                    .h(px(42.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(42.0))
                            .text_color(rgba(0xff6057ff))
                            .child("freqs"),
                    )
                    .child(
                        div()
                            .ml(px(22.0))
                            .mt(px(10.0))
                            .text_size(px(11.0))
                            .text_color(rgba(0xa4a6a4ff))
                            .child("MICROTONAL EXPLORER"),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .mr(px(62.0))
                            .flex()
                            .gap(px(54.0))
                            .items_center()
                            .text_size(px(13.0))
                            .child(div().text_color(rgba(0x939694ff)).child("EXPLORE"))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.0))
                                    .child("TUNE")
                                    .child(div().h(px(4.0)).bg(rgba(0xff6057ff))),
                            ),
                    )
                    .child(div().h(px(38.0)).w(px(1.0)).bg(rgba(0x303535ff)))
                    .child(
                        div()
                            .mx(px(30.0))
                            .flex()
                            .gap(px(12.0))
                            .items_center()
                            .child(
                                div()
                                    .w(px(14.0))
                                    .h(px(14.0))
                                    .rounded(px(7.0))
                                    .bg(rgba(status_color)),
                            )
                            .child(div().text_size(px(13.0)).child("MTS-ESP"))
                            .child(
                                div()
                                    .ml(px(8.0))
                                    .text_size(px(11.0))
                                    .text_color(rgba(0xa4a6a4ff))
                                    .child(status),
                            ),
                    )
                    .child(div().h(px(38.0)).w(px(1.0)).bg(rgba(0x303535ff)))
                    .child(
                        control_button("⚙", self.show_settings)
                            .id("settings")
                            .ml(px(24.0))
                            .on_click(cx.listener(Self::toggle_settings)),
                    )
                    .child(
                        control_button("?", self.show_help)
                            .id("help")
                            .ml(px(8.0))
                            .on_click(cx.listener(Self::toggle_help)),
                    ),
            )
            .child(
                div()
                    .h(px(392.0))
                    .flex()
                    .gap(px(16.0))
                    .child(
                        panel()
                            .w(px(650.0))
                            .h_full()
                            .p(px(20.0))
                            .flex()
                            .flex_col()
                            .child(section_title("NOTE LATTICE"))
                            .child(div().mt(px(6.0)).h(px(2.0)).bg(rgba(0x303535ff)))
                            .child(div().h(px(10.0)))
                            .child(matrix_graph),
                    )
                    .child(
                        panel()
                            .w(px(826.0))
                            .h_full()
                            .p(px(20.0))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(section_title("SCALE LANDSCAPE"))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .flex()
                                            .items_center()
                                            .gap(px(14.0))
                                            .text_size(px(12.0))
                                            .child("REACH")
                                            .child(format!("±{max_move:.0}¢"))
                                            .child(div().w(px(190.0)).child(max_move_slider)),
                                    ),
                            )
                            .child(div().h(px(8.0)))
                            .child(palette_graph)
                            .child(
                                div().mt(px(2.0)).flex().items_center().child(
                                    control_button("↻  NEW VARIATIONS", false)
                                        .id("regenerate")
                                        .ml_auto()
                                        .h(px(28.0))
                                        .on_click(cx.listener(Self::regenerate)),
                                ),
                            ),
                    ),
            )
            .child(
                panel()
                    .h(px(172.0))
                    .p(px(20.0))
                    .flex()
                    .flex_col()
                    .child(section_title("PITCH LINE"))
                    .child(pitch_graph),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .h(px(261.0))
                    .child(
                        div()
                            .w(px(1176.0))
                            .flex()
                            .flex_col()
                            .gap(px(14.0))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(14.0))
                                    .h(px(185.0))
                                    .child(
                                        panel()
                                            .w(px(616.0))
                                            .h_full()
                                            .p(px(20.0))
                                            .flex()
                                            .flex_col()
                                            .child(section_title("TUNING"))
                                            .child(
                                                div()
                                                    .mt(px(9.0))
                                                    .flex()
                                                    .gap(px(14.0))
                                                    .text_size(px(11.0))
                                                    .child(
                                                        div()
                                                            .w(px(172.0))
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(5.0))
                                                            .child("ROOT DEGREE")
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .gap(px(5.0))
                                                                    .items_center()
                                                                    .child(
                                                                        control_button("‹", false)
                                                                            .id("root-previous")
                                                                            .on_click(cx.listener(
                                                                                Self::root_previous,
                                                                            )),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .w(px(66.0))
                                                                            .text_size(px(15.0))
                                                                            .child(format!(
                                                                                "{}",
                                                                                scale.root_degree
                                                                                    + 1
                                                                            )),
                                                                    )
                                                                    .child(
                                                                        control_button("›", false)
                                                                            .id("root-next")
                                                                            .on_click(cx.listener(
                                                                                Self::root_next,
                                                                            )),
                                                                    ),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(186.0))
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(5.0))
                                                            .child(format!(
                                                                "ROOT PITCH   {:.2} Hz",
                                                                scale.root_hz
                                                            ))
                                                            .child(root_slider),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(172.0))
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(5.0))
                                                            .child("MIDI KEY")
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .gap(px(5.0))
                                                                    .items_center()
                                                                    .child(
                                                                        control_button("‹", false)
                                                                            .id("key-previous")
                                                                            .on_click(cx.listener(
                                                                                Self::key_previous,
                                                                            )),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .w(px(52.0))
                                                                            .text_size(px(15.0))
                                                                            .child(format!(
                                                                                "{}",
                                                                                scale.root_key
                                                                            )),
                                                                    )
                                                                    .child(
                                                                        control_button("›", false)
                                                                            .id("key-next")
                                                                            .on_click(cx.listener(
                                                                                Self::key_next,
                                                                            )),
                                                                    ),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(12.0))
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(11.0))
                                                    .child(format!(
                                                        "OCTAVE / REPEAT   {:.1}¢  ({:.3}:1)",
                                                        scale.period_cents,
                                                        2.0_f64.powf(
                                                            f64::from(scale.period_cents) / 1200.0
                                                        )
                                                    ))
                                                    .child(
                                                        div()
                                                            .ml_auto()
                                                            .text_color(rgba(0x999d9aff))
                                                            .child("SNAP RATIOS"),
                                                    )
                                                    .child(
                                                        control_button(
                                                            if self.snap_period {
                                                                "ON"
                                                            } else {
                                                                "OFF"
                                                            },
                                                            self.snap_period,
                                                        )
                                                        .id("shift-snaps")
                                                        .ml(px(8.0))
                                                        .h(px(25.0))
                                                        .on_click(
                                                            cx.listener(Self::toggle_snap_period),
                                                        ),
                                                    ),
                                            )
                                            .child(period_slider),
                                    )
                                    .child(
                                        panel()
                                            .w(px(546.0))
                                            .h_full()
                                            .p(px(20.0))
                                            .flex()
                                            .flex_col()
                                            .child(section_title("SCALE"))
                                            .child(
                                                div()
                                                    .mt(px(9.0))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .text_size(px(11.0))
                                                    .child(
                                                        div().mr(px(7.0)).child("NOTES / REPEAT"),
                                                    )
                                                    .child(
                                                        control_button("−", false)
                                                            .id("fewer")
                                                            .on_click(cx.listener(Self::fewer)),
                                                    )
                                                    .child(
                                                        control_button("+", false)
                                                            .id("more")
                                                            .on_click(cx.listener(Self::more)),
                                                    )
                                                    .child(
                                                        control_button("7", scale.count == 7)
                                                            .id("count-7")
                                                            .on_click(cx.listener(Self::count_7)),
                                                    )
                                                    .child(
                                                        control_button("12", scale.count == 12)
                                                            .id("count-12")
                                                            .on_click(cx.listener(Self::count_12)),
                                                    )
                                                    .child(
                                                        control_button("24", scale.count == 24)
                                                            .id("count-24")
                                                            .on_click(cx.listener(Self::count_24)),
                                                    )
                                                    .child(
                                                        control_button("36", scale.count == 36)
                                                            .id("count-36")
                                                            .on_click(cx.listener(Self::count_36)),
                                                    )
                                                    .child(
                                                        control_button("48", scale.count == 48)
                                                            .id("count-48")
                                                            .on_click(cx.listener(Self::count_48)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(13.0))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .text_size(px(12.0))
                                                    .child(
                                                        control_button("‹", false)
                                                            .id("previous-slot")
                                                            .on_click(
                                                                cx.listener(Self::previous_slot),
                                                            ),
                                                    )
                                                    .child(
                                                        control_button("›", false)
                                                            .id("next-slot")
                                                            .on_click(cx.listener(Self::next_slot)),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(183.0))
                                                            .px(px(10.0))
                                                            .py(px(8.0))
                                                            .border_1()
                                                            .border_color(rgba(0x303636ff))
                                                            .child(format!(
                                                                "SLOT {}   ·   {}",
                                                                self.slot,
                                                                if library::slot_exists(self.slot) {
                                                                    "SAVED"
                                                                } else {
                                                                    "EMPTY"
                                                                }
                                                            )),
                                                    )
                                                    .child(
                                                        control_button("↓ SAVE", false)
                                                            .id("save-slot")
                                                            .on_click(cx.listener(Self::save)),
                                                    )
                                                    .child(
                                                        control_button("▣ LOAD", false)
                                                            .id("load-slot")
                                                            .on_click(cx.listener(Self::load)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .mt(px(6.0))
                                                    .text_size(px(10.0))
                                                    .text_color(rgba(0x999d9aff))
                                                    .child(self.message.clone()),
                                            ),
                                    ),
                            )
                            .child(
                                panel()
                                    .h(px(62.0))
                                    .px(px(20.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(section_title("SYNTH"))
                                    .child(
                                        control_button("△", settings.waveform.index() == 0)
                                            .id("triangle-wave")
                                            .ml(px(15.0))
                                            .on_click(cx.listener(Self::triangle)),
                                    )
                                    .child(
                                        control_button("◩", settings.waveform.index() == 1)
                                            .id("square-wave")
                                            .on_click(cx.listener(Self::square)),
                                    )
                                    .child(
                                        control_button("╱", settings.waveform.index() == 2)
                                            .id("saw-wave")
                                            .on_click(cx.listener(Self::saw)),
                                    )
                                    .child(
                                        div()
                                            .ml(px(12.0))
                                            .h(px(28.0))
                                            .w(px(1.0))
                                            .bg(rgba(0x343939ff)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .child(format!("LVL  {:.0}%", settings.level * 100.0)),
                                    )
                                    .child(div().w(px(130.0)).child(level_slider))
                                    .child(div().h(px(28.0)).w(px(1.0)).bg(rgba(0x343939ff)))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .child(format!("A  {:.0} ms", settings.attack_ms)),
                                    )
                                    .child(div().w(px(130.0)).child(attack_slider))
                                    .child(div().h(px(28.0)).w(px(1.0)).bg(rgba(0x343939ff)))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .child(format!("R  {:.0} ms", settings.release_ms)),
                                    )
                                    .child(div().w(px(155.0)).child(release_slider)),
                            ),
                    )
                    .child(
                        panel()
                            .w(px(300.0))
                            .h(px(212.0))
                            .p(px(18.0))
                            .flex()
                            .flex_col()
                            .gap(px(10.0))
                            .child(section_title("SCALE STATS"))
                            .child(stat_row(
                                "COHESION",
                                format!("{:.0}%", analysis.coordinates.cohesion * 100.0),
                            ))
                            .child(stat_row(
                                "ROOT GRAVITY",
                                format!("{:.0}%", analysis.coordinates.root_gravity * 100.0),
                            ))
                            .child(stat_row(
                                "ENERGY",
                                format!("{:.0}%", analysis.coordinates.energy * 100.0),
                            ))
                            .child(stat_row(
                                "PAIR FIT",
                                format!("{:.0}%", selected_fit * 100.0),
                            ))
                            .child(stat_row(
                                "ROOT SHAPE",
                                format!(
                                    "{}–{}–{}  ({:.0}%)",
                                    scale.root_degree + 1,
                                    root_shape.0 + 1,
                                    root_shape.1 + 1,
                                    root_shape.2 * 100.0
                                ),
                            )),
                    ),
            )
            .child(
                div()
                    .mt_auto()
                    .ml_auto()
                    .text_size(px(10.0))
                    .text_color(rgba(0x727775ff))
                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
            )
            .child(settings_popover)
            .child(help_popover)
    }
}

fn contains(bounds: Bounds<Pixels>, x: f32, y: f32) -> bool {
    x >= f32::from(bounds.left())
        && x < f32::from(bounds.right())
        && y >= f32::from(bounds.top())
        && y < f32::from(bounds.bottom())
}

fn panel() -> gpui::Div {
    div()
        .bg(rgba(0x101313ff))
        .border_1()
        .border_color(rgba(0x2b3030ff))
}

fn section_title(title: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .child(div().w(px(6.0)).h(px(28.0)).bg(rgba(0xd2443bff)))
        .child(
            div()
                .h(px(28.0))
                .px(px(12.0))
                .bg(rgba(0xff6057ff))
                .text_color(rgba(0x111010ff))
                .text_size(px(19.0))
                .child(title),
        )
}

fn control_button(label: impl Into<gpui::SharedString>, active: bool) -> gpui::Div {
    div()
        .min_w(px(36.0))
        .h(px(34.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(if active { 0x2b1817ff } else { 0x151919ff }))
        .border_1()
        .border_color(rgba(if active { 0xed5c51ff } else { 0x303636ff }))
        .text_color(rgba(if active { 0xff756bff } else { 0xd1d1ceff }))
        .text_size(px(13.0))
        .child(label.into())
}

fn stat_row(label: &'static str, value: String) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .text_size(px(12.0))
        .child(div().text_color(rgba(0xc7c8c5ff)).child(label))
        .child(div().ml_auto().text_color(rgba(0xf1f0ebff)).child(value))
}

fn rect(window: &mut Window, left: f32, top: f32, right: f32, bottom: f32, color: u32) {
    window.paint_quad(fill(
        Bounds::from_corners(point(px(left), px(top)), point(px(right), px(bottom))),
        rgba(color),
    ));
}

fn circle(window: &mut Window, x: f32, y: f32, radius: f32, color: u32) {
    window.paint_quad(
        fill(
            Bounds::from_corners(
                point(px(x - radius), px(y - radius)),
                point(px(x + radius), px(y + radius)),
            ),
            rgba(color),
        )
        .corner_radii(px(radius)),
    );
}

fn score_color(score: f32) -> u32 {
    let t = score.clamp(0.0, 1.0).powf(3.0);
    let blend = |low: f32, high: f32| (low + (high - low) * t).round() as u32;
    (blend(45.0, 255.0) << 24) | (blend(51.0, 99.0) << 16) | (blend(51.0, 88.0) << 8) | 0xff
}

fn canvas_label(
    window: &mut Window,
    cx: &mut gpui::App,
    label: String,
    x: f32,
    y: f32,
    size: f32,
    color: u32,
) {
    let run = gpui::TextRun {
        len: label.len(),
        font: window.text_style().font(),
        color: rgba(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(label.into(), px(size), &[run], None);
    let _ = shaped.paint(
        point(px(x), px(y)),
        px(size),
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

fn line(window: &mut Window, start: (f32, f32), end: (f32, f32), color: u32, width: f32) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(px(start.0), px(start.1)));
    path.line_to(point(px(end.0), px(end.1)));
    if let Ok(path) = path.build() {
        window.paint_path(path, rgba(color));
    }
}

fn pitch_x(bounds: Bounds<Pixels>, cents: f32) -> f32 {
    f32::from(bounds.left()) + 45.0 + cents / 1200.0 * (f32::from(bounds.size.width) - 90.0)
}

fn degree_for_key(scale: Scale, key: u8) -> usize {
    (scale.root_degree as i32 + i32::from(key) - i32::from(scale.root_key))
        .rem_euclid(scale.count as i32) as usize
}

fn played_degrees(scale: Scale, keys: [u64; 2]) -> [bool; MAX_DEGREES] {
    let mut degrees = [false; MAX_DEGREES];
    for key in 0..128_usize {
        if keys[key / 64] & (1_u64 << (key % 64)) != 0 {
            degrees[degree_for_key(scale, key as u8)] = true;
        }
    }
    degrees
}

fn paint_pitch(
    bounds: Bounds<Pixels>,
    scale: Scale,
    pair: (usize, usize),
    held_degrees: [bool; MAX_DEGREES],
    recent_degree: Option<usize>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let right = f32::from(bounds.right());
    let bottom = f32::from(bounds.bottom());
    rect(window, left, top, right, bottom, 0x101414ff);
    for row in 0..4 {
        let y = top + 10.0 + row as f32 * 24.0;
        let mut x = left + 8.0;
        while x < right - 8.0 {
            circle(window, x, y, 0.65, 0x2a3030ff);
            x += 24.0;
        }
    }
    let mid = top + 51.0;
    line(
        window,
        (left + 45.0, mid),
        (right - 45.0, mid),
        0xbfc3c0ff,
        1.4,
    );
    for (index, held) in held_degrees.iter().copied().enumerate().take(scale.count) {
        let x = pitch_x(bounds, scale.cents[index] / scale.period_cents * 1200.0);
        let selected = index == pair.1 && index != scale.root_degree;
        let recent = recent_degree == Some(index);
        if held || recent {
            circle(window, x, mid, if held { 16.0 } else { 13.0 }, 0x6f4139ff);
        }
        if selected {
            circle(window, x, mid, 14.0, 0xff6057ff);
            circle(window, x, mid, 11.0, 0x151818ff);
        }
        let is_root = index == scale.root_degree;
        circle(
            window,
            x,
            mid,
            if is_root || held { 10.0 } else { 8.0 },
            if held {
                0xffc19aff
            } else if recent {
                0xe79573ff
            } else if is_root {
                0xff6057ff
            } else {
                0xf0f0edff
            },
        );
        if scale.count <= 12 {
            let label = if index == 0 {
                "0.0¢".to_owned()
            } else {
                format!("+{:.1}¢", scale.cents[index])
            };
            canvas_label(window, cx, label, x - 22.0, bottom - 23.0, 10.0, 0xc9cbc8ff);
        }
    }
    let end = pitch_x(bounds, 1200.0);
    circle(window, end, mid, 10.0, 0xff6057ff);
    if scale.count <= 12 {
        canvas_label(
            window,
            cx,
            format!("+{:.1}¢", scale.period_cents),
            end - 45.0,
            bottom - 23.0,
            10.0,
            0xc9cbc8ff,
        );
    }
}

fn matrix_area(bounds: Bounds<Pixels>, count: usize) -> (f32, f32, f32, f32, f32) {
    let step_x = (f32::from(bounds.size.width) - 142.0) / (count - 1) as f32;
    let step_y = (f32::from(bounds.size.height) - 69.0) / (count - 1) as f32;
    (
        f32::from(bounds.left()) + 94.0,
        f32::from(bounds.top()) + 42.0,
        step_x,
        step_y,
        (step_x.min(step_y) * 0.31).clamp(2.0, 11.0),
    )
}

fn paint_matrix(
    bounds: Bounds<Pixels>,
    scale: Scale,
    pair: (usize, usize),
    analysis: Analysis,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    rect(
        window,
        f32::from(bounds.left()),
        f32::from(bounds.top()),
        f32::from(bounds.right()),
        f32::from(bounds.bottom()),
        0x111515ff,
    );
    let (left, top, step_x, step_y, radius) = matrix_area(bounds, scale.count);
    for column in 0..scale.count {
        let x = left + column as f32 * step_x;
        if scale.count <= 16 || column % (scale.count / 12).max(1) == 0 {
            canvas_label(
                window,
                cx,
                column.to_string(),
                x - 4.0,
                top - 31.0,
                10.0,
                0xc4c7c3ff,
            );
        }
    }
    for row in 0..scale.count {
        let y = top + row as f32 * step_y;
        if scale.count <= 16 || row % (scale.count / 12).max(1) == 0 {
            canvas_label(
                window,
                cx,
                (row + 1).to_string(),
                left - 37.0,
                y - 7.0,
                10.0,
                0xc4c7c3ff,
            );
        }
        for column in 0..scale.count {
            let fit = analysis.pair_fit[row][column];
            let color = if row == column {
                0x343939ff
            } else {
                score_color(fit)
            };
            let x = left + column as f32 * step_x;
            if (row, column) == pair || (column, row) == pair {
                circle(window, x, y, radius + 4.0, 0xff655bff);
                circle(window, x, y, radius + 1.5, 0x101414ff);
            }
            circle(window, x, y, radius, color);
        }
    }
}

fn paint_slider(bounds: Bounds<Pixels>, value: f32, window: &mut Window) {
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let right = f32::from(bounds.right());
    let bottom = f32::from(bounds.bottom());
    let middle = (top + bottom) * 0.5;
    let start = left + 2.0;
    let end = right - 2.0;
    let handle = start + value.clamp(0.0, 1.0) * (end - start);
    rect(window, left, top, right, bottom, 0x151919ff);
    rect(window, start, middle - 3.0, end, middle + 3.0, 0x343939ff);
    rect(
        window,
        start,
        middle - 3.0,
        handle,
        middle + 3.0,
        0xff6258ff,
    );
    rect(
        window,
        handle - 2.0,
        middle - 8.0,
        handle + 2.0,
        middle + 8.0,
        0xff8679ff,
    );
}

fn quick_scores(scale: Scale) -> (f32, f32) {
    let mut total = 0.0;
    let mut root_total = 0.0;
    for a in 0..scale.count {
        for b in a + 1..scale.count {
            let fit = pair_fit((scale.cents[a] - scale.cents[b]).abs());
            total += fit;
            if a == scale.root_degree || b == scale.root_degree {
                root_total += fit;
            }
        }
    }
    (
        total / (scale.count * (scale.count - 1) / 2) as f32,
        root_total / (scale.count - 1) as f32,
    )
}

fn variations(base: Scale, max_move: f32) -> Vec<Scale> {
    let mut choices = vec![base];
    for direction in [-1.0_f32, 1.0] {
        for amount in [0.2, 0.4, 0.7, 1.0].map(|fraction| fraction * max_move) {
            let mut cohesion = base;
            let mut gravity = base;
            for index in 1..base.count {
                let mut best_cohesion = (f32::NEG_INFINITY, cohesion.cents[index]);
                let mut best_gravity = (f32::NEG_INFINITY, gravity.cents[index]);
                for offset in -8..=8 {
                    let move_cents = direction * amount * offset as f32 / 8.0;
                    if move_cents * direction < 0.0 {
                        continue;
                    }
                    let mut trial = base;
                    trial.move_degree(index, base.cents[index] + move_cents);
                    let (cohesion_score, gravity_score) = quick_scores(trial);
                    let score_c = cohesion_score * direction;
                    let score_g = gravity_score * direction;
                    if score_c > best_cohesion.0 {
                        best_cohesion = (score_c, trial.cents[index]);
                    }
                    if score_g > best_gravity.0 {
                        best_gravity = (score_g, trial.cents[index]);
                    }
                }
                cohesion.move_degree(index, best_cohesion.1);
                gravity.move_degree(index, best_gravity.1);
            }
            if !choices.contains(&cohesion) {
                choices.push(cohesion);
            }
            if !choices.contains(&gravity) {
                choices.push(gravity);
            }
        }
    }
    let anchors = choices.clone();
    for anchor in anchors.iter().skip(1) {
        for amount in [0.25_f32, 0.5, 0.75] {
            let mut between = base;
            for index in 1..base.count {
                between.move_degree(
                    index,
                    base.cents[index] + (anchor.cents[index] - base.cents[index]) * amount,
                );
            }
            if !choices.contains(&between) {
                choices.push(between);
            }
        }
    }
    choices
}

fn palette_frame(choices: &[Scale]) -> (f32, f32, f32, f32) {
    let mut min_x = 1.0_f32;
    let mut max_x = 0.0_f32;
    let mut min_y = 1.0_f32;
    let mut max_y = 0.0_f32;
    for choice in choices {
        let (cohesion, root_gravity) = quick_scores(*choice);
        min_x = min_x.min(cohesion);
        max_x = max_x.max(cohesion);
        min_y = min_y.min(root_gravity);
        max_y = max_y.max(root_gravity);
    }
    let pad_x = ((max_x - min_x) * 0.18).max(0.015);
    let pad_y = ((max_y - min_y) * 0.18).max(0.015);
    (min_x - pad_x, max_x + pad_x, min_y - pad_y, max_y + pad_y)
}

fn palette_xy(bounds: Bounds<Pixels>, scale: Scale, frame: (f32, f32, f32, f32)) -> (f32, f32) {
    let (cohesion, root_gravity) = quick_scores(scale);
    let (min_x, max_x, min_y, max_y) = frame;
    let left = f32::from(bounds.left()) + 40.0;
    let top = f32::from(bounds.top()) + 25.0;
    let width = f32::from(bounds.size.width) - 80.0;
    let height = f32::from(bounds.size.height) - 60.0;
    (
        left + ((cohesion - min_x) / (max_x - min_x)).clamp(0.0, 1.0) * width,
        top + (1.0 - (root_gravity - min_y) / (max_y - min_y)).clamp(0.0, 1.0) * height,
    )
}

fn palette_distance(
    bounds: Bounds<Pixels>,
    x: f32,
    y: f32,
    scale: Scale,
    frame: (f32, f32, f32, f32),
) -> f32 {
    let (cx, cy) = palette_xy(bounds, scale, frame);
    (x - cx).hypot(y - cy)
}

fn paint_palette(bounds: Bounds<Pixels>, current: Scale, choices: &[Scale], window: &mut Window) {
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let right = f32::from(bounds.right());
    let bottom = f32::from(bounds.bottom());
    rect(window, left, top, right, bottom, 0x111515ff);
    let mut y = top + 12.0;
    while y < bottom - 8.0 {
        let mut x = left + 12.0;
        while x < right - 8.0 {
            circle(window, x, y, 0.55, 0x283030ff);
            x += 18.0;
        }
        y += 18.0;
    }
    let frame = palette_frame(choices);
    let points: Vec<_> = choices
        .iter()
        .map(|choice| palette_xy(bounds, *choice, frame))
        .collect();
    if !points.is_empty() {
        // A Gaussian-smoothed path through the variations gives the contours
        // the same shape as the scale landscape without connecting individual dots.
        let bandwidth = (right - left) * 0.17;
        let mut ridge = Vec::with_capacity(97);
        for step in 0..=96 {
            let x = left + 8.0 + (right - left - 16.0) * step as f32 / 96.0;
            let (weighted_y, weight_sum) = points.iter().fold((0.0, 0.0), |(sum, total), p| {
                let distance = (x - p.0) / bandwidth;
                let weight = (-0.5 * distance * distance).exp();
                (sum + p.1 * weight, total + weight)
            });
            ridge.push((x, weighted_y / weight_sum));
        }
        for band in -8..=8 {
            let mut path = PathBuilder::stroke(px(1.0));
            let mut drawing = false;
            let mut has_segment = false;
            for (step, (x, center_y)) in ridge.iter().enumerate() {
                let phase = step as f32 / 96.0 * std::f32::consts::TAU * 1.3;
                let spacing = 14.0 + 1.8 * (phase + band as f32 * 0.17).sin();
                let y = center_y + band as f32 * spacing;
                if (top + 5.0..bottom - 5.0).contains(&y) {
                    if drawing {
                        path.line_to(point(px(*x), px(y)));
                        has_segment = true;
                    } else {
                        path.move_to(point(px(*x), px(y)));
                        drawing = true;
                    }
                } else {
                    drawing = false;
                }
            }
            if has_segment {
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgba(0x68777132));
                }
            }
        }
    }
    for (choice, (x, y)) in choices.iter().zip(points) {
        let selected = *choice == current;
        let (cohesion, _) = quick_scores(*choice);
        let color = if cohesion > 0.55 {
            0xff6258ff
        } else if cohesion > 0.4 {
            0xc67d71ff
        } else {
            0x9ea7a5ff
        };
        if selected {
            circle(window, x, y, 12.0, 0xff766bff);
            circle(window, x, y, 9.0, 0x111515ff);
        }
        circle(window, x, y, if selected { 5.5 } else { 3.5 }, color);
    }
}

/// Construct the hostable GPUI VST3 editor.
#[cfg(feature = "vst3")]
pub(crate) fn new_vst3_gui(shared: Arc<Shared>) -> toybox::gpui_gui::GpuiHostedGui {
    toybox::gpui_gui::GpuiHostedGui::new(
        "FreqsGpuiEditorView",
        move |_, cx| cx.new(|_| Editor::new(Arc::clone(&shared))).into(),
        WIDTH,
        HEIGHT,
    )
    .with_size_contract((WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, HEIGHT))
}

/// Construct a deterministic editor fixture for screenshot tests.
#[cfg(feature = "screenshot-test")]
pub fn new_screenshot_gui() -> toybox::gpui_gui::GpuiHostedGui {
    toybox::gpui_gui::GpuiHostedGui::new(
        "FreqsGpuiScreenshotView",
        move |_, cx| {
            cx.new(|_| Editor::screenshot(Arc::new(Shared::default())))
                .into()
        },
        WIDTH,
        HEIGHT,
    )
    .with_size_contract((WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, HEIGHT))
}

/// Construct a screenshot fixture with MIDI key 60 held.
#[cfg(feature = "screenshot-test")]
pub fn new_playing_note_screenshot_gui() -> toybox::gpui_gui::GpuiHostedGui {
    let shared = Arc::new(Shared::default());
    shared.set_held_keys([1_u64 << 60, 0]);
    shared.note_played(60);
    toybox::gpui_gui::GpuiHostedGui::new(
        "FreqsGpuiPlayingNoteView",
        move |_, cx| cx.new(|_| Editor::screenshot(Arc::clone(&shared))).into(),
        WIDTH,
        HEIGHT,
    )
    .with_size_contract((WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, HEIGHT))
}

/// Construct a screenshot fixture with the maximum 48-note scale.
#[cfg(feature = "screenshot-test")]
pub fn new_dense_screenshot_gui() -> toybox::gpui_gui::GpuiHostedGui {
    let shared = Arc::new(Shared::default());
    shared.edit(|scale| scale.set_count(MAX_DEGREES));
    toybox::gpui_gui::GpuiHostedGui::new(
        "FreqsGpuiDenseScaleView",
        move |_, cx| cx.new(|_| Editor::screenshot(Arc::clone(&shared))).into(),
        WIDTH,
        HEIGHT,
    )
    .with_size_contract((WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_highlights_follow_root_and_wrap_in_24_note_scale() {
        let mut scale = Scale::equal(24);
        scale.root_degree = 5;
        let mut keys = [0_u64; 2];
        for key in [60_usize, 61, 84] {
            keys[key / 64] |= 1_u64 << (key % 64);
        }
        let degrees = played_degrees(scale, keys);
        assert!(degrees[5]);
        assert!(degrees[6]);
        assert_eq!(degrees.iter().filter(|played| **played).count(), 2);
    }
}
