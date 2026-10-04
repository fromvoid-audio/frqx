//! Native GPUI editor for node tuning, pitch space, and audition.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use toybox::gpui::{
    self as gpui, canvas, div, fill, point, prelude::*, px, rgba, Bounds, Context, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, Window,
};

use toybox::gpui_gui::{
    NumericInput, NumericInputCanceled, NumericInputConfig, NumericInputRange, NumericInputStepped,
    NumericInputSubmitted,
};

use crate::analysis::pair_fit;
use crate::library;
use crate::scale::{nearest_lattice, Scale, MAX_DEGREES};
use crate::shared::{MasterStatus, Shared};
use crate::synth::{ATTACK_PARAM, LEVEL_PARAM, RELEASE_PARAM, WAVEFORM_PARAM};

/// Fixed editor width.
pub const WIDTH: u32 = 1600;
/// Fixed editor height.
pub const HEIGHT: u32 = 1000;

type GraphBounds = Rc<RefCell<Option<Bounds<Pixels>>>>;

struct Editor {
    shared: Arc<Shared>,
    pair: (usize, usize),
    dragging: Option<usize>,
    pitch_bounds: GraphBounds,
    palette_bounds: GraphBounds,
    node_inputs: Vec<(gpui::Entity<NumericInput>, gpui::Entity<NumericInput>)>,
    node_input_subscriptions: Vec<gpui::Subscription>,
    count_input: Option<gpui::Entity<NumericInput>>,
    count_subscriptions: Vec<gpui::Subscription>,
    node_scroll: gpui::ScrollHandle,
    node_input_revision: u64,
    node_input_epoch: u64,
    node_context_menu: Option<(usize, f32, f32)>,
    selected_node: usize,
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
}

impl Editor {
    fn new(shared: Arc<Shared>) -> Self {
        let (initial, initial_revision) = shared.snapshot_with_revision();
        Self {
            shared,
            pair: (0, 4),
            dragging: None,
            pitch_bounds: Rc::new(RefCell::new(None)),
            palette_bounds: Rc::new(RefCell::new(None)),
            node_inputs: Vec::new(),
            node_input_subscriptions: Vec::new(),
            count_input: None,
            count_subscriptions: Vec::new(),
            node_scroll: gpui::ScrollHandle::new(),
            node_input_revision: initial_revision,
            node_input_epoch: 0,
            node_context_menu: None,
            selected_node: 4,
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
        let (_, revision) = self
            .shared
            .edit_with_revision(|scale| scale.set_equal(scale.count));
        self.node_input_revision = revision;
        cx.notify();
    }

    fn just(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (_, revision) = self.shared.edit_with_revision(Scale::set_just_major);
        self.node_input_revision = revision;
        self.pair = (0, 4);
        cx.notify();
    }

    fn ensure_count_input(&mut self, count: usize, cx: &mut Context<Self>) {
        if self.count_input.is_none() {
            let input = cx.new(|cx| {
                NumericInput::new(
                    NumericInputConfig::new(
                        count as f32,
                        NumericInputRange::new(3.0, MAX_DEGREES as f32),
                        1.0,
                        1.0,
                        |value| format!("{value:.0}"),
                    ),
                    cx,
                )
            });
            self.count_subscriptions.push(cx.subscribe(
                &input,
                |this, _, event: &NumericInputSubmitted, cx| {
                    if let Ok(count) = event.text.trim().parse::<usize>() {
                        this.set_note_count(count, cx);
                    }
                    this.sync_count_input(cx);
                },
            ));
            self.count_subscriptions.push(
                cx.subscribe(&input, |this, _, _: &NumericInputCanceled, cx| {
                    this.sync_count_input(cx)
                }),
            );
            self.count_subscriptions.push(cx.subscribe(
                &input,
                |this, _, event: &NumericInputStepped, cx| {
                    let count = this.shared.snapshot().count as f32 + event.delta;
                    this.set_note_count(count.clamp(3.0, MAX_DEGREES as f32) as usize, cx);
                    this.sync_count_input(cx);
                },
            ));
            self.count_input = Some(input);
        }
        if let Some(input) = &self.count_input {
            input.update(cx, |input, cx| {
                if !input.is_editing() && input.value() != count as f32 {
                    input.set_value(count as f32, cx);
                }
            });
        }
    }

    fn sync_count_input(&self, cx: &mut Context<Self>) {
        if let Some(input) = &self.count_input {
            let count = self.shared.snapshot().count;
            input.update(cx, |input, cx| {
                input.set_value(count as f32, cx);
                input.set_text(count.to_string(), cx);
            });
        }
        cx.notify();
    }

    fn set_note_count(&mut self, count: usize, cx: &mut Context<Self>) {
        let (scale, revision) = self
            .shared
            .edit_with_revision(|scale| scale.set_count(count));
        self.node_input_revision = revision;
        self.node_context_menu = None;
        self.selected_node = self.selected_node.min(scale.count - 1);
        self.pair = (scale.root_degree, self.selected_node);
        self.variations = variations(scale, self.max_move);
        self.sync_count_input(cx);
    }

    fn open_node_context(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Right {
            return;
        }
        self.selected_node = index.min(self.shared.snapshot().count.saturating_sub(1));
        self.pair = (self.shared.snapshot().root_degree, self.selected_node);
        self.node_context_menu = Some((
            self.selected_node,
            f32::from(event.position.x),
            f32::from(event.position.y),
        ));
        cx.stop_propagation();
        cx.notify();
    }

    fn select_node(&mut self, index: usize, cx: &mut Context<Self>) {
        let scale = self.shared.snapshot();
        self.selected_node = index.min(scale.count.saturating_sub(1));
        self.pair = (scale.root_degree, self.selected_node);
        self.node_context_menu = None;
        cx.notify();
    }

    fn add_node_after(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .node_context_menu
            .map(|(index, _, _)| index)
            .unwrap_or(self.selected_node);
        let mut inserted = None;
        let result = self
            .shared
            .edit_if_revision(self.node_input_revision, |scale| {
                inserted = scale.insert_degree_after(index)
            });
        if let (Some(inserted), Some((scale, revision))) = (inserted, result) {
            self.node_input_revision = revision;
            self.selected_node = inserted;
            self.variations = variations(scale, self.max_move);
            self.message = format!("Added node {}", inserted + 1);
        } else if result.is_some() {
            self.message = "No room to add a node in this interval".into();
        } else {
            self.message = "Scale changed; choose a node again".into();
        }
        self.node_context_menu = None;
        cx.notify();
    }

    fn remove_selected_node(
        &mut self,
        _: &gpui::ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self
            .node_context_menu
            .map(|(index, _, _)| index)
            .unwrap_or(self.selected_node);
        self.remove_node(index, cx);
    }

    fn remove_node(&mut self, index: usize, cx: &mut Context<Self>) {
        let mut removed = false;
        let result = self
            .shared
            .edit_if_revision(self.node_input_revision, |scale| {
                removed = scale.remove_degree(index)
            });
        if removed {
            if let Some((scale, revision)) = result {
                self.node_input_revision = revision;
                self.selected_node = index.saturating_sub(1);
                self.pair = (scale.root_degree, self.selected_node);
                self.variations = variations(scale, self.max_move);
                self.message = format!("Removed node {}", index + 1);
            } else {
                self.message = "Scale changed; choose a node again".into();
            }
        } else if result.is_some() {
            self.message = "Keep at least three nodes and retain the root".into();
        } else {
            self.message = "Scale changed; choose a node again".into();
        }
        self.node_context_menu = None;
        cx.notify();
    }

    fn apply_node_frequency(
        &mut self,
        index: usize,
        hz: f64,
        expected_revision: u64,
        cx: &mut Context<Self>,
    ) {
        let result = self.shared.edit_if_revision(expected_revision, |scale| {
            scale.set_degree_frequency(index, hz);
        });
        let Some((scale, revision)) = result else {
            cx.notify();
            return;
        };
        self.node_input_revision = revision;
        self.pair = (scale.root_degree, index.min(scale.count - 1));
        self.selected_node = index;
        self.variations = variations(scale, self.max_move);
        self.sync_node_input(index, scale, cx);
        cx.notify();
    }

    fn apply_node_cents(
        &mut self,
        index: usize,
        cents: f32,
        expected_revision: u64,
        cx: &mut Context<Self>,
    ) {
        let result = self.shared.edit_if_revision(expected_revision, |scale| {
            scale.move_degree(index, cents);
        });
        let Some((scale, revision)) = result else {
            cx.notify();
            return;
        };
        self.node_input_revision = revision;
        self.pair = (scale.root_degree, index.min(scale.count - 1));
        self.selected_node = index;
        self.variations = variations(scale, self.max_move);
        self.sync_node_input(index, scale, cx);
        cx.notify();
    }

    fn sync_node_input(&self, index: usize, scale: Scale, cx: &mut Context<Self>) {
        if let Some((hz_input, cents_input)) = self.node_inputs.get(index) {
            let hz = scale.degree_frequency(index).unwrap_or(scale.root_hz) as f32;
            let cents = scale.cents[index];
            hz_input.update(cx, |input, cx| {
                input.set_value(hz, cx);
                input.set_text(format!("{hz:.2}"), cx);
            });
            cents_input.update(cx, |input, cx| {
                input.set_value(cents, cx);
                input.set_text(format!("{cents:.1}"), cx);
            });
        }
    }

    fn ensure_node_inputs(&mut self, scale: Scale, revision: u64, cx: &mut Context<Self>) {
        if self.node_input_revision != revision {
            self.node_input_revision = revision;
            self.node_input_epoch = self.node_input_epoch.wrapping_add(1);
            self.node_inputs.clear();
            self.node_input_subscriptions.clear();
            self.node_context_menu = None;
            self.selected_node = self.selected_node.min(scale.count.saturating_sub(1));
        }
        if self.node_inputs.len() != scale.count {
            self.node_input_epoch = self.node_input_epoch.wrapping_add(1);
            self.node_inputs.clear();
            self.node_input_subscriptions.clear();
            let epoch = self.node_input_epoch;
            for index in 0..scale.count {
                let hz = scale.degree_frequency(index).unwrap_or(scale.root_hz) as f32;
                let cents = scale.cents[index];
                let hz_input = cx.new(|cx| {
                    NumericInput::new(
                        NumericInputConfig::new(
                            hz,
                            NumericInputRange::new(
                                if index == scale.root_degree {
                                    10.0
                                } else {
                                    2.5
                                },
                                if index == scale.root_degree {
                                    20_000.0
                                } else {
                                    80_000.0
                                },
                            ),
                            1.0,
                            0.01,
                            |value| format!("{value:.2}"),
                        ),
                        cx,
                    )
                });
                let cents_input = cx.new(|cx| {
                    NumericInput::new(
                        NumericInputConfig::new(
                            cents,
                            NumericInputRange::new(0.0, 2400.0),
                            1.0,
                            0.1,
                            |value| format!("{value:.1}"),
                        ),
                        cx,
                    )
                });

                let subscription = cx.subscribe(
                    &hz_input,
                    move |this: &mut Self, _, event: &NumericInputSubmitted, cx| {
                        if this.node_input_epoch == epoch
                            && this.node_input_revision == this.shared.revision()
                            && index < this.shared.snapshot().count
                        {
                            if let Ok(value) = event.text.parse::<f64>() {
                                let expected = this.node_input_revision;
                                this.apply_node_frequency(index, value, expected, cx);
                            } else {
                                let (scale, _) = this.shared.snapshot_with_revision();
                                this.sync_node_input(index, scale, cx);
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                let subscription = cx.subscribe(
                    &hz_input,
                    move |this: &mut Self, _, _: &NumericInputCanceled, cx| {
                        if this.node_input_epoch == epoch {
                            let (scale, revision) = this.shared.snapshot_with_revision();
                            if revision == this.node_input_revision {
                                this.sync_node_input(index, scale, cx);
                            } else {
                                cx.notify();
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                let subscription = cx.subscribe(
                    &hz_input,
                    move |this: &mut Self, _, event: &NumericInputStepped, cx| {
                        if this.node_input_epoch == epoch
                            && this.node_input_revision == this.shared.revision()
                            && index < this.shared.snapshot().count
                        {
                            let (scale, revision) = this.shared.snapshot_with_revision();
                            if revision == this.node_input_revision {
                                if let Some(value) = scale.degree_frequency(index) {
                                    this.apply_node_frequency(
                                        index,
                                        value + f64::from(event.delta),
                                        revision,
                                        cx,
                                    );
                                }
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                let subscription = cx.subscribe(
                    &cents_input,
                    move |this: &mut Self, _, event: &NumericInputSubmitted, cx| {
                        if this.node_input_epoch == epoch
                            && this.node_input_revision == this.shared.revision()
                            && index < this.shared.snapshot().count
                        {
                            if let Ok(value) = event.text.parse::<f32>() {
                                let expected = this.node_input_revision;
                                this.apply_node_cents(index, value, expected, cx);
                            } else {
                                let (scale, _) = this.shared.snapshot_with_revision();
                                this.sync_node_input(index, scale, cx);
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                let subscription = cx.subscribe(
                    &cents_input,
                    move |this: &mut Self, _, _: &NumericInputCanceled, cx| {
                        if this.node_input_epoch == epoch {
                            let (scale, revision) = this.shared.snapshot_with_revision();
                            if revision == this.node_input_revision {
                                this.sync_node_input(index, scale, cx);
                            } else {
                                cx.notify();
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                let subscription = cx.subscribe(
                    &cents_input,
                    move |this: &mut Self, _, event: &NumericInputStepped, cx| {
                        if this.node_input_epoch == epoch
                            && this.node_input_revision == this.shared.revision()
                            && index < this.shared.snapshot().count
                        {
                            let (scale, revision) = this.shared.snapshot_with_revision();
                            if revision == this.node_input_revision {
                                this.apply_node_cents(
                                    index,
                                    scale.cents[index] + event.delta,
                                    revision,
                                    cx,
                                );
                            }
                        }
                    },
                );
                self.node_input_subscriptions.push(subscription);
                self.node_inputs.push((hz_input, cents_input));
            }
        }
        for (index, (hz_input, cents_input)) in self.node_inputs.iter().enumerate() {
            let hz = scale.degree_frequency(index).unwrap_or(scale.root_hz) as f32;
            let cents = scale.cents[index];
            hz_input.update(cx, |input, cx| {
                if !input.is_editing() && (input.value() - hz).abs() > 0.001 {
                    input.set_value(hz, cx);
                }
            });
            cents_input.update(cx, |input, cx| {
                if !input.is_editing() && (input.value() - cents).abs() > 0.001 {
                    input.set_value(cents, cx);
                }
            });
        }
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
        let (_, revision) = self.shared.edit_with_revision(|scale| {
            scale.root_degree = (scale.root_degree + scale.count - 1) % scale.count
        });
        let _ = revision;
        self.node_input_revision = 0;
        cx.notify();
    }

    fn root_next(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (_, revision) = self
            .shared
            .edit_with_revision(|scale| scale.root_degree = (scale.root_degree + 1) % scale.count);
        let _ = revision;
        self.node_input_revision = 0;
        cx.notify();
    }

    fn key_previous(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (_, revision) = self
            .shared
            .edit_with_revision(|scale| scale.root_key = scale.root_key.saturating_sub(1));
        self.node_input_revision = revision;
        cx.notify();
    }

    fn key_next(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (_, revision) = self
            .shared
            .edit_with_revision(|scale| scale.root_key = scale.root_key.saturating_add(1).min(127));
        self.node_input_revision = revision;
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
                self.node_input_revision = 0;
                self.node_input_epoch = self.node_input_epoch.wrapping_add(1);
                self.node_inputs.clear();
                self.node_input_subscriptions.clear();
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
        let pitch_bounds = *self.pitch_bounds.borrow();
        if let Some(bounds) = pitch_bounds {
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
                if event.click_count == 2 && (y - f32::from(bounds.top()) - 51.0).abs() <= 16.0 {
                    self.dragging = None;
                    self.node_context_menu = None;
                    let distance =
                        (x - pitch_x(bounds, scale.cents[index] / scale.period_cents * 1200.0))
                            .abs();
                    if distance <= 12.0 {
                        self.remove_node(index, cx);
                    } else {
                        let cents = (x - pitch_x(bounds, 0.0))
                            / (pitch_x(bounds, 1200.0) - pitch_x(bounds, 0.0))
                            * scale.period_cents;
                        self.insert_and_drag_node(cents);
                    }
                    cx.notify();
                    return;
                }
                if (x - pitch_x(bounds, scale.cents[index] / scale.period_cents * 1200.0)).abs()
                    < 20.0
                {
                    self.pair = (scale.root_degree, index);
                    self.selected_node = index;
                    if index > 0 {
                        self.dragging = Some(index);
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
                        self.node_input_revision = 0;
                    }
                }
                cx.notify();
            }
        }
    }

    fn insert_and_drag_node(&mut self, cents: f32) {
        self.dragging = None;
        let mut inserted = None;
        if let Some((scale, revision)) = self
            .shared
            .edit_if_revision(self.node_input_revision, |scale| {
                inserted = scale.insert_degree(cents)
            })
        {
            self.node_input_revision = revision;
            if let Some(index) = inserted {
                self.selected_node = index;
                self.pair = (scale.root_degree, index);
                self.dragging = Some(index);
                self.variations = variations(scale, self.max_move);
                self.message = format!("Added node {}", index + 1);
            } else {
                self.message = "No room to add a node here".into();
            }
        } else {
            self.message = "Scale changed; try again".into();
        }
    }

    fn graph_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
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
        self.retune_dragged_node(index, cents);
        cx.notify();
    }

    fn retune_dragged_node(&mut self, index: usize, cents: f32) {
        let mut changed = false;
        let (scale, revision) = self.shared.edit_with_revision(|scale| {
            changed = scale.move_degree(index, cents);
        });
        self.node_input_revision = revision;
        if changed {
            self.variations = variations(scale, self.max_move);
        }
    }

    fn graph_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging = None;
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
        let mut resulting_revision = None;
        match self.control_drag {
            Some(0) => {
                let (_, revision) = self.shared.edit_with_revision(|scale| {
                    scale.root_hz = 55.0 * 2.0_f64.powf(f64::from(t) * 5.0)
                });
                resulting_revision = Some(revision);
            }
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
                let (_, revision) = self
                    .shared
                    .edit_with_revision(|scale| scale.set_period(snapped));
                resulting_revision = Some(revision);
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
        if let Some(revision) = resulting_revision {
            self.node_input_revision = revision;
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        window.request_animation_frame();
        let (scale, scale_revision) = self.shared.snapshot_with_revision();
        let (revision, last_key) = self.shared.last_played();
        if revision != self.last_played_revision {
            self.last_played_revision = revision;
            if let Ok(key) = u8::try_from(last_key) {
                self.recent_played = Some((key, Instant::now()));
            }
        }
        self.ensure_node_inputs(scale, scale_revision, cx);
        self.ensure_count_input(scale.count, cx);
        let held_degrees = played_degrees(scale, self.shared.held_keys());
        let recent_degree = self.recent_played.and_then(|(key, when)| {
            (when.elapsed() < Duration::from_millis(160)).then(|| degree_for_key(scale, key))
        });
        let settings = self.shared.synth_settings();
        let pair = (
            self.pair.0.min(scale.count - 1),
            self.pair.1.min(scale.count - 1),
        );
        let pitch_bounds = Rc::clone(&self.pitch_bounds);
        let palette_bounds = Rc::clone(&self.palette_bounds);
        let root_bounds = Rc::clone(&self.root_bounds);
        let period_bounds = Rc::clone(&self.period_bounds);
        let max_move_bounds = Rc::clone(&self.max_move_bounds);
        let level_bounds = Rc::clone(&self.level_bounds);
        let attack_bounds = Rc::clone(&self.attack_bounds);
        let release_bounds = Rc::clone(&self.release_bounds);
        let candidates = self.variations.clone();
        let max_move = self.max_move;
        let palette_graph = div()
            .id("pitch-space-graph")
            .w_full()
            .h(px(236.0))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::graph_down))
            .on_mouse_move(cx.listener(Self::graph_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::graph_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::graph_up))
            .child(
                canvas(
                    move |bounds, _, _| *palette_bounds.borrow_mut() = Some(bounds),
                    move |bounds, _, window, cx| {
                        paint_palette(bounds, scale, &candidates, window, cx)
                    },
                )
                .size_full(),
            );
        let pitch_graph = div()
            .id("pitch-line-graph")
            .w_full()
            .h(px(104.0))
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
        let mut node_rows = Vec::with_capacity(scale.count);
        for index in 0..scale.count {
            let hz_input = self.node_inputs[index].0.clone();
            let cents_input = self.node_inputs[index].1.clone();
            let selected = index == pair.1;
            node_rows.push(
                div()
                    .id(format!("freq-node-row-{index}"))
                    .w(px(704.0))
                    .h(px(29.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(0.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.select_node(index, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event, window, cx| {
                            this.open_node_context(index, event, window, cx)
                        }),
                    )
                    .child(
                        div()
                            .w(px(56.0))
                            .text_size(px(12.0))
                            .text_color(rgba(if selected { 0xff695fff } else { 0xa9b4acff }))
                            .child(format!("{:02}", index + 1)),
                    )
                    .child(div().w(px(12.0)))
                    .child(
                        div()
                            .w(px(292.0))
                            .h(px(26.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .bg(rgba(0x101615ff))
                            .border_1()
                            .border_color(rgba(0x303a35ff))
                            .rounded(px(5.0))
                            .child(hz_input),
                    )
                    .child(div().w(px(22.0)))
                    .child(
                        div()
                            .w(px(322.0))
                            .h(px(26.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .bg(rgba(0x101615ff))
                            .border_1()
                            .border_color(rgba(0x303a35ff))
                            .rounded(px(5.0))
                            .child(cents_input),
                    ),
            );
        }
        let node_scrollbar = if scale.count > 7 {
            let viewport = 226.0;
            let max_scroll = f32::from(self.node_scroll.max_offset().y).max(0.0);
            let content_height = viewport + max_scroll;
            let thumb_height = (viewport * viewport / content_height).max(20.0);
            let progress =
                (-f32::from(self.node_scroll.offset().y) / max_scroll.max(1.0)).clamp(0.0, 1.0);
            div()
                .absolute()
                .right(px(7.0))
                .top(px(93.0))
                .w(px(4.0))
                .h(px(viewport))
                .bg(rgba(0x232b28ff))
                .rounded(px(2.0))
                .child(
                    div()
                        .absolute()
                        .top(px(progress * (viewport - thumb_height)))
                        .left(px(0.0))
                        .right(px(0.0))
                        .h(px(thumb_height))
                        .bg(rgba(0x67736cff))
                        .rounded(px(2.0)),
                )
        } else {
            div().hidden()
        };
        let (status, status_color) = match self.shared.status() {
            MasterStatus::Connecting => ("connecting".to_owned(), 0x777a78ff),
            MasterStatus::Active => (
                format!("active  ·  {} clients", self.shared.clients()),
                0x9ad4baff,
            ),
            MasterStatus::Blocked => ("another master active".to_owned(), 0xc18c67ff),
            MasterStatus::Unavailable => ("library unavailable".to_owned(), 0x777a78ff),
        };
        let settings_popover = if self.show_settings {
            panel()
                .id("settings-popover")
                .absolute()
                .top(px(68.0))
                .right(px(62.0))
                .w(px(245.0))
                .p(px(14.0))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
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
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(9.0))
                .text_size(px(12.0))
                .child("EDIT NODE PITCH IN HZ OR CENTS")
                .child("DOUBLE-CLICK THE PITCH LINE TO ADD A NOTE")
                .child("DOUBLE-CLICK A NOTE TO REMOVE IT")
                .child("RIGHT-CLICK A NODE FOR MORE ACTIONS")
                .child("SCROLL THE NODE LIST TO SEE MORE NOTES")
                .child("DRAG A NOTE ON THE PITCH LINE TO RETUNE")
                .child("CHOOSE A PITCH-SPACE POINT TO APPLY A VARIATION")
                .child("PLAY MIDI TO HEAR AND HIGHLIGHT NOTES")
                .child("SHIFT-DRAG THE REPEAT SLIDER TO SNAP RATIOS")
        } else {
            div().id("help-popover").hidden()
        };
        let node_context_menu = self.node_context_menu.map(|(index, x, y)| {
            let left = (x - 34.0).clamp(12.0, WIDTH as f32 - 220.0);
            let top = (y - 34.0).clamp(92.0, HEIGHT as f32 - 112.0);
            let can_remove = index != scale.root_degree && scale.count > 3;
            div()
                .id("node-context-menu")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(204.0))
                .p(px(6.0))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(4.0))
                .bg(rgba(0x1a2220ff))
                .border_1()
                .border_color(rgba(0x48534cff))
                .rounded(px(8.0))
                .child(
                    control_button("Add node after", false)
                        .id("add-node-after")
                        .w_full()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(Self::add_node_after)),
                )
                .child(
                    control_button("Remove node", false)
                        .id("remove-node")
                        .w_full()
                        .text_color(rgba(if can_remove { 0xd8c6c0ff } else { 0x69716bff }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .when(can_remove, |button| {
                            button.on_click(cx.listener(Self::remove_selected_node))
                        }),
                )
        });
        let context_dismiss = if self.node_context_menu.is_some() {
            div()
                .id("dismiss-node-menu")
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.node_context_menu = None;
                        cx.notify();
                    }),
                )
        } else {
            div().id("dismiss-node-menu").hidden()
        };
        div()
            .relative()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .px(px(34.0))
            .pt(px(34.0))
            .pb(px(36.0))
            .bg(rgba(0x0d1111ff))
            .text_color(rgba(0xedf0e9ff))
            .child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| paint_bezel(bounds, window),
                )
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .w_full()
                .h_full(),
            )
            .child(
                div()
                    .h(px(42.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(34.0))
                            .font_family(".SystemUIFont")
                            .text_color(rgba(0xff695fff))
                            .child("freqs"),
                    )
                    .child(
                        div()
                            .ml(px(20.0))
                            .mt(px(8.0))
                            .text_size(px(10.0))
                            .text_color(rgba(0x87928aff))
                            .child("MTS-ESP TUNING SYSTEM"),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_size(px(10.0))
                            .text_color(rgba(0x69736cff))
                            .child(format!("v{}", crate::BUILD_VERSION)),
                    )
                    .child(
                        div()
                            .ml(px(20.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(7.0))
                                    .h(px(7.0))
                                    .rounded(px(4.0))
                                    .bg(rgba(status_color)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgba(0xa9b4acff))
                                    .child("MTS-ESP"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgba(0x87928aff))
                                    .child(status),
                            ),
                    )
                    .child(
                        control_button("⚙", self.show_settings)
                            .id("settings")
                            .ml(px(14.0))
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
                    .mt(px(28.0))
                    .h(px(336.0))
                    .flex_shrink_0()
                    .flex()
                    .gap(px(20.0))
                    .child(
                        panel()
                            .w(px(756.0))
                            .h_full()
                            .p(px(18.0))
                            .flex()
                            .flex_col()
                            .relative()
                            .child(section_title("01  /  Nodes"))
                            .child(
                                div()
                                    .ml(px(12.0))
                                    .mt(px(34.0))
                                    .h(px(16.0))
                                    .flex()
                                    .gap(px(0.0))
                                    .text_size(px(10.0))
                                    .text_color(rgba(0xa9b4acff))
                                    .child(div().w(px(68.0)).child("NODE"))
                                    .child(div().w(px(12.0)))
                                    .child(div().w(px(292.0)).child("PITCH  ·  Hz"))
                                    .child(div().w(px(22.0)))
                                    .child(div().w(px(322.0)).child("CENTS")),
                            )
                            .child(
                                div().ml(px(12.0)).mt(px(7.0)).flex().gap(px(12.0)).child(
                                    div()
                                        .id("node-list-scroll")
                                        .w(px(704.0))
                                        .h(px(226.0))
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.0))
                                        .overflow_y_scroll()
                                        .track_scroll(&self.node_scroll)
                                        .children(node_rows),
                                ),
                            )
                            .child(node_scrollbar),
                    )
                    .child(
                        panel()
                            .w(px(756.0))
                            .h_full()
                            .p(px(18.0))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(26.0))
                                    .flex()
                                    .items_center()
                                    .child(section_title("02  /  Pitch space"))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_size(px(10.0))
                                            .text_color(rgba(0xa9b4acff))
                                            .child(format!("REACH  ±{max_move:.0}¢")),
                                    )
                                    .child(div().ml(px(12.0)).w(px(132.0)).child(max_move_slider)),
                            )
                            .child(palette_graph)
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .h(px(28.0))
                                    .flex()
                                    .items_center()
                                    .child(div().flex_1())
                                    .child(
                                        control_button("⚄", false)
                                            .id("regenerate")
                                            .ml_auto()
                                            .w(px(38.0))
                                            .h(px(28.0))
                                            .on_click(cx.listener(Self::regenerate)),
                                    ),
                            ),
                    ),
            )
            .child(
                panel()
                    .mt(px(20.0))
                    .h(px(142.0))
                    .flex_shrink_0()
                    .px(px(16.0))
                    .py(px(11.0))
                    .flex()
                    .flex_col()
                    .child(section_title("03  /  Pitch line"))
                    .child(pitch_graph),
            )
            .child(
                div()
                    .mt(px(20.0))
                    .h(px(256.0))
                    .flex_shrink_0()
                    .flex()
                    .gap(px(20.0))
                    .child(
                        panel()
                            .w(px(756.0))
                            .h_full()
                            .p(px(22.0))
                            .flex()
                            .flex_col()
                            .child(section_title("04  /  Reference"))
                            .child(
                                div()
                                    .mt(px(18.0))
                                    .h(px(68.0))
                                    .flex()
                                    .gap(px(0.0))
                                    .child(
                                        div()
                                            .w(px(220.0))
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .child(field_label("ROOT DEGREE"))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.0))
                                                    .child(
                                                        control_button("‹", false)
                                                            .id("root-previous")
                                                            .on_click(
                                                                cx.listener(Self::root_previous),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(36.0))
                                                            .text_size(px(16.0))
                                                            .child(format!(
                                                                "{:02}",
                                                                scale.root_degree + 1
                                                            )),
                                                    )
                                                    .child(
                                                        control_button("›", false)
                                                            .id("root-next")
                                                            .on_click(cx.listener(Self::root_next)),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .ml(px(34.0))
                                            .w(px(250.0))
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .child(field_value_label(
                                                "ROOT PITCH",
                                                format!("{:.2} Hz", scale.root_hz),
                                            ))
                                            .child(root_slider),
                                    )
                                    .child(
                                        div()
                                            .ml(px(93.0))
                                            .w(px(119.0))
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .child(field_label("MIDI KEY"))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.0))
                                                    .child(
                                                        control_button("‹", false)
                                                            .id("key-previous")
                                                            .on_click(
                                                                cx.listener(Self::key_previous),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(28.0))
                                                            .text_size(px(16.0))
                                                            .child(format!(
                                                                "{:02}",
                                                                scale.root_key
                                                            )),
                                                    )
                                                    .child(
                                                        control_button("›", false)
                                                            .id("key-next")
                                                            .on_click(cx.listener(Self::key_next)),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(div().mt(px(12.0)).h(px(1.0)).bg(rgba(0x252e2aff)))
                            .child(
                                div().mt(px(10.0)).h(px(16.0)).flex().items_center().child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .text_size(px(10.0))
                                        .child(
                                            div()
                                                .text_color(rgba(0xa9b4acff))
                                                .child("OCTAVE / REPEAT"),
                                        )
                                        .child(div().ml_auto().text_color(rgba(0xedf0e9ff)).child(
                                            format!(
                                                        "{:.1}¢  ·  {:.3}:1",
                                                        scale.period_cents,
                                                        2.0_f64.powf(
                                                            f64::from(scale.period_cents) / 1200.0
                                                        )
                                                    ),
                                        )),
                                ),
                            )
                            .child(div().mt(px(12.0)).child(period_slider))
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .h(px(26.0))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(10.0))
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(rgba(0xa9b4acff))
                                            .child("SNAP RATIOS"),
                                    )
                                    .child(
                                        control_button(
                                            if self.snap_period { "ON" } else { "OFF" },
                                            self.snap_period,
                                        )
                                        .id("shift-snaps")
                                        .w(px(64.0))
                                        .h(px(26.0))
                                        .on_click(cx.listener(Self::toggle_snap_period)),
                                    ),
                            ),
                    )
                    .child(
                        panel()
                            .w(px(756.0))
                            .h_full()
                            .p(px(20.0))
                            .flex()
                            .flex_col()
                            .child(section_title("05  /  Slots & structure"))
                            .child(
                                div()
                                    .mt(px(18.0))
                                    .text_size(px(10.0))
                                    .text_color(rgba(0xa9b4acff))
                                    .child("NOTES PER REPEAT"),
                            )
                            .child(
                                div()
                                    .mt(px(7.0))
                                    .h(px(38.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        control_button("−", false)
                                            .id("fewer")
                                            .w(px(40.0))
                                            .on_click(cx.listener(Self::fewer)),
                                    )
                                    .child(
                                        control_button("+", false)
                                            .id("more")
                                            .w(px(40.0))
                                            .on_click(cx.listener(Self::more)),
                                    )
                                    .child(
                                        div()
                                            .id("note-count")
                                            .w(px(100.0))
                                            .h(px(38.0))
                                            .px(px(8.0))
                                            .flex()
                                            .items_center()
                                            .bg(rgba(0x101615ff))
                                            .border_1()
                                            .border_color(rgba(0x48534cff))
                                            .rounded(px(5.0))
                                            .child(
                                                self.count_input
                                                    .as_ref()
                                                    .expect("count input initialized")
                                                    .clone(),
                                            ),
                                    ),
                            )
                            .child(div().mt(px(11.0)).h(px(1.0)).bg(rgba(0x252e2aff)))
                            .child(
                                div()
                                    .mt(px(11.0))
                                    .h(px(38.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        control_button("‹", false)
                                            .id("previous-slot")
                                            .w(px(40.0))
                                            .on_click(cx.listener(Self::previous_slot)),
                                    )
                                    .child(
                                        control_button("›", false)
                                            .id("next-slot")
                                            .w(px(40.0))
                                            .on_click(cx.listener(Self::next_slot)),
                                    )
                                    .child(
                                        div()
                                            .ml(px(4.0))
                                            .w(px(320.0))
                                            .h(px(34.0))
                                            .px(px(10.0))
                                            .flex()
                                            .items_center()
                                            .bg(rgba(0x101615ff))
                                            .border_1()
                                            .border_color(rgba(0x303a35ff))
                                            .rounded(px(6.0))
                                            .child(
                                                div()
                                                    .text_size(px(10.0))
                                                    .child(format!("SLOT {:02}", self.slot)),
                                            )
                                            .child(
                                                div()
                                                    .ml_auto()
                                                    .text_size(px(10.0))
                                                    .text_color(rgba(0xa9b4acff))
                                                    .child(if library::slot_exists(self.slot) {
                                                        "SAVED"
                                                    } else {
                                                        "EMPTY"
                                                    }),
                                            ),
                                    )
                                    .child(
                                        control_button("↓  SAVE", false)
                                            .id("save-slot")
                                            .ml(px(5.0))
                                            .w(px(130.0))
                                            .on_click(cx.listener(Self::save)),
                                    )
                                    .child(
                                        control_button("□  LOAD", false)
                                            .id("load-slot")
                                            .ml(px(5.0))
                                            .w(px(136.0))
                                            .on_click(cx.listener(Self::load)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .h(px(14.0))
                                    .text_size(px(10.0))
                                    .text_color(rgba(0x87928aff))
                                    .child(self.message.clone()),
                            ),
                    ),
            )
            .child(
                panel()
                    .mt(px(18.0))
                    .h(px(68.0))
                    .flex_shrink_0()
                    .px(px(22.0))
                    .py(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .w(px(244.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(78.0))
                                    .text_size(px(10.0))
                                    .text_color(rgba(0xff695fff))
                                    .child("AUDITION"),
                            )
                            .child(
                                control_button("△", settings.waveform.index() == 0)
                                    .id("triangle-wave")
                                    .w(px(40.0))
                                    .h(px(30.0))
                                    .on_click(cx.listener(Self::triangle)),
                            )
                            .child(
                                control_button("▧", settings.waveform.index() == 1)
                                    .id("square-wave")
                                    .w(px(40.0))
                                    .h(px(30.0))
                                    .on_click(cx.listener(Self::square)),
                            )
                            .child(
                                control_button("／", settings.waveform.index() == 2)
                                    .id("saw-wave")
                                    .w(px(40.0))
                                    .h(px(30.0))
                                    .on_click(cx.listener(Self::saw)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(366.0))
                            .ml(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .h(px(16.0))
                                    .flex()
                                    .items_center()
                                    .text_size(px(10.0))
                                    .child("LEVEL")
                                    .child(
                                        div()
                                            .ml_auto()
                                            .child(format!("{:.0}%", settings.level * 100.0)),
                                    ),
                            )
                            .child(div().w_full().child(level_slider)),
                    )
                    .child(
                        div()
                            .w(px(366.0))
                            .ml(px(34.0))
                            .flex()
                            .flex_col()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .h(px(16.0))
                                    .flex()
                                    .items_center()
                                    .text_size(px(10.0))
                                    .child("ATTACK")
                                    .child(
                                        div()
                                            .ml_auto()
                                            .child(format!("{:.0} ms", settings.attack_ms)),
                                    ),
                            )
                            .child(div().w_full().child(attack_slider)),
                    )
                    .child(
                        div()
                            .w(px(366.0))
                            .ml(px(34.0))
                            .flex()
                            .flex_col()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .h(px(16.0))
                                    .flex()
                                    .items_center()
                                    .text_size(px(10.0))
                                    .child("RELEASE")
                                    .child(
                                        div()
                                            .ml_auto()
                                            .child(format!("{:.0} ms", settings.release_ms)),
                                    ),
                            )
                            .child(div().w_full().child(release_slider)),
                    ),
            )
            .child(context_dismiss)
            .children(node_context_menu)
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
        .bg(gpui::linear_gradient(
            180.0,
            gpui::linear_color_stop(rgba(0x171e1bff), 0.0),
            gpui::linear_color_stop(rgba(0x111615ff), 1.0),
        ))
        .border_1()
        .border_color(rgba(0x303a35ff))
        .rounded(px(10.0))
}

fn section_title(title: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .h(px(18.0))
        .text_size(px(10.0))
        .text_color(rgba(0xa9b4acff))
        .child(title.to_uppercase())
}

fn field_label(label: &'static str) -> gpui::Div {
    div()
        .text_size(px(10.0))
        .text_color(rgba(0xa9b4acff))
        .child(label)
}

fn field_value_label(label: &'static str, value: String) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .text_size(px(10.0))
        .child(div().text_color(rgba(0xa9b4acff)).child(label))
        .child(div().ml_auto().text_color(rgba(0xedf0e9ff)).child(value))
}

fn control_button(label: impl Into<gpui::SharedString>, active: bool) -> gpui::Div {
    div()
        .min_w(px(36.0))
        .h(px(34.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(if active { 0x30201eff } else { 0x1a2220ff }))
        .border_1()
        .border_color(rgba(if active { 0xaa514bff } else { 0x303a35ff }))
        .rounded(px(7.0))
        .text_color(rgba(if active { 0xff695fff } else { 0xa9b4acff }))
        .text_size(px(13.0))
        .child(label.into())
}

fn rect(window: &mut Window, left: f32, top: f32, right: f32, bottom: f32, color: u32) {
    window.paint_quad(fill(
        Bounds::from_corners(point(px(left), px(top)), point(px(right), px(bottom))),
        rgba(color),
    ));
}

fn paint_gradient_polygon(
    window: &mut Window,
    points: &[(f32, f32)],
    angle: f32,
    start: u32,
    end: u32,
) {
    let Some(&(first_x, first_y)) = points.first() else {
        return;
    };
    let mut path = PathBuilder::fill();
    path.move_to(point(px(first_x), px(first_y)));
    for &(x, y) in points.iter().skip(1) {
        path.line_to(point(px(x), px(y)));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(
            path,
            gpui::linear_gradient(
                angle,
                gpui::linear_color_stop(rgba(start), 0.0),
                gpui::linear_color_stop(rgba(end), 1.0),
            ),
        );
    }
}

/// Draw the tapered metal plates and inset perimeter from the Frame concept.
fn paint_bezel(bounds: Bounds<Pixels>, window: &mut Window) {
    let sx = f32::from(bounds.size.width) / 1600.0;
    let sy = f32::from(bounds.size.height) / 1000.0;
    let point_at = |x: f32, y: f32| {
        (
            f32::from(bounds.left()) + x * sx,
            f32::from(bounds.top()) + y * sy,
        )
    };
    let plate = |points: &[(f32, f32)]| {
        points
            .iter()
            .map(|&(x, y)| point_at(x, y))
            .collect::<Vec<_>>()
    };
    let top = plate(&[(652.0, 0.0), (948.0, 0.0), (884.0, 23.0), (716.0, 23.0)]);
    let bottom = plate(&[
        (716.0, 979.0),
        (884.0, 979.0),
        (942.0, 1000.0),
        (658.0, 1000.0),
    ]);
    let left = plate(&[(0.0, 442.0), (29.0, 460.0), (29.0, 602.0), (0.0, 620.0)]);
    let right = plate(&[
        (1571.0, 460.0),
        (1600.0, 442.0),
        (1600.0, 620.0),
        (1571.0, 602.0),
    ]);
    paint_gradient_polygon(window, &top, 135.0, 0x35403aff, 0x2c3530ff);
    paint_gradient_polygon(window, &bottom, 135.0, 0x282f2bff, 0x222925ff);
    paint_gradient_polygon(window, &left, 135.0, 0x292f2cff, 0x222825ff);
    paint_gradient_polygon(window, &right, 135.0, 0x37413cff, 0x29312dff);

    for ((x1, y1), (x2, y2)) in [
        (point_at(680.0, 10.0), point_at(716.0, 23.0)),
        (point_at(884.0, 23.0), point_at(920.0, 10.0)),
        (point_at(1590.0, 448.0), point_at(1571.0, 460.0)),
        (point_at(1571.0, 602.0), point_at(1590.0, 614.0)),
        (point_at(920.0, 992.0), point_at(884.0, 979.0)),
        (point_at(716.0, 979.0), point_at(680.0, 992.0)),
        (point_at(10.0, 614.0), point_at(29.0, 602.0)),
        (point_at(29.0, 460.0), point_at(10.0, 448.0)),
    ] {
        line(window, (x1, y1), (x2, y2), 0x5c675fff, 1.0);
    }

    let outline = [
        (10.0, 10.0),
        (680.0, 10.0),
        (716.0, 23.0),
        (884.0, 23.0),
        (920.0, 10.0),
        (1590.0, 10.0),
        (1590.0, 448.0),
        (1571.0, 460.0),
        (1571.0, 602.0),
        (1590.0, 614.0),
        (1590.0, 990.0),
        (920.0, 990.0),
        (884.0, 979.0),
        (716.0, 979.0),
        (680.0, 990.0),
        (10.0, 990.0),
        (10.0, 614.0),
        (29.0, 602.0),
        (29.0, 460.0),
        (10.0, 448.0),
    ];
    let outline = plate(&outline);
    let mut path = PathBuilder::stroke(px(1.2));
    if let Some(&(x, y)) = outline.first() {
        path.move_to(point(px(x), px(y)));
        for &(x, y) in outline.iter().skip(1) {
            path.line_to(point(px(x), px(y)));
        }
        path.close();
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, rgba(0x46514aff));
    }
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
    rect(window, left, top, right, bottom, 0x131918ff);
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
        canvas_label(
            window,
            cx,
            (index + 1).to_string(),
            x - if scale.count > 24 { 4.0 } else { 5.0 },
            mid - 27.0,
            if scale.count > 24 { 8.5 } else { 10.0 },
            if selected { 0xff695fff } else { 0xa9b4acff },
        );
        if scale.count <= 12 || selected || is_root {
            let label = if index == 0 {
                "0.0¢".to_owned()
            } else {
                format!("+{:.1}¢", scale.cents[index])
            };
            canvas_label(window, cx, label, x - 22.0, bottom - 23.0, 10.0, 0xc9cbc8ff);
        }
    }
    let end = pitch_x(bounds, 1200.0);
    line(
        window,
        (end, top + 23.0),
        (end, bottom - 16.0),
        0x4d5752ff,
        1.0,
    );
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

fn paint_slider(bounds: Bounds<Pixels>, value: f32, window: &mut Window) {
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let right = f32::from(bounds.right());
    let bottom = f32::from(bounds.bottom());
    let middle = (top + bottom) * 0.5;
    let start = left + 2.0;
    let end = right - 2.0;
    let fill_end = start + value.clamp(0.0, 1.0) * (end - start);
    rect(window, left, top, right, bottom, 0x151919ff);
    rect(window, start, middle - 3.0, end, middle + 3.0, 0x343939ff);
    rect(
        window,
        start,
        middle - 3.0,
        fill_end,
        middle + 3.0,
        0xff6258ff,
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

fn paint_palette(
    bounds: Bounds<Pixels>,
    current: Scale,
    choices: &[Scale],
    window: &mut Window,
    cx: &mut gpui::App,
) {
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
            canvas_label(
                window,
                cx,
                "CURRENT SCALE".to_owned(),
                x + 13.0,
                y - 15.0,
                7.0,
                0xa9b4acff,
            );
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
    dense_screenshot_gui(false)
}

/// Construct the dense fixture scrolled to its last node.
#[cfg(feature = "screenshot-test")]
pub fn new_scrolled_screenshot_gui() -> toybox::gpui_gui::GpuiHostedGui {
    dense_screenshot_gui(true)
}

#[cfg(feature = "screenshot-test")]
fn dense_screenshot_gui(scrolled: bool) -> toybox::gpui_gui::GpuiHostedGui {
    let shared = Arc::new(Shared::default());
    shared.edit(|scale| scale.set_count(MAX_DEGREES));
    toybox::gpui_gui::GpuiHostedGui::new(
        "FreqsGpuiDenseScaleView",
        move |_, cx| {
            cx.new(|_| {
                let editor = Editor::screenshot(Arc::clone(&shared));
                if scrolled {
                    editor.node_scroll.scroll_to_bottom();
                }
                editor
            })
            .into()
        },
        WIDTH,
        HEIGHT,
    )
    .with_size_contract((WIDTH, HEIGHT), (WIDTH, HEIGHT), (WIDTH, HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newly_inserted_note_can_be_dragged_without_another_click() {
        let shared = Arc::new(Shared::default());
        let mut editor = Editor::new(Arc::clone(&shared));
        editor.insert_and_drag_node(250.0);
        let index = editor
            .dragging
            .expect("inserted note captures the held mouse");
        assert_eq!(editor.selected_node, index);
        assert_eq!(shared.snapshot().count, 8);
        editor.retune_dragged_node(index, 300.0);
        assert_eq!(shared.snapshot().cents[index], 300.0);
        assert_eq!(editor.variations[0], shared.snapshot());
        editor.insert_and_drag_node(300.0);
        assert_eq!(editor.dragging, None);
        assert_eq!(shared.snapshot().count, 8);
    }

    #[test]
    fn dragging_a_note_refreshes_pitch_space_from_the_current_tuning() {
        let shared = Arc::new(Shared::default());
        let mut editor = Editor::new(Arc::clone(&shared));
        let original_choices = editor.variations.clone();
        editor.retune_dragged_node(2, 450.0);
        let scale = shared.snapshot();
        assert_eq!(scale.cents[2], 450.0);
        assert_eq!(editor.variations[0], scale);
        assert_ne!(editor.variations, original_choices);
        assert_eq!(editor.node_input_revision, shared.revision());
        assert!(editor.variations.iter().all(Scale::valid));
    }

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
