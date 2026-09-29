# Package validation checklist

Record the date, package type, package version, operating system, display
server, GStreamer version, and decoder reported by each stream.

## Automated checks

```sh
make validate-packages
make package-nix
make package-flatpak
make validate-flatpak
```

For a release candidate, also build the Flatpak bundle and the beta AppImage
on its Ubuntu 24.04 build host. Retain SHA-256 checksums with released files.

## Functional matrix

Run these checks for the native Nix package and Flatpak on Wayland and X11.
Run the beta AppImage directly on Ubuntu 24.04 and through `appimage-run` on
current NixOS.

- Start with no existing configuration.
- Add, test, edit, and delete a camera.
- Save multiple views and verify restart persistence.
- Play an H.264 RTSP stream with software decoding available.
- Play an H.265 RTSP stream with software decoding available.
- Play an MJPEG RTSP stream.
- Verify a stream with audio starts muted and can be selected exclusively.
- Display eight streams concurrently for a short functional run.
- Disconnect and restore one camera without disrupting the other streams.
- Disconnect and restore the network.
- Verify invalid credentials and malformed URLs remain non-fatal.
- Start in kiosk mode, wait for pointer hiding, and leave with F11.
- Save and cancel graphical layout edits.
- Confirm routine logs do not include RTSP credentials.

Inside the Flatpak, the required elements can be inspected with:

```sh
flatpak run --command=gst-inspect-1.0 org.camstation.camstation gtk4paintablesink
flatpak run --command=gst-inspect-1.0 org.camstation.camstation avdec_h264
flatpak run --command=gst-inspect-1.0 org.camstation.camstation avdec_h265
flatpak run --command=gst-inspect-1.0 org.camstation.camstation jpegdec
```

## Deferred deployment signoff

The current package checks validate software decoding and package behavior.
Before declaring a specific kiosk deployment stable, separately complete:

- Intel VA-API decoder verification on the target machine.
- Representative eight-camera latency and resource measurements.
- A 24-hour unattended soak test.
- Reboot and graphical-session autostart verification.
