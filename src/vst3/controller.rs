//! VST3 edit controller hosting the native GPUI scale editor.

use std::ffi::CStr;
use std::ptr;
use std::slice;
use std::sync::Arc;

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use super::{read_state, write_state};
use crate::shared::Shared;
use crate::synth::{Waveform, ATTACK_PARAM, LEVEL_PARAM, RELEASE_PARAM, WAVEFORM_PARAM};

/// Instrument controller with four host-visible audition parameters.
pub(super) struct Controller {
    connection: InstanceConnection<Shared>,
}

impl Controller {
    pub(super) const CID: TUID = uid(0x3D36BA2B, 0xB2264D09, 0x8F6E3C99, 0x9BBF279E);

    pub(super) fn new() -> Self {
        Self {
            connection: InstanceConnection::new(
                InstanceConnectionRole::Controller,
                Arc::new(Shared::default()),
            ),
        }
    }
}

impl Class for Controller {
    type Interfaces = (IEditController, IConnectionPoint, IToyboxSharedState);
}

toybox::impl_vst3_instance_connection!(Controller, connection);

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        unsafe { read_state(state, self.connection.shared().as_ref()) }
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.setComponentState(state) }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { write_state(state, self.connection.shared().as_ref()) }
    }

    unsafe fn getParameterCount(&self) -> int32 {
        4
    }

    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let (id, title, unit, steps, default) = match index {
            0 => (WAVEFORM_PARAM, "Waveform", "", 2, 0.0),
            1 => (LEVEL_PARAM, "Level", "%", 0, 0.25),
            2 => (ATTACK_PARAM, "Attack", "ms", 0, 8.0 / 498.0),
            3 => (RELEASE_PARAM, "Release", "ms", 0, 800.0 / 2900.0),
            _ => return kInvalidArgument,
        };
        let info = unsafe { &mut *info };
        info.id = id;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring(unit, &mut info.units);
        info.stepCount = steps;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }

    unsafe fn getParamStringByValue(
        &self,
        id: ParamID,
        normalized: ParamValue,
        string: *mut String128,
    ) -> tresult {
        if string.is_null() || !normalized.is_finite() {
            return kInvalidArgument;
        }
        let display = match id {
            WAVEFORM_PARAM => {
                Waveform::from_index((normalized.clamp(0.0, 1.0) * 2.0).round() as u8)
                    .name()
                    .to_owned()
            }
            LEVEL_PARAM => format!("{:.0}%", normalized.clamp(0.0, 1.0) * 100.0),
            ATTACK_PARAM | RELEASE_PARAM => format!("{:.0} ms", unsafe {
                self.normalizedParamToPlain(id, normalized)
            }),
            _ => return kInvalidArgument,
        };
        copy_wstring(&display, unsafe { &mut *string });
        kResultOk
    }

    unsafe fn getParamValueByString(
        &self,
        id: ParamID,
        string: *mut TChar,
        normalized: *mut ParamValue,
    ) -> tresult {
        if string.is_null() || normalized.is_null() {
            return kInvalidArgument;
        }
        let length = unsafe { tchar_len(string) };
        let utf16 = unsafe { slice::from_raw_parts(string.cast::<u16>(), length) };
        let Ok(raw) = String::from_utf16(utf16) else {
            return kInvalidArgument;
        };
        let trimmed = raw.trim().to_ascii_lowercase();
        let value = if id == WAVEFORM_PARAM {
            match trimmed.as_str() {
                "triangle" => 0.0,
                "square" => 0.5,
                "saw" => 1.0,
                _ => return kInvalidArgument,
            }
        } else {
            let plain = if id == LEVEL_PARAM {
                trimmed.trim_end_matches('%').trim()
            } else if id == ATTACK_PARAM || id == RELEASE_PARAM {
                trimmed.trim_end_matches("ms").trim()
            } else {
                return kInvalidArgument;
            };
            let Ok(plain) = plain.parse::<f64>() else {
                return kInvalidArgument;
            };
            unsafe { self.plainParamToNormalized(id, plain) }
        };
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return kInvalidArgument;
        }
        unsafe { *normalized = value };
        kResultOk
    }

    unsafe fn normalizedParamToPlain(&self, id: ParamID, value: ParamValue) -> ParamValue {
        let value = value.clamp(0.0, 1.0);
        match id {
            WAVEFORM_PARAM => (value * 2.0).round(),
            LEVEL_PARAM => value * 100.0,
            ATTACK_PARAM => 2.0 + value * 498.0,
            RELEASE_PARAM => 100.0 + value * 2900.0,
            _ => 0.0,
        }
    }

    unsafe fn plainParamToNormalized(&self, id: ParamID, value: ParamValue) -> ParamValue {
        match id {
            WAVEFORM_PARAM => (value / 2.0).clamp(0.0, 1.0),
            LEVEL_PARAM => (value / 100.0).clamp(0.0, 1.0),
            ATTACK_PARAM => ((value - 2.0) / 498.0).clamp(0.0, 1.0),
            RELEASE_PARAM => ((value - 100.0) / 2900.0).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        self.connection.shared().synth_param(id).unwrap_or(0.0)
    }

    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        if self.connection.shared().set_synth_param(id, value) {
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn setComponentHandler(&self, _: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if name.is_null() {
            return ptr::null_mut();
        }
        let requested = unsafe { CStr::from_ptr(name) };
        let editor = unsafe { CStr::from_ptr(ViewType::kEditor) };
        if requested.to_bytes() != editor.to_bytes() {
            return ptr::null_mut();
        }
        #[cfg(target_os = "macos")]
        {
            let gui = crate::gui_gpui::new_vst3_gui(self.connection.shared());
            let view = create_gpui_view(
                gui,
                crate::gui_gpui::WIDTH,
                crate::gui_gpui::HEIGHT,
                (crate::gui_gpui::WIDTH, crate::gui_gpui::HEIGHT),
                (crate::gui_gpui::WIDTH, crate::gui_gpui::HEIGHT),
            );
            let Some(view) = ComWrapper::new(view).to_com_ptr::<IPlugView>() else {
                return ptr::null_mut();
            };
            ComPtr::into_raw(view)
        }
        #[cfg(not(target_os = "macos"))]
        ptr::null_mut()
    }
}
