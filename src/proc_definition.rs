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

struct Proc {
    definition: u32,
    flags: u8,
    supers: u8,
    unused: u16,

}
