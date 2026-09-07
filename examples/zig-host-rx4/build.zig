const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe = b.addExecutable(.{
        .name = "zig-host-rx4",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });

    // `rig add --rust rx4` inserts a link block before installArtifact.

    
    // rig-expose-begin
    // rig: link `rx4` façade `rx4_ffi` from target/rig/rx4
    exe.root_module.addLibraryPath(b.path("target/rig/rx4"));
    exe.root_module.addRPath(b.path("target/rig/rx4"));
    exe.root_module.addIncludePath(b.path("target/rig/rx4"));
    exe.root_module.linkSystemLibrary("rx4_ffi", .{});
    // rig-expose-end

    b.installArtifact(exe);

    const run_step = b.step("run", "Run zig-host-rx4");
    const run_cmd = b.addRunArtifact(exe);
    run_step.dependOn(&run_cmd.step);
}
