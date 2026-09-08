#!/usr/bin/env python3
"""Package and verify the complete, installable release matrix using stdlib only."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
TARGETS = {
    'x86_64-unknown-linux-gnu': ('manylinux', 'x86_64'),
    'aarch64-unknown-linux-gnu': ('manylinux', 'aarch64'),
    'x86_64-unknown-linux-musl': ('musllinux_1_2', 'x86_64'),
    'aarch64-unknown-linux-musl': ('musllinux_1_2', 'aarch64'),
    'x86_64-pc-windows-msvc': ('win_amd64', ''),
    'x86_64-apple-darwin': ('macosx', 'x86_64'),
    'aarch64-apple-darwin': ('macosx', 'arm64'),
}


def wheels(directory):
    return sorted(directory.glob(f'outlook_cli_rs-{VERSION}-*.whl'))


def wheel_target(path):
    platform = path.stem.split('-')[-1]
    matches = [target for target, (prefix, arch) in TARGETS.items()
               if platform.startswith(prefix) and platform.endswith(arch)]
    if len(matches) != 1:
        raise ValueError(f'Unexpected wheel platform: {path.name}')
    return matches[0]


def executable(wheel):
    with zipfile.ZipFile(wheel) as archive:
        entries = [n for n in archive.namelist()
                   if n.endswith(('.data/scripts/outlook', '.data/scripts/outlook.exe'))]
        if len(entries) != 1:
            raise ValueError(f'Expected one outlook executable in {wheel}')
        return Path(entries[0]).name, archive.read(entries[0])


def smoke(directory):
    candidates = wheels(directory)
    if len(candidates) != 1:
        raise ValueError('Smoke test requires exactly one wheel')
    with tempfile.TemporaryDirectory(prefix='outlook-wheel-') as temporary:
        root = Path(temporary)
        env = dict(os.environ, UV_TOOL_DIR=str(root / 'tools'),
                   UV_TOOL_BIN_DIR=str(root / 'bin'), UV_CACHE_DIR=str(root / 'cache'),
                   UV_PYTHON_DOWNLOADS='never')
        subprocess.run(['uv', 'tool', 'install', '--no-build', '--no-index',
                        '--python', sys.executable, '--find-links', str(directory.resolve()),
                        f'outlook-cli-rs=={VERSION}'], env=env, check=True)
        binary = root / 'bin' / ('outlook.exe' if os.name == 'nt' else 'outlook')
        assert subprocess.check_output([binary, '--version'], text=True).strip() == f'outlook {VERSION}'
        schema = json.loads(subprocess.check_output([binary, 'schema'], text=True))
        assert 'mail draft send' in json.dumps(schema)
        # Also execute the standalone bytes from the wheel, without its install
        # tree. This catches accidental runtime dependence on bundled libraries.
        name, data = executable(candidates[0])
        standalone = root / name
        standalone.write_bytes(data)
        standalone.chmod(0o755)
        assert subprocess.check_output([standalone, '--version'], text=True).strip() == f'outlook {VERSION}'
    print(f'Installed and executed {candidates[0].name} with source builds disabled')


def package(directory, target):
    candidates = wheels(directory)
    if len(candidates) != 1 or wheel_target(candidates[0]) != target:
        raise ValueError('Expected one wheel matching the requested target')
    name, data = executable(candidates[0])
    stem = f'outlook-v{VERSION}-{target}'
    files = {name: data, 'LICENSE': (ROOT / 'LICENSE').read_bytes(),
             'README.md': (ROOT / 'README.md').read_bytes()}
    if target.endswith('windows-msvc'):
        path = directory / f'{stem}.zip'
        with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for filename, content in files.items():
                archive.writestr(filename, content)
    else:
        path = directory / f'{stem}.tar.gz'
        with tarfile.open(path, 'w:gz') as archive:
            for filename, content in files.items():
                info = tarfile.TarInfo(filename)
                info.size = len(content)
                info.mode = 0o755 if filename == name else 0o644
                archive.addfile(info, io.BytesIO(content))
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    path.with_name(path.name + '.sha256').write_text(f'{digest}  {path.name}\n')
    print(f'Packaged {path.name}')


def verify(directory, tag=None):
    if tag is not None and tag != f'v{VERSION}':
        raise ValueError(f'Tag must match Cargo.toml: v{VERSION}')
    candidates = wheels(directory)
    actual = [wheel_target(path) for path in candidates]
    if len(actual) != len(TARGETS) or set(actual) != set(TARGETS):
        raise ValueError(f'Incomplete or duplicate wheel matrix: {actual}')
    expected = set(candidates)
    for target in TARGETS:
        suffix = '.zip' if target.endswith('windows-msvc') else '.tar.gz'
        path = directory / f'outlook-v{VERSION}-{target}{suffix}'
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        checksum = path.with_name(path.name + '.sha256')
        if checksum.read_text().strip() != f'{digest}  {path.name}':
            raise ValueError(f'Checksum mismatch: {path.name}')
        expected.update((path, checksum))
    source = directory / f'outlook_cli_rs-{VERSION}.tar.gz'
    with tarfile.open(source) as archive:
        manifest = archive.extractfile(f'outlook_cli_rs-{VERSION}/Cargo.toml')
        if manifest is None or tomllib.loads(manifest.read().decode())['package']['version'] != VERSION:
            raise ValueError('Source package version mismatch')
    expected.add(source)
    unexpected = set(directory.iterdir()) - expected - {directory / 'SHA256SUMS'}
    if unexpected:
        raise ValueError(f'Unexpected release files: {unexpected}')
    lines = [f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n'
             for p in sorted(expected) if not p.name.endswith('.sha256')]
    (directory / 'SHA256SUMS').write_text(''.join(lines))
    print(f'Verified {len(candidates)} wheels, source package, and {len(TARGETS)} archives')


def notes():
    text = (ROOT / 'CHANGELOG.md').read_text()
    start = text.index(f'## {VERSION} - ')
    end = text.find('\n## ', start + 1)
    print(text[start:end if end != -1 else None].strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    for name in ('smoke', 'archive', 'verify'):
        command = sub.add_parser(name)
        command.add_argument('directory', type=Path)
        if name == 'archive':
            command.add_argument('--target', choices=TARGETS, required=True)
        if name == 'verify':
            command.add_argument('--require-tag')
    sub.add_parser('notes')
    args = parser.parse_args()
    if args.command == 'smoke':
        smoke(args.directory)
    elif args.command == 'archive':
        package(args.directory, args.target)
    elif args.command == 'verify':
        verify(args.directory, args.require_tag)
    else:
        notes()


if __name__ == '__main__':
    main()
