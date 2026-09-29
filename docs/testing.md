# Testing

Camstation separates fast logic tests from tests that require media or a
graphical display:

```sh
make test          # unit, property, persistence, and fast component tests
make test-media    # synthetic GStreamer component tests
make test-ui       # ignored GTK workflows under Xvfb
make test-all      # all of the above
```

The Nix development shell includes `cargo-llvm-cov` and `xvfb-run`:

```sh
nix develop
make coverage
make coverage-html
```

The initial non-GTK coverage baseline is 91.8% of lines in configuration,
95.8% in layout logic, 100% in media lifecycle policy, 83.9% in UI mode logic,
and 34.8% overall. Overall coverage is intentionally not enforced because most
GTK callback wiring is validated by workflow tests rather than isolated line
execution.

Property tests exercise random tile positions and spans. Media component tests
use finite `videotestsrc` pipelines and do not require a physical camera or
network service. The UI suite is serialized because GTK and the default GLib
main context are process-global.

Package and live-stream validation remains documented separately in
[`package-validation.md`](package-validation.md).
