{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  wayland,
  libxkbcommon,
  dbus,
  wireplumber,
  pulseaudio,
  cava,
  curl,
  fontconfig,
  xdg-utils,
}:

rustPlatform.buildRustPackage {
  pname = "dshell";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type: !(lib.hasInfix "/target" path) && !(lib.hasInfix "/docs/screenshots" path);
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkg-config makeWrapper ];
  buildInputs = [ wayland libxkbcommon dbus ];

  # the tests only parse the example config and score app names; no display needed
  doCheck = true;

  postInstall = ''
    mkdir -p $out/share/dshell
    cp -r assets/wallpapers $out/share/dshell/
    cp -r config $out/share/dshell/
    # wpctl / pactl (sound), cava (visualizer), curl (weather, cover art), fc-match (fallback font)
    wrapProgram $out/bin/dshell \
      --set-default DSHELL_SHARE $out/share/dshell \
      --suffix PATH : ${lib.makeBinPath [ wireplumber pulseaudio cava curl fontconfig xdg-utils ]}
  '';

  meta = {
    description = "DarkShadow's Shell rewritten in Rust: bar, dynamic island, widgets, launcher, control center";
    homepage = "https://github.com/pabl0conepc-alt/DarkShadow-on-rust-";
    license = lib.licenses.mit;
    mainProgram = "dshell";
    platforms = lib.platforms.linux;
  };
}
