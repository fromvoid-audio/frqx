//! Audio-thread-owned VST3 MIDI ingestion and audition rendering.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::shared::Shared;
use crate::synth::{Synth, SynthSettings};

struct AudioState {
    synth: Synth,
}

/// Exclusive audio-thread synth state for one processor instance.
pub(super) struct AudioOwner {
    state: UnsafeCell<AudioState>,
    processing: AtomicBool,
    reset: AtomicBool,
}

// SAFETY: processing is an exclusive compare-exchange guard around the only
// access to state; control threads only write the reset atomic.
unsafe impl Send for AudioOwner {}
// SAFETY: see the exclusive processing guard above.
unsafe impl Sync for AudioOwner {}

impl AudioOwner {
    pub(super) fn new() -> Self {
        Self {
            state: UnsafeCell::new(AudioState {
                synth: Synth::default(),
            }),
            processing: AtomicBool::new(false),
            reset: AtomicBool::new(false),
        }
    }

    pub(super) fn request_reset(&self) {
        self.reset.store(true, Ordering::Release);
    }

    pub(super) unsafe fn process(
        &self,
        data: &mut ProcessData,
        shared: &Shared,
        sample_rate: f64,
    ) -> tresult {
        if self
            .processing
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return kResultFalse;
        }
        struct Release<'a>(&'a AtomicBool);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _release = Release(&self.processing);
        // SAFETY: the compare-exchange guard above excludes a second callback.
        let state = unsafe { &mut *self.state.get() };
        if self.reset.swap(false, Ordering::AcqRel) {
            state.synth.clear();
        }
        state.synth.set_sample_rate(sample_rate);
        state.synth.retune(|key| shared.frequency(key));
        let input_events = data.inputEvents;
        let Some((left, right)) = (unsafe { output_buffers(data) }) else {
            return process_ok();
        };
        let settings = shared.synth_settings();
        let mut cursor = 0usize;
        if !input_events.is_null() {
            // SAFETY: the host owns the list for the duration of process().
            let list = input_events;
            let count = unsafe { ((*(*list).vtbl).getEventCount)(list) }.max(0);
            for index in 0..count {
                let mut event = MaybeUninit::<Event>::uninit();
                if unsafe { ((*(*list).vtbl).getEvent)(list, index, event.as_mut_ptr()) }
                    != kResultOk
                {
                    continue;
                }
                let event = unsafe { event.assume_init() };
                let target = usize::try_from(event.sampleOffset)
                    .unwrap_or_default()
                    .clamp(cursor, left.len());
                render_segment(
                    state,
                    &mut left[cursor..target],
                    &mut right[cursor..target],
                    settings,
                );
                cursor = target;
                match event.r#type as Event_::EventTypes {
                    Event_::EventTypes_::kNoteOnEvent => {
                        let note = unsafe { event.__field0.noteOn };
                        if let Ok(key) = u8::try_from(note.pitch) {
                            if key < 128 {
                                if note.velocity > 0.0 {
                                    state.synth.note_on(
                                        key,
                                        note.channel,
                                        note.noteId,
                                        note.velocity,
                                        shared.frequency(key),
                                    );
                                    shared.note_played(key);
                                } else {
                                    state.synth.note_off(key, note.channel, note.noteId);
                                }
                            }
                        }
                    }
                    Event_::EventTypes_::kNoteOffEvent => {
                        let note = unsafe { event.__field0.noteOff };
                        if let Ok(key) = u8::try_from(note.pitch) {
                            state.synth.note_off(key, note.channel, note.noteId);
                        }
                    }
                    _ => {}
                }
            }
        }
        render_segment(state, &mut left[cursor..], &mut right[cursor..], settings);
        shared.set_held_keys(state.synth.held_key_mask());
        process_ok()
    }
}

fn render_segment(
    state: &mut AudioState,
    left: &mut [f32],
    right: &mut [f32],
    settings: SynthSettings,
) {
    for (left, right) in left.iter_mut().zip(right) {
        let sample = state.synth.next_sample(settings);
        *left = sample;
        *right = sample;
    }
}

unsafe fn output_buffers(data: &mut ProcessData) -> Option<(&mut [f32], &mut [f32])> {
    if data.numOutputs != 1 || data.outputs.is_null() || data.numSamples < 0 {
        return None;
    }
    let bus = unsafe { &mut *data.outputs };
    if bus.numChannels != 2 {
        return None;
    }
    bus.silenceFlags = 0;
    let channels = unsafe { bus.__field0.channelBuffers32 };
    if channels.is_null() {
        return None;
    }
    let pointers = unsafe { slice::from_raw_parts(channels, 2) };
    if pointers[0].is_null() || pointers[1].is_null() {
        return None;
    }
    let frames = data.numSamples as usize;
    let left = unsafe { slice::from_raw_parts_mut(pointers[0], frames) };
    let right = unsafe { slice::from_raw_parts_mut(pointers[1], frames) };
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HostEvents(Vec<Event>);

    impl Class for HostEvents {
        type Interfaces = (IEventList,);
    }

    impl IEventListTrait for HostEvents {
        unsafe fn getEventCount(&self) -> i32 {
            self.0.len() as i32
        }

        unsafe fn getEvent(&self, index: i32, event: *mut Event) -> tresult {
            let Some(value) = usize::try_from(index)
                .ok()
                .and_then(|index| self.0.get(index))
            else {
                return kResultFalse;
            };
            unsafe { *event = *value };
            kResultOk
        }

        unsafe fn addEvent(&self, _event: *mut Event) -> tresult {
            kNotImplemented
        }
    }

    fn note_on(offset: i32) -> Event {
        Event {
            busIndex: 0,
            sampleOffset: offset,
            ppqPosition: 0.0,
            flags: 0,
            r#type: Event_::EventTypes_::kNoteOnEvent as u16,
            __field0: Event__type0 {
                noteOn: NoteOnEvent {
                    channel: 0,
                    pitch: 60,
                    tuning: 0.0,
                    velocity: 1.0,
                    length: -1,
                    noteId: -1,
                },
            },
        }
    }

    fn note_off(offset: i32) -> Event {
        Event {
            busIndex: 0,
            sampleOffset: offset,
            ppqPosition: 0.0,
            flags: 0,
            r#type: Event_::EventTypes_::kNoteOffEvent as u16,
            __field0: Event__type0 {
                noteOff: NoteOffEvent {
                    channel: 0,
                    pitch: 60,
                    velocity: 0.0,
                    noteId: -1,
                    tuning: 0.0,
                },
            },
        }
    }

    #[test]
    fn host_midi_renders_at_event_offset_and_releases_with_a_tail() {
        let shared = Shared::default();
        let owner = AudioOwner::new();
        let events = ComWrapper::new(HostEvents(vec![note_on(16)]));
        let events = events.as_com_ref::<IEventList>().unwrap();
        let mut left = [0.0_f32; 512];
        let mut right = [0.0_f32; 512];
        let mut channels = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut output = AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: channels.as_mut_ptr(),
            },
        };
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: left.len() as i32,
            numInputs: 0,
            numOutputs: 1,
            inputs: std::ptr::null_mut(),
            outputs: &mut output,
            inputParameterChanges: std::ptr::null_mut(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: events.as_ptr(),
            outputEvents: std::ptr::null_mut(),
            processContext: std::ptr::null_mut(),
        };
        assert_eq!(
            unsafe { owner.process(&mut data, &shared, 48_000.0) },
            kResultOk
        );
        assert!(left[..16].iter().all(|sample| *sample == 0.0));
        assert!(left[32..128].iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(shared.held_keys(), [1_u64 << 60, 0]);
        assert_eq!(left, right);

        let release_events = ComWrapper::new(HostEvents(vec![note_off(128)]));
        let release_events = release_events.as_com_ref::<IEventList>().unwrap();
        data.inputEvents = release_events.as_ptr();
        left.fill(0.0);
        right.fill(0.0);
        assert_eq!(
            unsafe { owner.process(&mut data, &shared, 48_000.0) },
            kResultOk
        );
        assert!(left[128..].iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(shared.held_keys(), [0, 0]);
        assert_eq!(left, right);
    }
}
