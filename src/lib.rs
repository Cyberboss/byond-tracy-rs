#![feature(once_cell_try)]
#![feature(fn_align)]
mod byond;

use std::{
    cell::RefCell,
    ffi::{CString, c_char, c_int},
    sync::OnceLock,
};

use crate::byond::{
    BuildNumber,
    offsets::{OFFSETS, Offsets},
};
use tracy_client::Client;

#[cfg(not(target_pointer_width = "32"))]
compile_error!("Compiling for non-32bit is not allowed.");

struct Instance {
    offsets: &'static Offsets,
    tracy_client: Client,
}

static EMPTY_STRING: c_char = 0;
thread_local! {
    static RETURN_STRING: RefCell<CString> = RefCell::new(CString::default());
}

static INSTANCE: OnceLock<Instance> = OnceLock::new();

/// SAFETY: This function must only be called via the call()() or call_ext()() procs using the legacy API of a game running using Build Your Own Net Dream (BYOND, https://www.byond.com/).
/// It relies on reverse engineered internals of the game runtime
#[unsafe(no_mangle)]
pub unsafe extern "C" fn init(_argc: c_int, _argv: *const *const c_char) -> *const c_char {
    init_core()
}

fn byond_return(value: Option<String>) -> *const c_char {
    match value {
        None => &EMPTY_STRING,
        Some(result) if result.is_empty() => &EMPTY_STRING,
        Some(result) => RETURN_STRING.with(|cell| {
            // Panicking over an FFI boundary is bad form, so if a NUL ends up
            // in the result, just truncate.
            let cstring = match CString::new(result) {
                Ok(s) => s,
                Err(e) => {
                    let (pos, mut vec) = (e.nul_position(), e.into_vec());
                    vec.truncate(pos);
                    CString::new(vec).unwrap_or_default()
                }
            };
            cell.replace(cstring);
            cell.borrow().as_ptr()
        }),
    }
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
    let (byond_build, byondcore_handle) = match get_byond_build_and_byondcore_handle() {
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
        offsets,
        tracy_client: Client::start(),
    })
}

#[cfg(target_os = "windows")]
fn get_byond_build_and_byondcore_handle() -> Result<(BuildNumber, usize), String> {
    #[cfg(target_os = "windows")]
    use windows::Win32::Foundation::HMODULE;
    use windows::{
        Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
        core::PCSTR,
    };

    let byond_dll_name = "byondcore.dll";
    let byond_dll_cstr = CString::new(byond_dll_name).expect("Why isn't this parsing?");
    let byond_dll_name_pcstr = PCSTR(byond_dll_cstr.as_bytes().as_ptr());

    // SAFETY: https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandlea
    // We rely on BYOND to not unload the .dll
    let library_load_result = unsafe { GetModuleHandleA(byond_dll_name_pcstr) };

    let byondcore_handle = match library_load_result {
        Ok(handle) => handle,
        Err(error) => {
            return Err(format!(
                "Unable to find {} handle: {}",
                byond_dll_name,
                error.message(),
            ));
        }
    };

    let get_byond_build_name = "?GetByondBuild@ByondLib@@QAEJXZ";
    let get_byond_build_cstr = CString::new(get_byond_build_name).expect("Why isn't this parsing?");
    let get_byond_build_pcstr = PCSTR(get_byond_build_cstr.as_bytes().as_ptr());

    // SAFETY: https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getprocaddress
    let get_byond_build_pointer_result =
        unsafe { GetProcAddress(byondcore_handle, get_byond_build_pcstr) };

    let get_byond_build_farproc_pointer = match get_byond_build_pointer_result {
        Some(pointer) => pointer,
        None => {
            return Err(format!(
                "Unable to find symbol {} in byondcore.dll!",
                get_byond_build_name
            ));
        }
    };

    // SAFETY: The symbol specified using get_byond_build_name demangles to the following C++ declaration:
    // public: long __thiscall ByondLib::GetByondBuild(void)
    // Reverse engineering shows the "this" pointer is not used in this function
    unsafe {
        let get_byond_build: unsafe fn() -> i32 =
            std::mem::transmute(get_byond_build_farproc_pointer);
        Ok((get_byond_build(), byondcore_handle.0 as usize))
    }
}

fn get_byond_build2() -> Result<BuildNumber, String> {
    todo!()
}
