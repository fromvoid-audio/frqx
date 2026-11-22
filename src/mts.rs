//! Dynamically loaded MTS-ESP master running off the audio thread.

use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::shared::{MasterStatus, Shared};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

/// A single worker that owns the MTS-ESP master registration for this processor.
pub struct MasterWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MasterWorker {
    /// Start a worker that publishes scale edits without calling MTS-ESP on the audio thread.
    pub fn start(shared: Arc<Shared>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || run(shared, stop_for_thread));
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for MasterWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "macos")]
struct MtsApi {
    handle: *mut c_void,
    can_register: Option<unsafe extern "C" fn() -> bool>,
    has_master: Option<unsafe extern "C" fn() -> bool>,
    register: unsafe extern "C" fn(),
    deregister: unsafe extern "C" fn(),
    set_tunings: unsafe extern "C" fn(*const f64),
    get_clients: unsafe extern "C" fn() -> c_int,
    set_name: Option<unsafe extern "C" fn(*const c_char)>,
    set_period: Option<unsafe extern "C" fn(f64)>,
    set_map_size: Option<unsafe extern "C" fn(i8)>,
    set_map_start: Option<unsafe extern "C" fn(i8)>,
    set_ref_key: Option<unsafe extern "C" fn(i8)>,
}

#[cfg(target_os = "macos")]
impl MtsApi {
    fn load() -> Option<Self> {
        // SAFETY: the path is a static C string; a non-null handle remains open
        // until this API object is dropped.
        let handle = unsafe {
            dlopen(
                c"/Library/Application Support/MTS-ESP/libMTS.dylib".as_ptr(),
                2,
            )
        };
        if handle.is_null() {
            return None;
        }
        // SAFETY: symbol names and signatures follow ODDSound's public C API.
        unsafe {
            let can_register = Self::symbol(handle, c"MTS_CanRegisterMaster");
            let has_master = Self::symbol(handle, c"MTS_HasMaster");
            let register = Self::symbol(handle, c"MTS_RegisterMaster");
            let deregister = Self::symbol(handle, c"MTS_DeregisterMaster");
            let set_tunings = Self::symbol(handle, c"MTS_SetNoteTunings");
            let get_clients = Self::symbol(handle, c"MTS_GetNumClients");
            let (Some(register), Some(deregister), Some(set_tunings), Some(get_clients)) =
                (register, deregister, set_tunings, get_clients)
            else {
                dlclose(handle);
                return None;
            };
            if can_register.is_none() && has_master.is_none() {
                dlclose(handle);
                return None;
            }
            Some(Self {
                handle,
                can_register,
                has_master,
                register,
                deregister,
                set_tunings,
                get_clients,
                set_name: Self::symbol(handle, c"MTS_SetScaleName"),
                set_period: Self::symbol(handle, c"MTS_SetPeriodRatio"),
                set_map_size: Self::symbol(handle, c"MTS_SetMapSize"),
                set_map_start: Self::symbol(handle, c"MTS_SetMapStartKey"),
                set_ref_key: Self::symbol(handle, c"MTS_SetRefKey"),
            })
        }
    }

    unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
        let pointer = unsafe { dlsym(handle, name.as_ptr()) };
        if pointer.is_null() {
            None
        } else {
            // SAFETY: each call site specifies the published function signature.
            Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&pointer) })
        }
    }

    fn available(&self) -> bool {
        // Prefer the current API. Older installed libraries offer HasMaster.
        unsafe {
            if let Some(can_register) = self.can_register {
                can_register()
            } else {
                !self.has_master.expect("validated fallback")()
            }
        }
    }

    fn publish(&self, scale: crate::scale::Scale) {
        let frequencies = scale.frequencies();
        // SAFETY: the table remains valid during the synchronous call and all
        // optional metadata functions use the signatures of the master API.
        unsafe {
            (self.set_tunings)(frequencies.as_ptr());
            if let Some(set_name) = self.set_name {
                set_name(c"frqx".as_ptr());
            }
            if let Some(set_period) = self.set_period {
                set_period(2.0_f64.powf(f64::from(scale.period_cents) / 1200.0));
            }
            if let Some(set_map_size) = self.set_map_size {
                set_map_size(scale.count as i8);
            }
            if let Some(set_map_start) = self.set_map_start {
                set_map_start((i32::from(scale.root_key) - scale.root_degree as i32) as i8);
            }
            if let Some(set_ref_key) = self.set_ref_key {
                set_ref_key(scale.root_key as i8);
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MtsApi {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by dlopen and is closed exactly once.
        unsafe { dlclose(self.handle) };
    }
}

#[cfg(target_os = "macos")]
fn run(shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let Some(api) = MtsApi::load() else {
        shared.set_status(MasterStatus::Unavailable);
        return;
    };
    let mut owned = false;
    let mut published_revision = 0;
    while !stop.load(Ordering::Acquire) {
        if !owned {
            if api.available() {
                // SAFETY: registration is performed only when the library says
                // there is no registered master.
                unsafe { (api.register)() };
                owned = true;
                shared.set_status(MasterStatus::Active);
            } else {
                shared.set_status(MasterStatus::Blocked);
                shared.set_clients(0);
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        }
        let revision = shared.revision();
        if revision != published_revision {
            api.publish(shared.snapshot());
            published_revision = revision;
        }
        // SAFETY: the installed function returns a count and owns no caller data.
        shared.set_clients(unsafe { (api.get_clients)() });
        thread::sleep(Duration::from_millis(40));
    }
    if owned {
        // SAFETY: this worker is the instance that registered the master.
        unsafe { (api.deregister)() };
    }
    shared.set_status(MasterStatus::Connecting);
    shared.set_clients(0);
}

#[cfg(not(target_os = "macos"))]
fn run(shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    shared.set_status(MasterStatus::Unavailable);
    while !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(250));
    }
}
