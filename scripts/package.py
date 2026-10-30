#!/usr/bin/env python3
"""Package prebuilt UI and headless agent. Never registers services or changes OS permissions."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import plistlib
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
LOCAL_CONFIG = ROOT / '.tools/package-local.json'


def sign(path, identity):
    subprocess.run(['codesign', '--force', '--timestamp=none', '--sign', identity, str(path)], check=True)
    subprocess.run(['codesign', '--verify', '--strict', str(path)], check=True)


def executable(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    destination.chmod(0o755)


def package_mac(dist, identity):
    ui = ROOT / 'target/release/mk'
    agent = ROOT / 'target/release/mk-agent'
    if not ui.exists() or not agent.exists():
        raise SystemExit('Build both mk and mk-agent before packaging macOS')
    # Assemble and sign a complete bundle before replacing the selected output.
    with tempfile.TemporaryDirectory(prefix='.mk-package-', dir=dist) as staging:
        app = Path(staging) / 'Coban-mod.app'
        contents = app / 'Contents'
        resources = contents / 'Resources'
        resources.mkdir(parents=True)
        executable(ui, contents / 'MacOS/Coban-mod')
        subprocess.run(['iconutil', '-c', 'icns', str(ROOT / 'assets/MK.iconset'), '-o', str(resources / 'MK.icns')], check=True)
        info = {
            'CFBundleExecutable': 'Coban-mod', 'CFBundleIdentifier': 'vn.baotruong.mk',
            'CFBundleName': 'Coban-mod', 'CFBundleDisplayName': 'Coban-mod', 'CFBundleIconFile': 'MK.icns',
            'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': VERSION,
            'CFBundleVersion': VERSION, 'LSMinimumSystemVersion': '12.0',
            'NSHighResolutionCapable': True, 'LSUIElement': True,
        }
        (contents / 'Info.plist').write_bytes(plistlib.dumps(info))
        helper = contents / 'Library/LoginItems/MK Agent.app'
        helper_contents = helper / 'Contents'
        executable(agent, helper_contents / 'MacOS/mk-agent')
        (helper_contents / 'Resources').mkdir()
        shutil.copy2(resources / 'MK.icns', helper_contents / 'Resources/MK.icns')
        helper_info = dict(info, CFBundleExecutable='mk-agent', CFBundleIdentifier='vn.baotruong.mk.agent',
                           CFBundleName='Coban-mod Agent', CFBundleDisplayName='Coban-mod Agent', LSUIElement=True,
                           NSInputMonitoringUsageDescription='Coban-mod Agent nhận các phím hành động từ bàn phím Coban đã chọn khi bạn bật dịch vụ. Không ghi lại nội dung gõ phím.')
        (helper_contents / 'Info.plist').write_bytes(plistlib.dumps(helper_info))
        sign(helper, identity)
        sign(app, identity)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
        destination = dist / 'Coban-mod.app'
        if destination.exists():
            previous = dist / 'Coban-mod.previous.app'
            if previous.exists():
                shutil.rmtree(previous)
            destination.rename(previous)
        app.rename(destination)
        print(destination)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--mac', action='store_true')
    parser.add_argument('--windows', action='store_true')
    parser.add_argument('--sign-identity')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    dist = args.out_dir.resolve()
    dist.mkdir(parents=True, exist_ok=True)
    if args.mac:
        config = json.loads(LOCAL_CONFIG.read_text()) if LOCAL_CONFIG.exists() else {}
        identity = args.sign_identity or os.environ.get('MK_CODESIGN_IDENTITY') or config.get('macos_signing_identity') or '-'
        package_mac(dist, identity)
        if identity == '-':
            print('Development bundle signed ad-hoc. Distribution requires a stable signing identity.')
    if args.windows:
        candidates = [ROOT / 'target/x86_64-pc-windows-gnu/release/mk.exe', ROOT / 'target/release/mk.exe']
        source = next((path for path in candidates if path.exists()), None)
        if source is None:
            raise SystemExit('Build Windows release first')
        # Portable distribution stays one executable; --agent runs in a separate process.
        shutil.copy2(source, dist / 'Coban-mod.exe')
        print(dist / 'Coban-mod.exe')
    artifacts = {}
    for name in ['Coban-mod.app/Contents/MacOS/Coban-mod', 'Coban-mod.app/Contents/Library/LoginItems/MK Agent.app/Contents/MacOS/mk-agent', 'Coban-mod.exe']:
        path = dist / name
        if path.exists():
            artifacts[name] = {'bytes': path.stat().st_size, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    (dist / 'manifest.json').write_text(json.dumps({'version': VERSION, 'artifacts': artifacts}, indent=2) + '\n')


if __name__ == '__main__':
    main()
