"""Verification oracle: run original 1.8.9 bytecode to obtain reference worlds.

No game code or libraries are downloaded. Minecraft files are read-only; the
caller selects a project-local generated world, never an original save.
The Rust game never invokes this program or Java. This is a comparison tool,
not a runtime world generator for Ferrum.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def java_executable():
    """Pin original 1.8.9 numeric behavior to Java 8 when locally installed.

    MC189_JAVA explicitly selects another VM for cross-version verification.
    Never change the user's global PATH or launch their installed game.
    """
    override = os.environ.get('MC189_JAVA')
    if override:
        return override
    java8 = Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / 'Java/jre1.8.0_501/bin/java.exe'
    if java8.is_file():
        return str(java8)
    executable = shutil.which('java')
    if not executable:
        raise RuntimeError('Java is required only to regenerate the original-bytecode fixtures')
    return executable


def classpath(jar):
    named = ROOT / '.local/reference/1.8.9-named.jar'
    if not named.is_file():
        raise RuntimeError('Run decompile.bat first to prepare the owned mapped 1.8.9 JAR')
    metadata = json.loads(jar.with_suffix('.json').read_text(encoding='utf-8'))
    minecraft = jar.parent.parent.parent
    result = [named]
    for library in metadata.get('libraries', []):
        artifact = library.get('downloads', {}).get('artifact', {})
        relative = artifact.get('path')
        if not relative:
            group, name, version, *_ = library['name'].split(':')
            relative = f'{group.replace(".", "/")}/{name}/{version}/{name}-{version}.jar'
        path = minecraft / 'libraries' / relative
        if path.is_file():
            result.append(path)
    return os.pathsep.join(map(str, result))


def command(jar, world, seed):
    java = java_executable()
    javac = os.environ.get('MC189_JAVAC') or shutil.which('javac')
    if not java or not javac:
        raise RuntimeError('A JDK with java and javac on PATH is required for original vanilla generation')
    cp = classpath(jar)
    classes = ROOT / '.local/vanilla-bridge/classes'
    classes.mkdir(parents=True, exist_ok=True)
    source = ROOT / 'tools/java/VanillaWorldBridge.java'
    compiled = classes / 'VanillaWorldBridge.class'
    if not compiled.is_file() or compiled.stat().st_mtime < source.stat().st_mtime:
        subprocess.run([javac, '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp, '-d', str(classes), str(source)], check=True, stdout=sys.stderr)
    return [java, '-Xms128M', '-Xmx2G', '-Djava.awt.headless=true', '-Dlog4j2.formatMsgNoLookups=true', '-cp', str(classes)+os.pathsep+cp, 'VanillaWorldBridge', str(world), str(seed)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar', type=Path, default=Path(os.environ.get('APPDATA', str(Path.home()))) / '.minecraft/versions/1.8.9/1.8.9.jar')
    parser.add_argument('--world', required=True, type=Path)
    parser.add_argument('--seed', required=True, type=int)
    parser.add_argument('--stdio', action='store_true', help='Persistent stdin/stdout line protocol for Rust')
    parser.add_argument('--chunks', nargs='*', default=[], help='Chunk x,z pairs, e.g. 0,0 -1,2')
    parser.add_argument('--base', help='Hash a direct original unpopulated ChunkProviderGenerate result at x,z')
    args = parser.parse_args()
    world = args.world.resolve()
    allowed = (ROOT / '.local/vanilla-worlds').resolve()
    if not world.is_relative_to(allowed):
        raise RuntimeError('Generated world must be below project .local/vanilla-worlds (original saves stay read-only)')
    if not -(1 << 63) <= args.seed < (1 << 63):
        raise RuntimeError('Seed must fit a signed Java long')
    world.mkdir(parents=True, exist_ok=True)
    cmd = command(args.jar.resolve(), world, args.seed)
    if args.stdio:
        # Replace Python on POSIX; on Windows this waits while forwarding the
        # inherited pipe handles without introducing another protocol layer.
        raise SystemExit(subprocess.call(cmd, cwd=world))
    process = subprocess.Popen(cmd, cwd=world, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8')
    try:
        line = process.stdout.readline().strip()
        print(line, flush=True)
        if not line.startswith('READY '):
            raise RuntimeError('Vanilla bridge failed to initialize')
        for request in ([('LOAD ' + ' '.join(args.chunks))] if args.chunks else []) + ([('BASE ' + args.base)] if args.base else []):
            process.stdin.write(request+'\n'); process.stdin.flush()
            line = process.stdout.readline().strip(); print(line, flush=True)
            if not line.startswith('OK '): raise RuntimeError('Vanilla bridge request failed')
        process.stdin.write('QUIT\n');process.stdin.flush()
        raise SystemExit(process.wait(timeout=120))
    finally:
        if process.poll() is None: process.kill();process.wait()


if __name__ == '__main__':
    main()
