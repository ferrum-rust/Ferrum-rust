"""Generate the deterministic no-entity wooded-coast Anvil used by the frame benchmark."""

import gzip
import struct
import zlib
from test_legacy_world import string, tag, integer, byte_array


def nbt_list(name, kind, items):
    return tag(9, name, bytes([kind]) + struct.pack(">i", len(items)) + b"".join(items))


def long_tag(name, value):
    return tag(4, name, struct.pack(">q", value))


def boolean(name, value):
    return tag(1, name, bytes([int(value)]))


def write_level(path, settings):
    rules = {
        "doDaylightCycle": "false",
        "doMobSpawning": "false",
        "doFireTick": "false",
        "randomTickSpeed": "0",
    }
    feet = settings["player_feet"]
    player = nbt_list("Pos", 6, [struct.pack(">d", v) for v in feet]) + nbt_list(
        "Motion", 6, [struct.pack(">d", 0.0)] * 3
    )
    player += nbt_list(
        "Rotation", 5, [struct.pack(">f", settings[k]) for k in ["yaw_degrees", "pitch_degrees"]]
    )
    player += (
        tag(5, "Health", struct.pack(">f", 20))
        + boolean("OnGround", True)
        + integer("foodLevel", 20)
        + integer("Dimension", 0)
    )
    fields = [
        integer("version", 19133),
        tag(8, "LevelName", string(settings["name"])),
        long_tag("RandomSeed", settings["seed"]),
        tag(8, "generatorName", string("flat")),
        integer("generatorVersion", 0),
        tag(8, "generatorOptions", string("3;1;1;")),
        boolean("MapFeatures", False),
        integer("GameType", 0),
        boolean("initialized", True),
        boolean("allowCommands", True),
        tag(1, "Difficulty", bytes([0])),
        boolean("DifficultyLocked", True),
        long_tag("Time", settings["time"]),
        long_tag("DayTime", settings["time"]),
        boolean("raining", False),
        boolean("thundering", False),
        integer("clearWeatherTime", 1000000),
        integer("rainTime", 1000000),
        integer("thunderTime", 1000000),
        tag(10, "Player", player + b"\0"),
        tag(10, "GameRules", b"".join(tag(8, k, string(v)) for k, v in rules.items()) + b"\0"),
    ]
    fields.extend(integer("Spawn" + axis, int(v)) for axis, v in zip("XYZ", feet))
    (path / "level.dat").write_bytes(
        gzip.compress(tag(10, "", tag(10, "Data", b"".join(fields) + b"\0") + b"\0"), mtime=0)
    )


def world_chunks(seed):
    import numpy as np

    radius = 12
    origin = -radius * 16
    width = 16 * (radius * 2 + 1)
    z, x = np.mgrid[origin : origin + width, origin : origin + width]
    river = np.abs(z - (0.40 * x - 35 + 10 * np.sin(x / 38)))
    h = np.rint(
        65 + 6 * np.sin(x / 24) + 5 * np.cos(z / 33) + 4 * np.sin((2 * x - z) / 42) + 2 * np.cos((x + z) / 14)
    ).astype(int)
    h = np.minimum(h, np.floor(53 + river * 0.65).astype(int))
    h[np.maximum(np.abs(x - 8), np.abs(z - 8)) <= 4] = 77
    y = np.arange(96)[:, None, None]
    # Anvil numeric IDs: stone core, three dirt layers, grass or beach,
    # then still water below sea level. Store arrays in section Y/Z/X order.
    rock = np.zeros((96, width, width), dtype=np.uint8)
    rock[:] = np.where(y <= 61, 9, 0)
    np.copyto(rock, 2, where=y == h)
    np.copyto(rock, 12, where=(y == h) & (h < 62))
    np.copyto(rock, 3, where=y < h)
    np.copyto(rock, 1, where=y < h - 3)
    rock[0, :, :] = 7
    rng = np.random.default_rng(seed)
    for iz in range(5, width - 6, 7):
        for ix in range(5, width - 6, 7):
            tx = ix + int(rng.integers(-2, 3))
            tz = iz + int(rng.integers(-2, 3))
            ground = h[tz, tx]
            if ground < 63 or max(abs(tx + origin - 8), abs(tz + origin - 8)) < 13 or rng.random() > 0.44:
                continue
            top = ground + int(rng.integers(4, 8))
            for layer, extent in ((top - 2, 2), (top - 1, 2), (top, 1), (top + 1, 1)):
                canopy = rock[layer, tz - extent : tz + extent + 1, tx - extent : tx + extent + 1]
                canopy[canopy == 0] = 18
            rock[ground + 1 : top + 1, tz, tx] = 17
    # Equal precomputed lighting in both clients, including side-lit ground under tree crowns.
    opacity = np.where(rock == 0, 0, 15).astype(np.uint8)
    opacity[rock == 9] = 3
    opacity[rock == 18] = 1
    light = np.maximum(15 - np.cumsum(opacity[::-1], axis=0, dtype=np.int32), 0)[::-1].astype(np.int8)
    attenuation = np.maximum(opacity, 1).astype(np.int8)
    for _ in range(15):
        prev = light.copy()
        for axis in range(3):
            lo = [slice(None)] * 3
            hi = lo.copy()
            lo[axis] = slice(None, -1)
            hi[axis] = slice(1, None)
            a, b = tuple(lo), tuple(hi)
            np.maximum(light[a], prev[b] - attenuation[a], out=light[a])
            np.maximum(light[b], prev[a] - attenuation[b], out=light[b])
        if np.array_equal(prev, light):
            break
    height = (96 - np.argmax((rock != 0)[::-1], axis=0)).astype(">i4")
    for cz in range(-radius, radius + 1):
        for cx in range(-radius, radius + 1):
            rows = slice((cz + radius) * 16, (cz + radius + 1) * 16)
            cols = slice((cx + radius) * 16, (cx + radius + 1) * 16)
            sections = []
            for sy in range(6):
                sub = rock[sy * 16 : sy * 16 + 16, rows, cols]
                if not sub.any():
                    continue
                sky = light[sy * 16 : sy * 16 + 16, rows, cols].reshape(-1).astype(np.uint8)
                # CHECK_DECAY=false and DECAYABLE=false keep test foliage stable when chunks tick.
                meta = np.where(sub == 18, 4, 0).astype(np.uint8).reshape(-1)

                def nibble(a):
                    return (a[::2] | (a[1::2] << 4)).tobytes()

                sections.append(
                    tag(1, "Y", bytes([sy]))
                    + byte_array("Blocks", sub.tobytes())
                    + byte_array("Data", nibble(meta))
                    + byte_array("SkyLight", nibble(sky))
                    + byte_array("BlockLight", bytes(2048))
                    + b"\0"
                )
            chunk = (
                integer("xPos", cx)
                + integer("zPos", cz)
                + boolean("TerrainPopulated", True)
                + boolean("LightPopulated", True)
            )
            chunk += (
                long_tag("InhabitedTime", 0)
                + long_tag("LastUpdate", 6000)
                + byte_array("Biomes", bytes([1]) * 256)
            )
            chunk += tag(11, "HeightMap", struct.pack(">i", 256) + height[rows, cols].tobytes()) + nbt_list(
                "Sections", 10, sections
            )
            chunk += nbt_list("Entities", 10, []) + nbt_list("TileEntities", 10, []) + b"\0"
            yield cx, cz, zlib.compress(tag(10, "", tag(10, "Level", chunk) + b"\0"), 3)


def create_world(world, scene):
    region = world / "region"
    region.mkdir(parents=True)
    write_level(world, scene)
    regions = {}
    for x, z, compressed in world_chunks(scene["seed"]):
        data = regions.setdefault((x // 32, z // 32), bytearray(8192))
        entry = struct.pack(">iB", len(compressed) + 1, 2) + compressed
        sectors = (len(entry) + 4095) // 4096
        slot = (x % 32 + 32 * (z % 32)) * 4
        data[slot : slot + 4] = struct.pack(">I", (len(data) // 4096) * 256 + sectors)
        data.extend(entry)
        data.extend(bytes(sectors * 4096 - len(entry)))
    for (rx, rz), data in regions.items():
        (region / f"r.{rx}.{rz}.mca").write_bytes(data)
