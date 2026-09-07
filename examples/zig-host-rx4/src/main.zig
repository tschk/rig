const std = @import("std");
const rx4 = @import("rig_bindings/rx4_bindings.zig");

pub fn main() void {
    const abi = rx4.rx4_abi_version();
    const ver = rx4.rx4_version();
    std.debug.print("zig-host-rx4: native rx4 ABI={d} version={s}\n", .{ abi, ver });

    const agent = rx4.rx4_agent_new();
    defer rx4.rx4_agent_free(agent);
    const smoke = rx4.rx4_prompt_smoke(agent, "ping");
    std.debug.print("zig-host-rx4: prompt_smoke={d}\n", .{smoke});
}
