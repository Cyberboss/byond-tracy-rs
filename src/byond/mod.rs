use std::mem::transmute;

use crate::byond::offsets::Offsets;

pub mod offsets;

pub type BuildNumber = i32;

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
struct DMObject {
    part_1: ObjectPart1,
    part_2: ObjectPart2,
}

#[repr(C)]
struct String {
    data: *const u8,
    id: u32,
    left: *const String,
    right: *const String,
    refcount: u32,
    unknown_0: u32,
    length: u32,
}

#[repr(C)]
struct ProcDefinition {
    path: u32,
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
struct Proc {
    definition: u32,
    flags: u8,
    supers: u8,
    unused: u16,
    usr: DMObject,
    src: DMObject,
    context: *const ExecutionContext,
    sequence: u32,
    callback: fn(DMObject, u32) -> (),
    callback_arg: u32,
    argc: u32,
    argv: *const [DMObject],
    unknown_0: u32,
}

#[repr(C)]
struct ProcDefsDescriptor {
    size: usize,
    path_offset: usize,
    bytecode_offset: usize,
}

#[repr(C)]
struct Trampoline {
    exec_proc: [u8; 32],
    server_tick: [u8; 32],
    send_maps: [u8; 32],
}

#[cfg(target_os = "windows")]
type ExecProcFunction = unsafe extern "cdecl" fn(*const Proc) -> DMObject;

#[cfg(not(target_os = "windows"))]
type ExecProcFunction = unsafe extern "regparm3" fn(*const Proc) -> DMObject;

#[cfg(target_os = "windows")]
type ServerTickFunction = unsafe extern "stdcall" fn() -> i32;

#[cfg(not(target_os = "windows"))]
type ServerTickFunction = unsafe extern "cdecl" fn() -> i32;

type SendMapsFunction = unsafe extern "cdecl" fn();

struct ProcdefPointer(usize);

pub struct ByondReflectionData {
    strings_base_address: *const String,
    strings_len: *const usize,
    miscs_base_address: *const Misc,
    miscs_len: *const usize,
    procdefs_base_address: usize,
    procdefs_len: *const usize,
    procdef_desc: ProcDefsDescriptor,
    exec_proc_address: usize,
    orig_exec_proc: ExecProcFunction,
    server_tick_address: usize,
    orig_server_tick: ServerTickFunction,
    send_maps_address: usize,
    orig_send_maps: SendMapsFunction,
    trampoline: Trampoline,
}

impl ByondReflectionData {
    pub fn create_and_initialize_hooks(offsets: &Offsets, byondcore_base_address: usize) -> Self {
        let prologues = [
            offsets.prologue >> 0,
            offsets.prologue >> 8,
            offsets.prologue >> 16,
        ];

        // SAFETY: Provided offsets should have been verified to be the offsets of the BYOND internals we're looking for
        unsafe {
            Self {
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
                exec_proc_address: transmute(byondcore_base_address + offsets.exec_proc),
                orig_exec_proc: todo!(),
                server_tick_address: transmute(byondcore_base_address + offsets.server_tick),
                orig_server_tick: todo!(),
                send_maps_address: transmute(byondcore_base_address + offsets.send_maps),
                orig_send_maps: todo!(),
                trampoline: todo!(),
            }
        }
    }

    pub fn strings(&self) -> &[String] {
        unsafe { std::slice::from_raw_parts(self.strings_base_address, *self.strings_len) }
    }

    fn get_procdef(&self, index: usize) -> Option<ProcdefPointer> {
        todo!()
    }
}

// SAFETY: Pointers are read only and accessed in a manner with correct ownership from the BYOND runtime
unsafe impl Send for ByondReflectionData {}

// SAFETY: Pointers are read only and accessed in a manner with correct ownership from the BYOND runtime
unsafe impl Sync for ByondReflectionData {}
