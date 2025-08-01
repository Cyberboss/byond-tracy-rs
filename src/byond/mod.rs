use std::{
    array::from_fn,
    ffi::{CStr, c_char},
    mem::transmute,
    ptr::null,
};

use crate::byond::offsets::Offsets;

use tracy_client::{SpanLocation, internal::make_span_location};

pub(crate) mod offsets;

pub(crate) const MAX_PROCS: usize = 0x14000;

pub(crate) type BuildNumber = i32;

#[cfg(target_os = "windows")]
pub(crate) type ExecProcFunction = unsafe extern "cdecl" fn(*const Proc) -> DreamObject;

#[cfg(not(target_os = "windows"))]
pub(crate) type ExecProcFunction = unsafe extern "regparm3" fn(*const Proc) -> DreamObject;

#[cfg(target_os = "windows")]
pub(crate) type ServerTickFunction = unsafe extern "stdcall" fn() -> i32;

#[cfg(not(target_os = "windows"))]
pub(crate) type ServerTickFunction = unsafe extern "cdecl" fn() -> i32;

pub(crate) type SendMapsFunction = unsafe extern "cdecl" fn();

type DreamStringId = u32;

#[repr(C)]
union ObjectPart1 {
    padding: u32,
    object_type: u8,
}

#[repr(C)]
union ObjectPart2 {
    i: u32,
    f: f32,
}

#[repr(C)]
pub(crate) struct DreamObject {
    part_1: ObjectPart1,
    part_2: ObjectPart2,
}

#[repr(C)]
struct DreamString {
    data: *const c_char,
    id: DreamStringId,
    left: *const DreamString,
    right: *const DreamString,
    refcount: u32,
    unknown_0: u32,
    length: u32,
}

#[repr(C)]
struct ProcDefinition {
    path: DreamStringId,
    name: u32,
    desc: u32,
    category: u32,
    flags: u32,
    _unknown0: u32,
    bytecode: u32,
    locals: u32,
    parameters: u32,
}

#[repr(C)]
struct Bytecode {
    length: u16,
    unknown_0: u32,
    bytecode: *const u32,
}

#[repr(C)]
struct Locals {
    length: u16,
    unknown_0: u32,
    locals: *const u32,
}

#[repr(C)]
struct Params {
    length: u16,
    unknown_0: u32,
    params: *const u32,
}

#[repr(C)]
struct Misc {
    bytecode: Bytecode,
    locals: Locals,
    params: Params,
}

#[repr(C)]
struct ExecutionContext;

#[repr(C)]
pub(crate) struct Proc {
    pub procdef: usize,
    flags: u8,
    supers: u8,
    unused: u16,
    usr: DreamObject,
    src: DreamObject,
    pub context: *const ExecutionContext,
    sequence: u32,
    callback: fn(DreamObject, u32) -> (),
    callback_arg: u32,
    argc: u32,
    argv: *const [DreamObject],
    unknown_0: u32,
}

#[repr(C)]
struct ProcDefsDescriptor {
    size: usize,
    path_offset: usize,
    bytecode_offset: usize,
}

#[repr(C)]
#[derive(Default)]
struct Trampoline {
    exec_proc: [u8; 32],
    server_tick: [u8; 32],
    send_maps: [u8; 32],
}

struct ProcdefPointer(usize);

pub(crate) struct ByondReflectionData {
    strings_base_address: *const DreamString,
    strings_len: *const usize,
    miscs_base_address: *const Misc,
    miscs_len: *const usize,
    procdefs_base_address: usize,
    procdefs_len: *const usize,
    procdef_desc: ProcDefsDescriptor,
    exec_proc_address: usize,
    pub orig_exec_proc: ExecProcFunction,
    server_tick_address: usize,
    pub orig_server_tick: ServerTickFunction,
    send_maps_address: usize,
    pub orig_send_maps: SendMapsFunction,
    // TODO: _Alignas(4096)
    trampoline: Trampoline,
}

pub struct SourceLocations {}

impl ByondReflectionData {
    pub fn create_and_initialize_hooks(
        offsets: &Offsets,
        byondcore_base_address: usize,
        exec_proc_hook: ExecProcFunction,
        server_tick_hook: ServerTickFunction,
        send_maps_hook: SendMapsFunction,
    ) -> Result<Self, String> {
        // SAFETY: Provided offsets should have been verified to be the offsets of the BYOND internals we're looking for
        unsafe {
            let exec_proc_address = byondcore_base_address + offsets.exec_proc;
            let server_tick_address = byondcore_base_address + offsets.server_tick;
            let send_maps_address = byondcore_base_address + offsets.send_maps;

            let trampoline = Trampoline::default();

            Ok(Self {
                strings_base_address: transmute(byondcore_base_address + offsets.strings),
                strings_len: transmute(byondcore_base_address + offsets.strings_len),
                miscs_base_address: transmute(byondcore_base_address + offsets.miscs),
                miscs_len: transmute(byondcore_base_address + offsets.miscs_len),
                procdefs_base_address: byondcore_base_address + offsets.procdefs,
                procdefs_len: transmute(byondcore_base_address + offsets.procdefs_len),
                procdef_desc: ProcDefsDescriptor {
                    size: (offsets.procdefs_descriptor >> 0) & 0xFF,
                    path_offset: (offsets.procdefs_descriptor >> 8) & 0xFF,
                    bytecode_offset: (offsets.procdefs_descriptor >> 16) & 0xFF,
                },
                exec_proc_address,
                server_tick_address,
                send_maps_address,
                trampoline,
                orig_exec_proc: transmute(hook::<ExecProcFunction>(
                    exec_proc_hook,
                    transmute(exec_proc_address),
                    offsets.prologue >> 8,
                    &mut trampoline.exec_proc,
                    "exec_proc",
                )?),
                orig_server_tick: transmute(hook::<ServerTickFunction>(
                    server_tick_hook,
                    transmute(server_tick_address),
                    offsets.prologue >> 8,
                    &mut trampoline.server_tick,
                    "server_tick",
                )?),
                orig_send_maps: transmute(hook::<SendMapsFunction>(
                    send_maps_hook,
                    transmute(send_maps_address),
                    offsets.prologue >> 16,
                    &mut trampoline.send_maps,
                    "send_maps",
                )?),
            })
        }
    }

    pub fn build_source_locations(&self) -> [SpanLocation; MAX_PROCS] {
        from_fn(|i| {
            let mut name: *const u8 = null();
            let mut function = "<?>";
            let mut file = "<?.dm>".as_ptr();
            let mut line = 0xFFFFFFFF;

            if let Some(procdef) = self.get_procdef(i) {
                let str = self.get_string_from_id(procdef.path_string_id());

                if !str.is_null() {
                    let str_ref = unsafe { *str };
                    if !str_ref.data.is_null() {
                        let c_str = unsafe { CStr::from_ptr(str_ref.data) };
                        function = c_str.to_str().expect("Failed to get proc name!");
                    }

                    todo!()
                }
            }

            // TODO: Colour
            make_span_location(function, name, file, line)
        })
    }

    fn get_procdef(&self, index: usize) -> Option<ProcdefPointer> {
        todo!()
    }

    fn get_string_from_id(&self, string_id: u32) -> *const DreamString {
        todo!()
    }
}

impl ProcdefPointer {
    fn path_string_id(&self) -> DreamStringId {}
}

// SAFETY: Pointers are read only and accessed in a manner with correct ownership from the BYOND runtime
unsafe impl Send for ByondReflectionData {}

// SAFETY: Pointers are read only and accessed in a manner with correct ownership from the BYOND runtime
unsafe impl Sync for ByondReflectionData {}

// SAFETY:
// - hook_fn and original_fn must be two different function pointers with identical calling conventions, parameters, and return types
// - size must be the number of bytes to overwrite in the function's prologue to safely hook it
// - trampoline's memory location must be pinned
unsafe fn hook<T>(
    hook_fn: *const T,
    original_fn: *const T,
    size: usize,
    trampoline: &mut [u8; 32],
    hook_name: &str,
) -> Result<*const T, String> {
    let jmp = [0xE9, 0x00, 0x00, 0x00, 0x00];

    let trampoline_ptr: *mut [u8; 32] = trampoline;

    // SAFETY: A pointer has an equivalent bit layout to a usize
    let trampoline_address: usize = unsafe { transmute(trampoline_ptr) };
    let og_function_address: usize;
    let hook_fn_address: usize;

    // SAFETY: Based on input safety requirements, these are function pointers
    unsafe {
        og_function_address = transmute(original_fn);
        hook_fn_address = transmute(hook_fn);
    }

    let trampoline_jmp_from = trampoline_address + size + jmp.len();
    let trampoline_jmp_to = og_function_address + size;
    let trampoline_offset = trampoline_jmp_to - trampoline_jmp_from;

    todo!("Memcpy 1");

    let jmp_from = og_function_address + jmp.len();
    let jmp_to = hook_fn_address;
    let offset = jmp_to - jmp_from;

    let old_protection = unprotect_address(og_function_address, size)?;

    todo!("Memcpy 2");

    if size > jmp.len() {
        for i in 0..(size - jmp.len()) {
            let nop: u8 = 0x90;
            todo!("Memcpy 3");
        }
    }

    reprotect_address(og_function_address, size, old_protection).expect(
        format!(
            "Could not reprotect address of hooked function: {}",
            hook_name
        )
        .as_str(),
    );

    Ok(unsafe { transmute(trampoline_address) })
}

struct ProtectionFlags {
    // TODO
}

fn unprotect_address(address: usize, size: usize) -> Result<ProtectionFlags, String> {
    todo!()
}

fn reprotect_address(address: usize, size: usize, flags: ProtectionFlags) -> Result<(), String> {
    todo!()
}
