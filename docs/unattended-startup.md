# Unattended startup

Camstation can start directly in its configured view and recover camera streams as the network becomes available. Use `--kiosk` to force kiosk mode, or enable kiosk-on-start in the view manager. Press **F11** to leave or re-enter kiosk mode. The pointer is hidden after three seconds without movement while kiosk mode is active.

Use `--windowed` during maintenance to ignore a saved kiosk-on-start preference:

```sh
camstation --windowed
```

`--kiosk` and `--windowed` cannot be used together. A deployment can also select a view by UUID or case-insensitive name:

```sh
camstation --kiosk --view Overview
```

## XDG desktop autostart

For a normal desktop session, create `~/.config/autostart/camstation.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=Camstation
Exec=/absolute/path/to/camstation --kiosk --view Overview
Terminal=false
X-GNOME-Autostart-enabled=true
```

Use the installed executable's absolute path. Log out and back in to test the entry. Desktop autostart supplies the Wayland or X11 session environment automatically.

## systemd user service

For a kiosk account using systemd, create `~/.config/systemd/user/camstation.service`:

```ini
[Unit]
Description=Camstation camera kiosk
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart=/absolute/path/to/camstation --kiosk --view Overview
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
```

Then enable the service:

```sh
systemctl --user daemon-reload
systemctl --user enable camstation.service
systemctl --user start camstation.service
```

The service must run inside a graphical user session with the appropriate `WAYLAND_DISPLAY` or `DISPLAY` environment. Desktop environments differ in how they import these variables into the systemd user manager. If the service cannot open a display, prefer XDG autostart or configure the session to import its display environment.

Camstation does not need to wait for the network before starting. Unavailable streams retain their tiles and reconnect automatically. Use `--log-file` to write structured logs with daily rotation, preventing unbounded growth in unattended deployments:

```sh
camstation --kiosk --log-file /var/log/camstation
```

For systemd services, journald provides log rotation automatically. For file-redirected deployments, `--log-file` is recommended. Avoid putting RTSP credentials directly in the service or desktop entry; keep them in Camstation's user-only configuration file instead.

Test startup, F11 access, network recovery, and display permissions before relying on an unattended deployment.
