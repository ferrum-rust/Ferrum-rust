"""Integration test: construct Anvil/NBT fixtures without any Minecraft content.

Checks negative chunk coordinates, zlib/gzip/uncompressed payloads, high block
IDs, metadata nibbles, modern-world rejection, and corrupt-sector rejection.
Run after building: python scripts/test_legacy_world.py
"""
import gzip
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / 'target/release/ferrum189.exe'

def string(text):
    data = text.encode()
    return struct.pack('>H', len(data)) + data

def tag(kind, name, payload):
    return bytes([kind]) + string(name) + payload

def integer(name, value):
    return tag(3, name, struct.pack('>i', value))

def byte_array(name, value):
    return tag(7, name, struct.pack('>i', len(value)) + value)

def fixture(path, compression=2, modern=False, corrupt=False, corrupt_light=False, player=False):
    path.mkdir(parents=True)
    spawn = 8192 if player else -8
    data = integer('SpawnX', spawn) + integer('SpawnY', 66) + integer('SpawnZ', spawn)
    if player:
        def typed_list(name, element, fmt, values):
            return tag(9, name, bytes([element]) + struct.pack('>i', len(values)) +
                       b''.join(struct.pack(fmt, v) for v in values))
        pose = typed_list('Pos', 6, '>d', (-8.123456789, 64.125, -8.987654321))
        pose += typed_list('Motion', 6, '>d', (0.0734567890123, -0.125, 11.0))
        pose += typed_list('Rotation', 5, '>f', (450.25, -62.125))
        pose += tag(1, 'OnGround', b'\x01') + integer('Dimension', 0)
        pose += integer('SpawnX', -17) + integer('SpawnY', 72) + integer('SpawnZ', 15)
        pose += tag(1, 'SpawnForced', b'\x01')
        data += tag(10, 'Player', pose + b'\0')
    if modern:
        data += integer('DataVersion', 4000)
    (path / 'level.dat').write_bytes(gzip.compress(tag(10, '', tag(10, 'Data', data + b'\0') + b'\0')))
    (path / 'region').mkdir()
    blocks = bytearray(4096)
    metadata = bytearray(2048)
    add = bytearray(2048)
    for z in range(16):
        for x in range(16):
            blocks[z * 16 + x] = 2
    blocks[256] = 35
    blocks[257] = 35
    metadata[128] = 0xE1
    blocks[258] = 1
    add[129] = 0x01  # Legacy 12-bit block ID 257, low nibble.
    section = tag(1, 'Y', b'\x04') + byte_array('Blocks', blocks) + byte_array('Data', metadata) + byte_array('Add', add)
    section += byte_array('SkyLight', bytes([0x75]) * (2047 if corrupt_light else 2048)) + byte_array('BlockLight', bytes([0x42]) * 2048) + b'\0'
    sections = tag(9, 'Sections', b'\x0a' + struct.pack('>i', 1) + section)
    height = tag(11, 'HeightMap', struct.pack('>i', 256) + b''.join(struct.pack('>i', 70 + i % 4) for i in range(256)))
    chunk = tag(10, '', tag(10, 'Level', integer('xPos', -1) + integer('zPos', -1) + sections + height + b'\0') + b'\0')
    compressed = {1: gzip.compress, 2: zlib.compress, 3: lambda b: b}[compression](chunk)
    payload = struct.pack('>I', len(compressed) + 1) + bytes([compression]) + compressed
    sectors = (len(payload) + 4095) // 4096
    header = bytearray(8192)
    slot = (31 + 31 * 32) * 4
    header[slot:slot + 4] = bytes([0, 0, 1 if corrupt else 2, sectors])
    (path / 'region/r.-1.-1.mca').write_bytes(header + payload + bytes(sectors * 4096 - len(payload)))

def main():
    with tempfile.TemporaryDirectory(prefix='ferrum189-fixture-') as temp:
        root = Path(temp)
        for compression in [1, 2, 3]:
            path = root / str(compression)
            fixture(path, compression=compression)
            result = subprocess.run([str(EXE), '--world', str(path), '--check', '--fresh', '--render-distance', '2'], cwd=ROOT, capture_output=True, text=True)
            assert result.returncode == 0, result.stdout + result.stderr
            report = json.loads((ROOT / 'reports/world.json').read_text())
            assert report['chunks'] == 1, report
            assert report['greedy_quads'] > 6, report
            assert report['block_states'] == {'32': 256, '561': 1, '574': 1, '4112': 1}, report
        path = root / 'player'
        fixture(path, player=True)
        def source_hashes():
            return {p.relative_to(path): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in path.rglob('*') if p.is_file()}
        before = source_hashes()
        result = subprocess.run([str(EXE), '--world', str(path), '--check', '--fresh', '--render-distance', '2'],
                                cwd=ROOT, capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        report = json.loads((ROOT / 'reports/world.json').read_text())
        assert report['chunks'] == 1 and report['world_spawn'] == [8192, 66, 8192], report
        assert report['player_position'] == [-8.123456789, 64.125, -8.987654321], report
        assert report['player_motion'] == [0.0734567890123, -0.125, 0.0], report
        assert report['player_rotation'] == [90.25, -62.125] and report['player_on_ground'], report
        assert report['personal_spawn'] == {'point': [-17, 72, 15], 'forced': True}, report
        assert source_hashes() == before, 'Read-only import modified its source'
        for name, kwargs, error in [('modern', {'modern': True}, 'newer than 1.8.9'), ('corrupt', {'corrupt': True}, 'Invalid Anvil sector offset'), ('bad-light', {'corrupt_light': True}, 'Invalid legacy SkyLight array length')]:
            path = root / name
            fixture(path, **kwargs)
            result = subprocess.run([str(EXE), '--world', str(path), '--check', '--fresh', '--render-distance', '2'], cwd=ROOT, capture_output=True, text=True)
            assert result.returncode != 0 and error in result.stderr, result.stdout + result.stderr
    print('7 Anvil integration scenarios passed.')

if __name__ == '__main__':
    main()
