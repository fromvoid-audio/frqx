//! MTS-ESP tuning master with a built-in audition tone.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::mts::MasterWorker;
use crate::scale::{Scale, MAX_DEGREES};
use crate::shared::Shared;
use crate::synth::{
    SynthSettings, Waveform, ATTACK_PARAM, LEVEL_PARAM, RELEASE_PARAM, WAVEFORM_PARAM,
};

mod controller;
mod synth_audio;

use synth_audio::AudioOwner;

const STATE_MAGIC: u32 = u32::from_le_bytes(*b"FREQ");
const STATE_VERSION: u32 = 2;
const STATE_LEN_V1: usize = 3 + 4 + 8 + 16 * 4;
const STATE_LEN: usize = 3 + 4 + 8 + MAX_DEGREES * 4 + 1 + 12;

unsafe fn write_state(stream: *mut IBStream, shared: &Shared) -> tresult {
    let scale = shared.snapshot();
    let mut payload = [0u8; STATE_LEN];
    payload[0] = scale.count as u8;
    payload[1] = scale.root_key;
    payload[2] = scale.root_degree as u8;
    payload[3..7].copy_from_slice(&scale.period_cents.to_le_bytes());
    payload[7..15].copy_from_slice(&scale.root_hz.to_le_bytes());
    for (index, cents) in scale.cents.iter().enumerate() {
        payload[15 + index * 4..19 + index * 4].copy_from_slice(&cents.to_le_bytes());
    }
    let settings = shared.synth_settings();
    let settings_start = 15 + MAX_DEGREES * 4;
    payload[settings_start] = settings.waveform.index();
    payload[settings_start + 1..settings_start + 5].copy_from_slice(&settings.level.to_le_bytes());
    payload[settings_start + 5..settings_start + 9]
        .copy_from_slice(&settings.attack_ms.to_le_bytes());
    payload[settings_start + 9..settings_start + 13]
        .copy_from_slice(&settings.release_ms.to_le_bytes());
    match unsafe { write_versioned_payload(stream, STATE_MAGIC, STATE_VERSION, &payload) } {
        Ok(()) => kResultOk,
        Err(_) => kResultFalse,
    }
}

unsafe fn read_state(stream: *mut IBStream, shared: &Shared) -> tresult {
    let Ok(state) = (unsafe { read_versioned_payload(stream, STATE_MAGIC, &[1, STATE_VERSION]) })
    else {
        return kInvalidArgument;
    };
    let capacity = if state.version == 1 && state.payload.len() == STATE_LEN_V1 {
        16
    } else if state.version == STATE_VERSION && state.payload.len() == STATE_LEN {
        MAX_DEGREES
    } else {
        return kInvalidArgument;
    };
    let payload = &state.payload;
    let mut scale = Scale::equal(7);
    scale.count = payload[0] as usize;
    scale.root_key = payload[1];
    scale.root_degree = payload[2] as usize;
    scale.period_cents = f32::from_le_bytes(payload[3..7].try_into().expect("fixed state width"));
    scale.root_hz = f64::from_le_bytes(payload[7..15].try_into().expect("fixed state width"));
    for (index, cents) in scale.cents.iter_mut().take(capacity).enumerate() {
        *cents = f32::from_le_bytes(
            payload[15 + index * 4..19 + index * 4]
                .try_into()
                .expect("fixed state width"),
        );
    }
    let settings = if state.version == 1 {
        SynthSettings::default()
    } else {
        let start = 15 + MAX_DEGREES * 4;
        if payload[start] > 2 {
            return kInvalidArgument;
        }
        SynthSettings {
            waveform: Waveform::from_index(payload[start]),
            level: f32::from_le_bytes(
                payload[start + 1..start + 5]
                    .try_into()
                    .expect("fixed state width"),
            ),
            attack_ms: f32::from_le_bytes(
                payload[start + 5..start + 9]
                    .try_into()
                    .expect("fixed state width"),
            ),
            release_ms: f32::from_le_bytes(
                payload[start + 9..start + 13]
                    .try_into()
                    .expect("fixed state width"),
            ),
        }
    };
    if !settings.valid() || !scale.valid() {
        return kInvalidArgument;
    }
    if shared.replace(scale) && shared.set_synth_settings(settings) {
        kResultOk
    } else {
        kInvalidArgument
    }
}

/// VST3 audio processor class.
pub(crate) struct Processor {
    connection: InstanceConnection<Shared>,
    shared: Arc<Shared>,
    _worker: MasterWorker,
    audio: AudioOwner,
    sample_rate: AtomicU64,
}

impl Processor {
    /// Stable class identifier for the generated processor.
    pub(crate) const CID: TUID = uid(0x2CFB7EF2, 0x652346FB, 0xA76DA2AD, 0x2DD6CAB6);

    fn new() -> Self {
        let shared = Arc::new(Shared::default());
        let worker = MasterWorker::start(Arc::clone(&shared));
        Self {
            connection: InstanceConnection::new(
                InstanceConnectionRole::Processor,
                Arc::clone(&shared),
            ),
            shared,
            _worker: worker,
            audio: AudioOwner::new(),
            sample_rate: AtomicU64::new(48_000.0_f64.to_bits()),
        }
    }
}

impl Class for Processor {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IConnectionPoint,
        IToyboxSharedState,
    );
}

toybox::impl_vst3_instance_connection!(Processor, connection);

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if class_id.is_null() {
            return kInvalidArgument;
        }
        unsafe { *class_id = controller::Controller::CID };
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match (media_type as MediaTypes, dir as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput)
            | (MediaTypes_::kEvent, BusDirections_::kInput) => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if bus.is_null() || index != 0 {
            return kInvalidArgument;
        }
        let (label, channels) = match (media_type as MediaTypes, dir as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => ("Audio Output", 2),
            (MediaTypes_::kEvent, BusDirections_::kInput) => ("MIDI Input", 1),
            _ => return kInvalidArgument,
        };

        // SAFETY: the pointer is validated above.
        let bus = unsafe { &mut *bus };
        bus.mediaType = media_type;
        bus.direction = dir;
        bus.channelCount = channels;
        copy_wstring(label, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive;
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media_type: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        unsafe { read_state(state, &self.shared) }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { write_state(state, &self.shared) }
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins != 0 || num_outs != 1 || outputs.is_null() {
            return kResultFalse;
        }
        let _ = inputs;
        unsafe { *outputs = SpeakerArr::kStereo };
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if arr.is_null() || index != 0 {
            return kInvalidArgument;
        }
        match dir as BusDirections {
            BusDirections_::kOutput => {
                // SAFETY: the pointer is validated above.
                unsafe { *arr = SpeakerArr::kStereo };
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn canProcessSampleSize(&self, symbolic_sample_size: i32) -> tresult {
        match symbolic_sample_size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => kResultOk,
            SymbolicSampleSizes_::kSample64 => kNotImplemented,
            _ => kInvalidArgument,
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() {
            return kInvalidArgument;
        }
        let rate = unsafe { (*setup).sampleRate };
        if !rate.is_finite() || rate < 8_000.0 {
            return kInvalidArgument;
        }
        self.sample_rate.store(rate.to_bits(), Ordering::Release);
        kResultOk
    }

    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        if state == 0 {
            self.audio.request_reset();
            self.shared.set_held_keys([0; 2]);
        }
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }

        // SAFETY: the host owns and keeps this structure valid for the call.
        let process_data = unsafe { &mut *data };
        if process_data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32 {
            return kNotImplemented;
        }
        for id in [WAVEFORM_PARAM, LEVEL_PARAM, ATTACK_PARAM, RELEASE_PARAM] {
            if let Some((_, normalized)) =
                unsafe { latest_param_point(process_data.inputParameterChanges, id) }
            {
                self.shared.set_synth_param(id, normalized);
            }
        }
        let rate = f64::from_bits(self.sample_rate.load(Ordering::Acquire));
        unsafe { self.audio.process(process_data, &self.shared, rate) }
    }

    unsafe fn getTailSamples(&self) -> u32 {
        let rate = f64::from_bits(self.sample_rate.load(Ordering::Acquire));
        (rate * 3.1).ceil().min(f64::from(u32::MAX)) as u32
    }
}

/// Factory exposed to VST3 hosts.
#[derive(Default)]
pub(crate) struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the pointer is validated above.
        let info = unsafe { &mut *info };
        copy_cstring("PORTALSURFER", &mut info.vendor);
        copy_cstring("https://portalsurfer.org", &mut info.url);
        copy_cstring("support@portalsurfer.org", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        2
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if info.is_null() || !(0..=1).contains(&index) {
            return kInvalidArgument;
        }
        // SAFETY: the pointer is validated above.
        let info = unsafe { &mut *info };
        if index == 0 {
            write_class_info_many(info, Processor::CID, CATEGORY_AUDIO_MODULE_CLASS, "freqs");
        } else {
            write_class_info_many(
                info,
                controller::Controller::CID,
                CATEGORY_COMPONENT_CONTROLLER_CLASS,
                "freqs Controller",
            );
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }

        // SAFETY: the host supplies a pointer to a VST3 class identifier.
        let class_id = unsafe { *(cid as *const TUID) };
        let instance = if class_id == Processor::CID {
            ComWrapper::new(Processor::new()).to_com_ptr::<FUnknown>()
        } else if class_id == controller::Controller::CID {
            ComWrapper::new(controller::Controller::new()).to_com_ptr::<FUnknown>()
        } else {
            return kInvalidArgument;
        };
        let Some(instance) = instance else {
            return kResultFalse;
        };
        let ptr = instance.as_ptr();
        // SAFETY: the host supplies the requested interface and output pointer.
        unsafe { ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let (cid, category, subcategories, name) = match index {
            0 => (
                Processor::CID,
                CATEGORY_AUDIO_MODULE_CLASS,
                "Instrument",
                "freqs",
            ),
            1 => (
                controller::Controller::CID,
                CATEGORY_COMPONENT_CONTROLLER_CLASS,
                "Instrument",
                "freqs Controller",
            ),
            _ => return kInvalidArgument,
        };
        let info = unsafe { &mut *info };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(name, &mut info.name);
        info.classFlags = 0;
        copy_cstring(subcategories, &mut info.subCategories);
        copy_cstring("PORTALSURFER", &mut info.vendor);
        copy_cstring(env!("CARGO_PKG_VERSION"), &mut info.version);
        copy_cstring("VST 3", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vst3_reports_an_instrument_with_midi_input_and_stereo_output() {
        let factory = Factory;
        let mut class_info = unsafe { std::mem::zeroed::<PClassInfo2>() };
        assert_eq!(
            unsafe { factory.getClassInfo2(0, &mut class_info) },
            kResultOk
        );
        let subcategories = class_info
            .subCategories
            .iter()
            .map(|byte| *byte as u8)
            .take_while(|byte| *byte != 0)
            .collect::<Vec<_>>();
        assert_eq!(subcategories, b"Instrument");

        let processor = Processor::new();
        assert_eq!(
            unsafe {
                processor.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32)
            },
            0
        );
        assert_eq!(
            unsafe {
                processor.getBusCount(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32)
            },
            1
        );
        assert_eq!(
            unsafe {
                processor.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32)
            },
            1
        );
        let mut bus = unsafe { std::mem::zeroed::<BusInfo>() };
        assert_eq!(
            unsafe {
                processor.getBusInfo(
                    MediaTypes_::kEvent as i32,
                    BusDirections_::kInput as i32,
                    0,
                    &mut bus,
                )
            },
            kResultOk
        );
        assert_eq!(bus.mediaType, MediaTypes_::kEvent as i32);
        let mut output = SpeakerArr::kEmpty;
        assert_eq!(
            unsafe { processor.setBusArrangements(std::ptr::null_mut(), 0, &mut output, 1) },
            kResultTrue
        );
        assert_eq!(output, SpeakerArr::kStereo);
    }
}
