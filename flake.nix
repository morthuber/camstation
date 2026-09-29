{
  description = "Camstation RTSP camera viewer";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs supportedSystems;
      packageFor = pkgs:
        let
          gst = pkgs.gst_all_1;
          gstPlugins = [
            gst.gstreamer
            gst.gst-plugins-base
            gst.gst-plugins-good
            gst.gst-plugins-bad
            gst.gst-libav
            gst.gst-plugins-rs
          ];
          gstPluginPath = pkgs.lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "camstation";
          version = "0.2.1";
          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
          ];

          buildInputs = [
            pkgs.gtk4
            gst.gstreamer
            gst.gst-plugins-base
          ];

          postInstall = ''
            install -Dm644 resources/org.camstation.camstation.desktop \
              $out/share/applications/org.camstation.camstation.desktop
            install -Dm644 resources/org.camstation.camstation.metainfo.xml \
              $out/share/metainfo/org.camstation.camstation.metainfo.xml
            install -Dm644 resources/org.camstation.camstation.svg \
              $out/share/icons/hicolor/scalable/apps/org.camstation.camstation.svg
            install -Dm644 LICENSE $out/share/licenses/camstation/LICENSE
            install -Dm644 THIRD_PARTY_NOTICES.md \
              $out/share/doc/camstation/THIRD_PARTY_NOTICES.md
          '';

          preFixup = ''
            gappsWrapperArgs+=(
              --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${gstPluginPath}"
            )
          '';

          meta = {
            description = "Configurable multi-camera RTSP station";
            license = pkgs.lib.licenses.mit;
            mainProgram = "camstation";
            platforms = supportedSystems;
          };
        };
    in
    {
      packages = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          camstation = packageFor pkgs;
        in
        {
          inherit camstation;
          default = camstation;
        });

      apps = forEachSystem (system: {
        camstation = {
          type = "app";
          program = "${self.packages.${system}.camstation}/bin/camstation";
          meta.description = "Configurable multi-camera RTSP station";
        };
        default = self.apps.${system}.camstation;
      });

      devShells = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          gst = pkgs.gst_all_1;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              cargo-llvm-cov
              clippy
              curl
              flatpak
              flatpak-builder
              gtk4
              intel-media-driver
              libva-utils
              llvmPackages.llvm
              pkg-config
              patchelf
              rustc
              rustfmt
              appstream
              desktop-file-utils
              xauth
              xvfb-run
              (python3.withPackages (pythonPackages: [
                pythonPackages.aiohttp
                pythonPackages.tomlkit
              ]))
              gst.gstreamer
              gst.gst-libav
              gst.gst-plugins-bad
              gst.gst-plugins-base
              gst.gst-plugins-good
              gst.gst-plugins-rs
            ];

            RUST_BACKTRACE = "1";
            LLVM_COV = "${pkgs.llvmPackages.llvm}/bin/llvm-cov";
            LLVM_PROFDATA = "${pkgs.llvmPackages.llvm}/bin/llvm-profdata";

            shellHook = ''
              echo "Camstation development shell"
              echo "GTK:       $(pkg-config --modversion gtk4)"
              echo "GStreamer: $(pkg-config --modversion gstreamer-1.0)"
              echo "Run 'cargo run' to start the application."
            '';
          };
        });
    };
}
