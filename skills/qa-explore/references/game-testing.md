# Game evidence boundaries

Use the host's existing Unreal/Unity/Roblox MCP, native computer-use, device tools or configured test runners. This package does not ship an engine controller, engine binaries or a real-time gameplay policy.

Record map, build, platform, input method, resolution and server/client roles. Separate editor, instrumented test-build, headless and rendered packaged results. Internal API movement and teleportation do not establish normal-player-input traversal. A server-side state update does not establish that all clients saw the correct result.

Let an existing lower-level controller perform continuous movement, aiming and timed input. The frontier model should select objectives and inspect outcomes. Record input duration and whether focus/capture was established. Do not claim controller/mobile acceptance from keyboard-only evidence.

For multiplayer, correlate each peer's evidence by session and role. For performance, record hardware and measurement conditions, account for automation overhead, and use the target's profiler/telemetry; screenshots alone do not measure frame time. Preserve save data, initial state and seeds, while acknowledging physics/network nondeterminism.
