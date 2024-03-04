{
  description = "A very basic flake";

  outputs = { self, nixpkgs }: let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    dynamicInputs = with pkgs; [
      vulkan-loader
      libxkbcommon
      # WINIT_UNIX_BACKEND=wayland
      wayland

      # WINIT_UNIX_BACKEND=x11
      xorg.libXcursor
      xorg.libXrandr
      xorg.libXi
      xorg.libX11
    ];
  in {

    packages.x86_64-linux.foo = pkgs.callPackage ./default.nix;

    packages.x86_64-linux.default = self.packages.x86_64-linux.foo;

    devShells.x86_64-linux.default = pkgs.mkShell {
      # https://github.com/rust-windowing/winit/issues/2807#issuecomment-1547061738
      LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath dynamicInputs;
      packages = with pkgs; [
        pkgs.llvmPackages.bintools # lld
        pkg-config
        vulkan-headers
        libGL
        fontconfig
        freetype
    ] ++ dynamicInputs;
    };
  };
}
