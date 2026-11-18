"""Remap the user's local 1.8.9 jar and decompile it into ignored .local/reference.

No Minecraft jar or source is downloaded. Tool artifacts and MCP name mappings
come from Maven Central / Forge Maven. Original game files remain untouched.
"""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / '.local' / 'tools'
REF = ROOT / '.local' / 'reference'
ARTIFACTS = {
    'mcp-1.8.9-srg.zip': 'https://maven.minecraftforge.net/de/oceanlabs/mcp/mcp/1.8.9/mcp-1.8.9-srg.zip',
    'mcp_stable-22-1.8.9.zip': 'https://maven.minecraftforge.net/de/oceanlabs/mcp/mcp_stable/22-1.8.9/mcp_stable-22-1.8.9.zip',
    'SpecialSource-1.11.4-shaded.jar': 'https://repo.maven.apache.org/maven2/net/md-5/SpecialSource/1.11.4/SpecialSource-1.11.4-shaded.jar',
    'cfr-0.152.jar': 'https://repo.maven.apache.org/maven2/org/benf/cfr/0.152/cfr-0.152.jar',
}

def download(name, url):
    target = TOOLS / name
    if not target.exists():
        print('Downloading', name, flush=True)
        temporary = target.with_suffix(target.suffix + '.part')
        request = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0 Ferrum189-reference/0.1'})
        with urllib.request.urlopen(request, timeout=90) as source, temporary.open('wb') as out:
            while chunk := source.read(1024 * 1024):
                out.write(chunk)
        temporary.replace(target)
    # Verify repository-provided SHA-1 when available, record SHA-256 below.
    try:
        request = urllib.request.Request(url + '.sha1', headers={'User-Agent': 'Mozilla/5.0 Ferrum189-reference/0.1'})
        expected = urllib.request.urlopen(request, timeout=20).read().decode().strip().split()[0]
    except Exception:
        expected = None
    if expected and re.fullmatch(r'[a-fA-F0-9]{40}', expected):
        actual = hashlib.sha1(target.read_bytes()).hexdigest()
        if actual.lower() != expected.lower():
            raise RuntimeError('Checksum mismatch: ' + name)
    return target

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--jar', type=Path, default=Path(os.environ.get('APPDATA', str(Path.home()))) / '.minecraft/versions/1.8.9/1.8.9.jar')
    parser.add_argument('--skip-decompile', action='store_true')
    args = parser.parse_args()
    if not args.jar.is_file():
        raise SystemExit('Local 1.8.9 jar not found: ' + str(args.jar))
    TOOLS.mkdir(parents=True, exist_ok=True)
    REF.mkdir(parents=True, exist_ok=True)
    paths = {name: download(name, url) for name, url in ARTIFACTS.items()}
    with zipfile.ZipFile(paths['mcp_stable-22-1.8.9.zip']) as archive:
        names = {}
        for filename in ['fields.csv', 'methods.csv', 'params.csv']:
            with archive.open(filename) as file:
                for row in csv.DictReader(io.TextIOWrapper(file, encoding='utf-8')):
                    names[row.get('searge', row.get('param'))] = row['name']
    with zipfile.ZipFile(paths['mcp-1.8.9-srg.zip']) as archive:
        srg = archive.read('joined.srg').decode()
    # Remap members in the bytecode too: inherited/overridden member names are
    # resolved by SpecialSource, rather than just replacing obfuscated strings.
    def named(member):
        owner, sep, name = member.rpartition('/')
        return owner + sep + names.get(name, name)
    output = []
    for line in srg.splitlines():
        parts = line.split()
        if parts[0] == 'FD:':
            parts[2] = named(parts[2])
        elif parts[0] == 'MD:':
            parts[3] = named(parts[3])
        output.append(' '.join(parts))
    mapping = REF / 'notch-to-mcp.srg'
    mapping.write_text('\n'.join(output) + '\n', encoding='utf-8')
    mapped = REF / '1.8.9-named.jar'
    print('Remapping local jar...', flush=True)
    with (REF / 'remap.log').open('w', encoding='utf-8') as log:
        subprocess.run(['java', '-Xmx2G', '-jar', str(paths['SpecialSource-1.11.4-shaded.jar']), '--in-jar', str(args.jar), '--out-jar', str(mapped), '--srg-in', str(mapping), '--kill-lvt'], stdout=log, stderr=subprocess.STDOUT, check=True)
    if not args.skip_decompile:
        print('Decompiling named jar (several minutes)...', flush=True)
        libraries = []
        version_json = args.jar.with_suffix('.json')
        minecraft_dir = args.jar.parent.parent.parent
        if version_json.is_file():
            version = json.loads(version_json.read_text(encoding='utf-8'))
            for library in version.get('libraries', []):
                artifact = library.get('downloads', {}).get('artifact', {})
                relative = artifact.get('path')
                if not relative:
                    coordinate = library.get('name', '').split(':')
                    if len(coordinate) >= 3:
                        group, name, version = coordinate[:3]
                        relative = f'{group.replace(".", "/")}/{name}/{version}/{name}-{version}.jar'
                if relative:
                    candidate = minecraft_dir / 'libraries' / relative
                    if candidate.is_file():
                        libraries.append(str(candidate))
        command = ['java', '-Xmx3G', '-jar', str(paths['cfr-0.152.jar']), str(mapped), '--outputdir', str(REF / 'java'), '--silent', 'true', '--caseinsensitivefs', 'true']
        if libraries:
            command += ['--extraclasspath', os.pathsep.join(libraries)]
        print(f'Using {len(libraries)} installed dependency jars for type resolution.', flush=True)
        with (REF / 'decompile.log').open('w', encoding='utf-8') as log:
            subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    java_files = list((REF / 'java').rglob('*.java'))
    errors = []
    unresolved = []
    for file in java_files:
        text = file.read_text(encoding='utf-8', errors='replace')
        if 'Could not decompile' in text:
            errors.append(str(file.relative_to(REF)))
        if 'Could not load the following classes' in text:
            unresolved.append(str(file.relative_to(REF)))
    manifest = {'input_jar': str(args.jar.resolve()), 'input_sha256': hashlib.sha256(args.jar.read_bytes()).hexdigest(), 'sources': len(java_files), 'files_with_decompiler_errors': errors, 'files_with_unresolved_dependencies': unresolved, 'artifacts': {name: {'url': ARTIFACTS[name], 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()} for name, path in paths.items()}, 'note': 'MCP names improve readability. Local names, comments and every original source construct cannot be recovered. This is research output, not a buildable original source release.'}
    (REF / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    print(f'Done: {len(java_files)} Java files, {len(errors)} files with decompiler errors, {len(unresolved)} with unresolved dependencies. See {REF}', flush=True)

if __name__ == '__main__':
    main()
