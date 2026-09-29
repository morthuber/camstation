{
  description = "Camview development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      devShells = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          gst = pkgs.gst_all_1;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              gtk4
              intel-media-driver
              libva-utils
              pkg-config
              rustc
              rustfmt
              gst.gstreamer
              gst.gst-libav
              gst.gst-plugins-bad
              gst.gst-plugins-base
              gst.gst-plugins-good
              gst.gst-plugins-rs
              gst.gst-plugins-ugly
            ];

            RUST_BACKTRACE = "1";

            shellHook = ''
              echo "Camview development shell"
              echo "GTK:       $(pkg-config --modversion gtk4)"
              echo "GStreamer: $(pkg-config --modversion gstreamer-1.0)"
              echo "Run 'cargo run' to start the application."
            '';
          };
        });
    };
}
