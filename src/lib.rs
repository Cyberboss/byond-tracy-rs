#![feature(once_cell_try)]
mod byond;

use crate::byond::{BuildNumber, ByondReflectionData, DreamObject, Proc, offsets::OFFSETS};
#[cfg(not(target_os = "windows"))]
use libloading::os::unix::{Library, RTLD_NOW};
#[cfg(target_os = "windows")]
use libloading::os::windows::Library;
use std::{
    cell::RefCell,
    ffi::{CString, c_char, c_int},
    ptr::null,
    sync::OnceLock,
};
use tracy_client::{Client, SpanLocation};

#[cfg(not(target_pointer_width = "32"))]
compile_error!("Compiling for non-32bit is not allowed.");

static EMPTY_STRING: c_char = 0;
thread_local! {
    static RETURN_STRING: RefCell<CString> = RefCell::new(CString::default());
}

static INSTANCE: OnceLock<Instance> = OnceLock::new();

static SERVER_TICK_SOURCE_LOCATION: SpanLocation = todo!();

static SEND_MAPS_SOURCE_LOCATION: SpanLocation = todo!();

struct Instance {
    pub byond: ByondReflectionData,
    tracy_client: Client,
}

impl Instance {
    fn tracy_client(&self) -> Client {
        self.tracy_client.clone()
    }
}

/// SAFETY: This function must only be called via the call()() or call_ext()() procs using the legacy API of a game running using Build Your Own Net Dream (BYOND, https://www.byond.com/).
/// It relies on reverse engineered internals of the game runtime
#[unsafe(no_mangle)]
pub unsafe extern "C" fn init(_argc: c_int, _argv: *const *const c_char) -> *const c_char {
    init_core()
}

fn init_core() -> *const c_char {
    let mut initialize_attempted = false;
    match INSTANCE.get_or_try_init(|| {
        initialize_attempted = true;
        setup()
    }) {
        Ok(_) => if initialize_attempted {
            "ok"
        } else {
            "already initialized"
        }
        .as_ptr() as *const c_char,
        Err(error) => error,
    }
}

fn setup() -> Result<Instance, *const c_char> {
    let (byond_build, byondcore_base_address) = match get_byond_build_and_byondcore_handle() {
        Ok(byond_build) => byond_build,
        Err(error) => {
            return Err(if error.is_empty() {
                &EMPTY_STRING
            } else {
                RETURN_STRING.with(|cell| {
                    // Panicking over an FFI boundary is bad form, so if a NUL ends up
                    // in the result, just truncate.
                    let cstring = match CString::new(error) {
                        Ok(s) => s,
                        Err(e) => {
                            let (pos, mut vec) = (e.nul_position(), e.into_vec());
                            vec.truncate(pos);
                            CString::new(vec).unwrap_or_default()
                        }
                    };
                    cell.replace(cstring);
                    cell.borrow().as_ptr()
                })
            });
        }
    };

    let mut target_offsets = None;
    for offsets in OFFSETS {
        if offsets.byond_build == byond_build {
            target_offsets = Some(offsets)
        }
    }

    let offsets = match target_offsets {
        Some(offsets) => offsets,
        None => return Err("byond version unsupported".as_ptr() as *const c_char),
    };

    Ok(Instance {
        byond: match ByondReflectionData::create_and_initialize_hooks(
            offsets,
            byondcore_base_address,
            exec_proc_hook,
            server_tick_hook,
            send_maps_hook,
        ) {
            Ok(data) => data,
            Err(error) => return Err(error.as_ptr() as *const c_char),
        },
        tracy_client: Client::start(),
    })
}

fn get_byond_build_and_byondcore_handle() -> Result<(BuildNumber, usize), String> {
    let byondcore_handle = get_byondcore_handle()?;

    let get_byond_build_name = "?GetByondBuild@ByondLib@@QAEJXZ";
    // SAFETY: The symbol specified using get_byond_build_name demangles to the following C++ declaration:
    // public: long __thiscall ByondLib::GetByondBuild(void)
    // Reverse engineering shows the "this" pointer is not used in this function
    let get_byond_build_pointer_result =
        unsafe { byondcore_handle.get(get_byond_build_name.as_bytes()) };

    let get_byond_build: unsafe fn() -> i32 = match get_byond_build_pointer_result {
        Ok(pointer) => *pointer,
        Err(error) => {
            return Err(format!(
                "Unable to find symbol {} in byondcore.dll: {}",
                get_byond_build_name, error
            ));
        }
    };

    // SAFETY: GetByondBuild() is essentially a static const function
    let build_number = unsafe { get_byond_build() };

    Ok((build_number, byondcore_handle.into_raw() as usize))
}

#[cfg(target_os = "windows")]
fn get_byondcore_handle() -> Result<Library, String> {
    let byond_dll_name = "byondcore.dll";

    let handle_acquisition_result = Library::open_already_loaded(byond_dll_name);

    match handle_acquisition_result {
        Ok(handle) => Ok(handle.into()),
        Err(error) => Err(format!(
            "Unable to find {} handle: {}",
            byond_dll_name, error,
        )),
    }
}

#[cfg(not(target_os = "windows"))]
fn get_byondcore_handle() -> Result<Library, String> {
    let byond_so_name = "libbyond.so";

    // From consts.rs in libloading:
    // Other constants that exist but are not bound because they are platform-specific (non-posix)
    // extensions. Some of these constants are only relevant to `dlsym` or `dlmopen` calls.
    //
    // This is the value for Linux
    const RTLD_NOLOAD: c_int = 0x00004;
    let handle_acquisition_result =
        unsafe { Library::open(Some(byond_so_name), RTLD_NOW | RTLD_NOLOAD) };

    match handle_acquisition_result {
        Ok(handle) => Ok(handle.into()),
        Err(error) => Err(format!(
            "Unable to find {} address: {}",
            byond_so_name, error,
        )),
    }
}

#[cfg(target_os = "windows")]
unsafe extern "C" fn exec_proc_hook(proc: *const Proc) -> DreamObject {
    exec_proc_hook_core(proc)
}

#[cfg(not(target_os = "windows"))]
unsafe extern "regparm(3)" fn exec_proc_hook(proc: *const Proc) -> DreamObject {
    exec_proc_hook_core(proc)
}

#[inline(always)]
fn exec_proc_hook_core(proc: *const Proc) -> DreamObject {
    let instance_ref = INSTANCE
        .get()
        .expect("(exec_proc_hook) Hook installed but OnceLock empty!");
    let orig_exec_proc = instance_ref.byond.orig_exec_proc;
    let proc_ref: &Proc = unsafe { &*proc };
    if proc_ref.procdef < 0x14000 {
        let srcloc = todo!("get source loc");
        let zone = instance_ref.tracy_client().span(srcloc, 0);

        // procs with pre-existing contexts are resuming from sleep
        if proc_ref.context != null() {
            zone.emit_color(0xAF4444);
        }

        let return_value = unsafe { orig_exec_proc(proc) };

        drop(zone);

        return_value
    } else {
        unsafe { orig_exec_proc(proc) }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "stdcall" fn server_tick_hook() -> i32 {
    server_tick_hook_core()
}

#[cfg(not(target_os = "windows"))]
unsafe extern "C" fn server_tick_hook() -> i32 {
    server_tick_hook_core()
}

#[inline(always)]
fn server_tick_hook_core() -> i32 {
    let instance_ref = INSTANCE
        .get()
        .expect("(exec_proc_hook) Hook installed but OnceLock empty!");
    let orig_server_tick = instance_ref.byond.orig_server_tick;

    let tracy_client = instance_ref.tracy_client();

    tracy_client.frame_mark();

    let zone = tracy_client.span(&SERVER_TICK_SOURCE_LOCATION, 0);

    let interval = unsafe { orig_server_tick() };

    drop(zone);

    interval
}

unsafe extern "C" fn send_maps_hook() {
    let instance_ref = INSTANCE
        .get()
        .expect("(exec_proc_hook) Hook installed but OnceLock empty!");
    let orig_send_maps = instance_ref.byond.orig_send_maps;

    let zone = instance_ref
        .tracy_client()
        .span(&SEND_MAPS_SOURCE_LOCATION, 0);

    unsafe { orig_send_maps() };

    drop(zone);
}
