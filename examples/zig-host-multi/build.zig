const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe = b.addExecutable(.{
        .name = "zig-host-multi",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });

    // `rig sync` / `rig add` inserts // rig-expose-begin:<pkg> … end link blocks here.

    
    // rig-expose-begin:rx4
    // rig: link `rx4` façade `rx4_ffi` from target/rig/rx4
    exe.root_module.addLibraryPath(b.path("target/rig/rx4"));
    exe.root_module.addRPath(b.path("target/rig/rx4"));
    exe.root_module.addIncludePath(b.path("target/rig/rx4"));
    exe.root_module.linkSystemLibrary("rx4_ffi", .{});
    // rig-expose-end:rx4

    
    // rig-expose-begin:sha2
    // rig: link `sha2` façade `sha2_ffi` from target/rig/sha2
    exe.root_module.addLibraryPath(b.path("target/rig/sha2"));
    exe.root_module.addRPath(b.path("target/rig/sha2"));
    exe.root_module.addIncludePath(b.path("target/rig/sha2"));
    exe.root_module.linkSystemLibrary("sha2_ffi", .{});
    // rig-expose-end:sha2

    b.installArtifact(exe);

    const run_step = b.step("run", "Run zig-host-multi");
    const run_cmd = b.addRunArtifact(exe);
    run_step.dependOn(&run_cmd.step);
}
