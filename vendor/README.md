# Native window retirement compatibility patch

Sidecar uses the crates.io eframe 0.31.1 source with one Linux-only change in
`src/native/run.rs`. The registry archive SHA-256 and every original file hash
are recorded in `eframe-upstream.json`. The archive's source revision is
1669e52a7ccfc3489c1b0999b9ed48894a0b3887 in https://github.com/emilk/egui.
MIT and Apache-2.0 license notices are copied from that exact revision.

winit 0.30.13's Wayland Window::drop sets a retirement flag. A later event-loop
iteration removes the retained WindowState and destroys its protocol objects.
eframe drops windows in LoopExiting, after its last regular iteration. Returning
to a blocking tray receive at that point can leave a black native window behind.

`eframe-window-retirement.patch` adds one windowless run-on-demand cycle after
normal Linux teardown. Its handler creates no window/GL context; AboutToWait exits,
and winit flushes destruction through its final roundtrip. Cold tray startup and
parked receive behavior remain event-driven; there is no periodic cleanup timer.

The package is excluded from workspace membership. Third-party source is not
reformatted with the application; keep this patch narrow and auditable. Run
`python3 tests/verify_vendor.py` before builds/releases. Source identity includes
vendored Rust/manifests, and source distributions include vendor files/licenses.
`cargo audit` still runs; a local compatibility patch is not an upstream security
certification. No Cargo registry files or system libraries are modified.

The latest released winit checked during this fix was 0.30.13. Remove this local
patch when a supported released stack demonstrably retires windows before
returning; use the real KWin lifecycle regression, not just application status.
Upgrade provenance and re-run private-bus, native Wayland/X11, packaging and
advisory checks when changing it. No automatic upstream submission is performed.
