const std = @import("std");
const rx4 = @import("rig_bindings/rx4_bindings.zig");
const sha2 = @import("rig_bindings/sha2_bindings.zig");

pub fn main() void {
    const r_abi = rx4.rx4_abi_version();
    const r_ver = rx4.rx4_version();
    std.debug.print("zig-host-multi: rx4 ABI={d} version={s}\n", .{ r_abi, r_ver });

    const s_abi = sha2.sha2_abi_version();
    const s_ver = sha2.sha2_version();
    const s_name = sha2.sha2_name();
    std.debug.print("zig-host-multi: {s} ABI={d} version={s}\n", .{ s_name, s_abi, s_ver });
}
