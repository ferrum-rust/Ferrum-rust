# Ferrum 1.8.9 (`Ferrum-rust`)

**[한국어](README.md)** | **English**

**Ferrum 1.8.9** is an experimental **100% native Rust + Vulkan (`wgpu` 24)** Minecraft 1.8.9-compatible client and integrated singleplayer/LAN runtime — built from scratch without a JVM or OpenGL wrapper.

> **Note**: This repository does not include any copyrighted Minecraft binaries (`.jar`), textures, sounds, or assets. A legitimate local installation of **Minecraft 1.8.9** is required; assets are extracted locally on first setup.

---

## Key Features

### 1. Native Vulkan Renderer & Performance Pipeline
- **High-Throughput Terrain Rendering**: Hidden-face culling, greedy meshing, frustum & back-face culling, and frame-budgeted asynchronous chunk meshing & GPU uploads
- **Up to 128-Chunk Distant Terrain (LOD)**: Asynchronous multi-resolution surface LOD tiles beyond the full-detail near radius up to 128 chunks, backed by shared GPU arena buffers and merged draw calls
- **Lighting & Visibility**: Split skylight/blocklight propagation, smooth lighting (AO) refinement around slabs and stairs, distance-sorted translucency, and wall-aware entity occlusion culling
- **Sodium-Style Video Settings UI**: Live in-game controls for graphics quality, render distance, smooth lighting, clouds, particles, VSync, entity culling, and per-frame chunk upload budgets

### 2. World Generation & Multi-Dimension Support
- **Seed-Faithful 1.8.9 Worldgen**: Overworld (`DEFAULT`, Superflat, Large Biomes, Amplified, Customized) biomes, terrain density, caves, ravines, ore veins, villages, mineshafts, dungeons, and vegetation matching original RNG ordering
- **Nether & End Dimensions**: Nether portal construction, coordinate scaling, and portal linking, Nether Fortresses, End travel, and the full **Ender Dragon boss fight, End Crystals, and exit podium**
- **Legacy World Import & Background Saves**: Read-only loading of existing 1.8.9 Anvil (`.mca` / `level.dat`) saves, plus non-blocking background snapshot autosaves (`saves/native-v4`)

### 3. Singleplayer Simulation & Gameplay
- **Dedicated 20 Hz Local Tick Worker**: Physics, scheduled block ticks, redstone circuits, and mob AI run on a dedicated thread decoupled from frame presentation
- **Survival & Block Mechanics**: Tool tiers, mining speed & durability, hunger & saturation, potions & status effects (including milk cure), enchantment glint, water/lava flow, falling blocks (sand, gravel, anvils, dragon eggs), sponge water absorption & Nether evaporation, redstone components, and command blocks
- **Entities & Mounts**: 32 mob species with rendering, AI, combat, and breeding, villager trading, and rideable horses (jump bar & inventory), pigs, minecarts (normal, powered, detector, and activator rails), and boats
- **Spatial 3D Audio**: Distance-attenuated, stereo-panned world sound effects and music (`rodio` OGG Vorbis decoding)

### 4. Native Launcher, LAN Sharing & Multiplayer (Protocol 47)
- **Win32 Account Launcher (`launcher`)**: Microsoft OAuth Device Code sign-in alongside instant offline profile switching
- **Open to LAN**: Share any singleplayer world over the local network via **Esc → Open to LAN**
- **1.8.9 Multiplayer Client**: Protocol 47 server list ping, direct connect, tab-complete chat, and server resource pack support

---

## Getting Started

### Prerequisites
- **OS**: Windows 10 / 11 (64-bit)
- **GPU**: Vulkan 1.2+ capable graphics card with up-to-date drivers
- **Toolchain**:
  - [Rust](https://rustup.rs/) (`stable-x86_64-pc-windows-msvc`)
  - [Python 3.9+](https://www.python.org/) (used once to prepare local assets)
- **Minecraft 1.8.9**: Official Minecraft 1.8.9 launched at least once under `%APPDATA%\.minecraft`

### 1. Prepare Local Assets
Extract textures, sounds, and language files from your local Minecraft 1.8.9 installation into `.local/resources`:

```bat
decompile.bat
resources.bat
```

### 2. Build Release Binaries
Compile both the client (`ferrum189.exe`) and the native Win32 launcher (`launcher.exe`):

```powershell
cargo build --release --locked --bins
```

### 3. Run
Launch via `start.bat` or the account launcher:

```bat
:: Launch the client directly
start.bat

:: Open the Microsoft / offline account launcher
target\release\launcher.exe
```

Common CLI options:

```bat
:: Jump straight into a Creative world with a specific seed
start.bat --play --creative --seed 189

:: Open an existing .minecraft Anvil save (original files remain read-only)
start.bat --world "%APPDATA%\.minecraft\saves\New World"

:: Connect directly to a 1.8.9 multiplayer server
start.bat --connect 127.0.0.1:25565 --username Player

:: Run a headless worldgen + meshing sanity check
target\release\ferrum189.exe --check --render-distance 4 --fresh
```

---

## Default Controls

All 33 keybindings can be customized in **Options → Controls**.

| Input | Action |
| --- | --- |
| `W` `A` `S` `D` / Mouse | Move / Look |
| `Space` / `Ctrl` / `Shift` | Jump & Swim / Sprint / Sneak (Dismount) |
| Double-tap `Space` | Toggle Creative Flight (`Space` up / `Shift` down) |
| Left Click / Right Click | Attack & Mine / Place, Interact & Use Item |
| Scroll Wheel / `1`–`9` | Select Hotbar Slot (Middle Click: Pick Block) |
| `E` / `Esc` | Open Inventory / Close Window & Pause Menu |
| `Q` / `Ctrl + Q` | Drop One Item / Drop Full Stack |
| `T` / `/` | Open Chat / Enter Command (`Tab` Auto-Complete) |
| `F1` / `F2` / `F3` | Hide HUD / Save Screenshot (`screenshots/`) / Debug Overlay |
| `F5` / `F11` | Cycle Camera Perspective / Toggle Fullscreen |
| `F3 + A` / `F3 + B` / `F3 + H` | Reload Chunks / Entity Hitboxes / Advanced Item Tooltips |

---

## Project Layout

```text
Ferrum-rust/
├── src/                    # Core Rust client, Vulkan renderer, worldgen, simulation, networking & WGSL shaders
│   ├── bin/launcher.rs     # Native Win32 account manager & game launcher
│   ├── vanilla/            # 1.8.9 terrain, structures, lighting, physics, combat & item rules
│   ├── commands/           # Singleplayer slash command engine
│   └── ui/                 # Menus, inventory windows, chat & Sodium-style video settings UI
├── sodium-dev/             # UI icon resources referenced at compile time
├── scripts/                # Asset extraction, reference comparison & benchmarking scripts (Python / PowerShell)
├── tools/java/             # Oracle utilities used during development to verify 1.8.9 behavior
└── docs/                   # Technical documentation
    ├── compatibility.md    # Feature compatibility & implementation matrix
    ├── protocol-47.md      # Protocol 47 multiplayer packet coverage
    └── benchmark.md        # Rendering & streaming benchmark guide
```

See [docs/compatibility.md](docs/compatibility.md), [docs/protocol-47.md](docs/protocol-47.md), and [docs/benchmark.md](docs/benchmark.md) for in-depth technical notes.

---

## Disclaimer

- This project is an independent, unofficial clean-room/compatibility implementation for educational and technical research purposes. It is **not affiliated with, endorsed by, or associated with Mojang Studios or Microsoft**.
- No proprietary Minecraft binaries or assets are distributed in this repository. All Minecraft trademarks and assets belong to their respective owners.
