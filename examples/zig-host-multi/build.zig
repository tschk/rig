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

    // rig-expose-begin
    // rig: link `sha2` façade `sha2_ffi` from target/rig/sha2
    exe.root_module.addLibraryPath(b.path("target/rig/sha2"));
    exe.root_module.addRPath(b.path("target/rig/sha2"));
    exe.root_module.addIncludePath(b.path("target/rig/sha2"));
    exe.root_module.linkSystemLibrary("sha2_ffi", .{});
    // rig-expose-end

    b.installArtifact(exe);

    const run_step = b.step("run", "Run zig-host-multi");
    const run_cmd = b.addRunArtifact(exe);
    run_step.dependOn(&run_cmd.step);
}

