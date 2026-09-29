# Camview Product and Technical Specification

## 1. Purpose

Camview is a standalone Linux desktop application for continuously displaying RTSP cameras from a trusted local network. It presents up to ten cameras in configurable, saved grid layouts and is designed to run unattended as a single-monitor kiosk.

The initial target is eight simultaneous streams on a moderately specified x86-64 desktop with an Intel integrated GPU. NixOS and Ubuntu are the primary operating systems.

## 2. Goals

- Display up to ten heterogeneous RTSP streams, with eight as the normal workload.
- Keep live-view latency below two seconds on a healthy local network.
- Prefer any compatible hardware decoder available through GStreamer and fall back to software decoding when needed.
- Let users add cameras and arrange or resize their tiles entirely through the GUI.
- Save multiple named layouts and restore a selected layout at startup.
- Recover automatically from unavailable, interrupted, or stalled streams.
- Run fullscreen and unattended for extended periods.
- Package the application consistently for NixOS and Ubuntu.
- Leave clear extension points for snapshots, ONVIF PTZ, and motion highlighting.

## 3. Non-goals for the MVP

- Camera discovery or ONVIF discovery.
- Recording or playback of recorded footage.
- Motion detection or notifications.
- Snapshots.
- PTZ controls.
- Multiple monitors or synchronized multi-host displays.
- Remote administration or a web interface.
- Simultaneously mixing audio from multiple cameras.

These features may be added later without changing the core media architecture.

## 4. Target environment

### 4.1 Hardware

- Architecture: x86-64.
- Graphics: Intel integrated GPU with VA-API support.
- Display: one monitor.
- Expected workload: eight streams; hard UI limit for the MVP: ten.
- Cameras may differ in codec, resolution, frame rate, and audio support.

Performance depends on codec profiles, resolutions, GPU generation, and camera-side buffering. Camview will expose diagnostics rather than promise a fixed frame rate across unknown hardware and streams.

### 4.2 Operating systems and display servers

- NixOS, using a Nix development shell and optionally a native Nix package.
- Ubuntu Desktop, primarily using Flatpak.
- Wayland is preferred; X11 fallback is supported.

### 4.3 Distribution

Flatpak is the primary cross-distribution format. It provides more predictable GTK, GStreamer, codec-plugin, and GPU integration than AppImage, particularly on NixOS.

The Flatpak will require:

- Network access for RTSP streams.
- GPU device access for available hardware-decoding backends.
- Wayland and fallback X11 sockets.
- PipeWire/PulseAudio access for optional camera audio.
- Persistent application configuration storage.

An AppImage is not part of the MVP. It can be reconsidered after Flatpak validation.

## 5. Technology decisions

| Concern | Decision |
| --- | --- |
| Language | Rust, edition 2024 |
| GUI | GTK 4 via gtk-rs |
| Media | GStreamer 1.x via gstreamer-rs |
| Video presentation | `gtk4paintablesink` |
| Hardware decoding | Available GStreamer hardware decoders, selected through decoder autoplugging |
| Configuration | Versioned JSON via Serde |
| Logging | `tracing` with environment-filter support |
| CLI | `clap` |
| Development environment | Nix flake plus native Cargo workflow |
| Primary package | Flatpak |

### 5.1 Rejected alternatives

- **Electron/Tauri web video:** browsers do not natively provide the required RTSP pipeline control, and bridging decoded frames adds complexity and copies.
- **OpenCV:** useful for analysis, but not an efficient multi-stream playback framework.
- **Python:** suitable for a prototype, but less desirable for robust packaging and a long-running media kiosk.
- **Qt Multimedia:** viable, but offers less low-level RTSP pipeline control than direct GStreamer integration. Qt plus GStreamer would add another integration layer without a clear MVP benefit.
- **libVLC/libmpv instances:** easy for basic playback, but less suitable for per-stream pipeline tuning, diagnostics, and planned media extensions.

## 6. User experience

### 6.1 Application modes

#### View mode

- Shows the active saved layout.
- Camera tiles preserve their configured positions.
- Each tile may show a camera name and compact connection state overlay.
- Double-clicking a tile expands it within the application without rebuilding its pipeline; Escape or another double-click restores the grid.
- Audio can be enabled for one camera at a time.

#### Layout-edit mode

- Allows tiles to be dragged and resized.
- Positions and dimensions snap to logical grid cells.
- Invalid overlaps are rejected or visibly indicated.
- Changes can be saved or cancelled.
- Camera assignment and tile removal are available through tile actions.

#### Kiosk mode

- Starts fullscreen on the configured startup view.
- Hides editing controls and, after inactivity, the pointer.
- Does not exit merely because streams or the network are unavailable.
- F11 leaves or re-enters the complete kiosk state; layout editing remains unavailable until kiosk mode is exited.

### 6.2 Main screens and dialogs

1. **Live view:** active camera layout and status overlays.
2. **View manager:** create, rename, duplicate, select, and delete saved views.
3. **Layout editor:** arrange and resize camera tiles.
4. **Camera manager:** add, edit, remove, and test cameras.
5. **Application settings:** startup view, kiosk-on-start, status-overlay preferences, and logging level.
6. **Diagnostics:** pipeline state, decoder selected, codec, resolution, frame rate when known, reconnect status, and recent error text.

## 7. Camera configuration

Each camera has:

- Stable generated ID.
- Display name.
- RTSP URL.
- Optional secondary/substream RTSP URL.
- Transport preference: TCP by default, UDP optional, or automatic where supported.
- Audio availability and default mute state.
- Optional advanced pipeline settings added only when a demonstrated need exists.

The camera editor must validate basic fields and offer a connection test before saving. A failed test does not prevent saving because a camera may be temporarily unavailable.

RTSP credentials may initially be included in the URL. The configuration file must be created with user-only permissions where the platform permits. Secret Service integration is a post-MVP security improvement.

## 8. Saved view and layout model

A view contains:

- Stable ID and user-visible name.
- Logical row and column count.
- Ordered set of tiles.
- Optional per-view display preferences.

A tile contains:

- Camera ID.
- Grid column and row.
- Column span and row span.
- Optional overlay visibility override.

The first implementation uses `GtkGrid` with integer spans. The layout model remains toolkit-independent so a custom GTK layout manager can replace it if drag/resize behavior requires finer control.

Tiles preserve video aspect ratio and use letterboxing rather than stretching. Empty grid regions show the application background.

## 9. Media architecture

### 9.1 Independent pipelines

Each visible camera owns an independent GStreamer pipeline managed by a `CameraController`. Pipelines are not shared between cameras.

Benefits:

- A failed stream cannot tear down other streams.
- Reconnect policy and diagnostics remain per-camera.
- Camera-specific transport and audio settings are possible.
- Future views can stop streams that are not visible.

### 9.2 Pipeline construction

The application will build an RTSP pipeline dynamically:

1. Connect to the RTSP source.
2. Select the appropriate RTP depayloader and parser.
3. Autoplug a compatible decoder, preferring an available hardware backend where supported.
4. Present video through `gtk4paintablesink` without routing normal playback frames through Rust.
5. If audio exists, decode it into a controllable volume element and system audio sink.
6. Disable avoidable buffering and drop stale data rather than allow latency to grow.

The initial implementation uses `playbin3` for broad codec compatibility and configures its `rtspsrc` through the `source-setup` signal. If later requirements need more transport control, pipeline construction can move to `rtspsrc` plus dynamic pads without changing the controller interface.

### 9.3 Hardware acceleration

GStreamer decoder autoplugging selects among compatible registered factories and should prefer hardware decoders according to their plugin ranks. Supported backends may include VA-API, Intel QSV/MSDK, NVIDIA, V4L2, Vulkan, AMD AMF, or other platform plugins. Software decoding remains a fallback for unavailable hardware, unsupported codecs, or unsupported profiles.

The diagnostics UI and logs must expose the selected decoder and classify hardware acceleration using GStreamer factory metadata with known-backend fallbacks, so acceleration can be verified rather than assumed. Packaging provides Intel's media driver for the initial target, while runtime selection remains vendor-neutral.

Main/substream selection is supported in the data model. The grid should normally use a suitable substream when one is configured; future expanded-view behavior may switch to the main stream.

### 9.4 Latency

- Target: the lowest practical end-to-end latency on a healthy LAN, always below two seconds under representative conditions.
- Latency is not user-configurable; every camera uses the same minimum-latency policy.
- The RTSP jitter buffer is set to zero latency, RTSP buffering mode is disabled, and the video sink presents frames without clock synchronization.
- TCP is the default transport for predictable behavior; UDP may be considered later only if measurements show a meaningful benefit.
- Queues must be bounded, and stale frames should be dropped rather than accumulated.
- This policy favors immediacy over jitter tolerance and perfectly smooth frame pacing.
- Camera-side encoding and buffering may impose an irreducible portion of latency.

### 9.5 Audio

- Every stream starts muted.
- At most one camera is audible at a time.
- Unmuting one camera automatically mutes the previously audible camera.
- Audio failures do not stop video playback.

## 10. Stream lifecycle and recovery

A camera controller has these externally visible states:

- `Stopped`
- `Starting`
- `Playing`
- `Stalled`
- `Reconnecting`
- `Failed`

The controller listens to its GStreamer bus for errors, state changes, buffering, and end-of-stream messages. It also tracks frame delivery so a connected-but-stalled stream can be detected.

On failure, the pipeline is torn down and rebuilt after capped exponential backoff:

```text
1 s, 2 s, 5 s, 10 s, 15 s, 30 s
```

A successful playback interval resets the backoff. The tile stays present and shows a connecting, stalled, or error overlay including concise error text and retry status.

Application shutdown explicitly stops every pipeline and releases media resources.

## 11. Configuration persistence

Default location:

```text
$XDG_CONFIG_HOME/camview/config.json
```

If `XDG_CONFIG_HOME` is unset, the platform-appropriate user configuration directory is used.

Requirements:

- The root object contains `schema_version`.
- Unknown future fields should not unnecessarily prevent startup.
- Semantic validation occurs after deserialization.
- Writes use a temporary file followed by an atomic rename.
- A last-known-good backup is retained.
- A malformed configuration produces an actionable error and does not silently overwrite the file.
- IDs are stable UUIDs and references are validated.

An illustrative schema is:

```json
{
  "schema_version": 1,
  "startup_view": "baf8b68e-b944-4662-a51e-56c6daea4094",
  "kiosk_on_start": true,
  "cameras": [
    {
      "id": "564f8172-5030-4e9e-883f-83825ef5a30f",
      "name": "Front door",
      "rtsp_url": "rtsp://camera.local/stream",
      "substream_url": null,
      "transport": "tcp"
    }
  ],
  "views": [
    {
      "id": "baf8b68e-b944-4662-a51e-56c6daea4094",
      "name": "Overview",
      "columns": 4,
      "rows": 4,
      "tiles": [
        {
          "camera_id": "564f8172-5030-4e9e-883f-83825ef5a30f",
          "column": 0,
          "row": 0,
          "column_span": 2,
          "row_span": 2
        }
      ]
    }
  ]
}
```

## 12. Command-line interface

Planned interface:

```text
camview [OPTIONS]

--kiosk                 Start fullscreen in kiosk mode
--windowed              Ignore kiosk-on-start and open with normal controls
--view <ID_OR_NAME>      Override the configured startup view
--config <PATH>          Use an alternate configuration file
--log <FILTER>           Override the tracing filter
--version                Show version information
```

GUI startup remains the default. CLI options support deployment and troubleshooting rather than replacing GUI configuration.

## 13. Logging and diagnostics

- Use structured `tracing` events.
- Default logs avoid printing complete RTSP URLs because they may contain credentials.
- `RUST_LOG` and `--log` can enable component-specific debug output.
- GStreamer debug output remains separately controllable through `GST_DEBUG`.
- Expected temporary camera failures are warnings, not application-fatal errors.
- Panic and startup errors should identify an actionable cause.

## 14. Reliability and performance requirements

- One failed or malformed stream must not interrupt other streams.
- The application remains open when no cameras are available.
- Network availability after process startup must be handled through retries.
- Configuration updates must not corrupt the previous valid configuration.
- Blocking media or file operations must not run on the GTK main thread.
- Normal rendering must avoid CPU readback of decoded video frames.
- Repeated reconnects must not leak pipelines, bus watches, timers, or GTK objects.
- Eight-camera soak tests should run for at least 24 hours before calling the kiosk MVP stable.

## 15. Security and privacy

- The application is intended for a trusted LAN but treats RTSP credentials as sensitive.
- Full URLs must be redacted in routine logs and UI error reports.
- Configuration permissions should be restricted to the owning user.
- No telemetry, cloud service, or external network communication is included.
- Flatpak network access cannot be limited to the LAN by the application package; deployment firewall policy may enforce that if needed.

## 16. Testing strategy

### Unit tests

- Configuration serialization and validation.
- Schema migration.
- Grid bounds and overlap checks.
- Reconnect backoff calculation.
- URL redaction.
- Camera and view reference integrity.

### Component tests

- Pipeline construction against local synthetic GStreamer sources.
- State transitions for bus errors and end-of-stream.
- Watchdog behavior for stalled delivery.
- Atomic configuration replacement and recovery.

### Manual/system tests

- H.264, H.265, and MJPEG cameras where available.
- Streams with and without audio.
- Hardware-decoder selection on available GPU backends, including Intel VA-API on the target deployment system.
- Eight simultaneous streams.
- Camera power loss and recovery.
- Network loss and recovery.
- Invalid credentials and invalid URLs.
- Wayland and X11.
- NixOS and Ubuntu Flatpak.
- Kiosk startup after reboot.
- Long-running soak test.

A local H.264 RTSP test stream is available at `rtsp://127.0.0.1:8554/city-traffic` for repeatable development tests. It is not an application runtime dependency.

## 17. MVP acceptance criteria

The MVP is complete when:

1. A user can add, edit, test, and remove manually configured RTSP cameras in the GUI.
2. Eight cameras can be displayed concurrently on the target Intel desktop, subject to codec capabilities of that hardware.
3. Heterogeneous supported streams select an appropriate decoder, preferring a compatible available hardware backend.
4. The user can create multiple named views and graphically move and resize tiles on a snapped grid.
5. Saved configuration and the selected startup view survive application restarts.
6. Kiosk mode starts fullscreen without requiring interaction.
7. Failed and stalled streams retain their tile, display status, and reconnect automatically.
8. Healthy local streams remain below two seconds of latency under representative conditions.
9. A single camera can be unmuted, and unmuting it mutes every other camera.
10. The application is usable on NixOS and as a Flatpak on Ubuntu.
11. Routine logs do not expose RTSP credentials.
12. No camera failure causes the application or another stream to stop.

## 18. Delivery milestones

### M0: Development foundation

- Cargo project and module boundaries.
- Nix development shell.
- GTK/GStreamer initialization smoke test.
- Formatting, linting, and test commands.

### M1: Media proof of concept

- One RTSP URL rendered with `gtk4paintablesink`.
- Hardware decoder and low-latency pipeline diagnostics.
- Basic pipeline error display.

### M2: Reliable multi-camera playback

- Independent controllers and pipelines.
- Up to ten camera tiles.
- Reconnect backoff and stall watchdog.
- Exclusive audio selection.

### M3: Configuration and views

- Versioned, atomic persistence.
- Camera manager and connection test.
- Multiple views and startup selection.

### M4: Graphical layout and kiosk

- Drag, resize, snapping, and overlap validation.
- Fullscreen kiosk behavior and CLI options.
- Unattended startup documentation.

### M5: Packaging and hardening

- Flatpak manifest and codec validation.
- Nix package or launch wrapper.
- Cross-distribution tests.
- Eight-stream performance and soak testing.

## 19. Deferred decisions

These do not block M0 or M1:

- Final reverse-DNS application ID, required before publishing a Flatpak.
- Open-source license and public repository location.
- Whether credentials move to Secret Service before the first public release.
- Whether expanded tiles dynamically switch from substream to main stream.

Until a publishing ID is chosen, development uses `io.github.orthuber.Camview` as a provisional application ID. It can be changed before external release.
