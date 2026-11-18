from pathlib import Path
import json
import subprocess

root = Path(__file__).resolve().parents[1]
output = root / 'dist/github-release'
output.mkdir(parents=True, exist_ok=True)
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--locked', '--format-version', '1',
    '--filter-platform', 'x86_64-pc-windows-msvc',
    '--manifest-path', str(root / 'desktop/src-tauri/Cargo.toml'),
], encoding='utf-8'))
resolved = {node['id'] for node in metadata['resolve']['nodes']}
texts = ['Resolved Windows Rust dependency notices, including build dependencies.\n']
inventory = []
missing = []
for package in sorted(metadata['packages'], key=lambda item: (item['name'], item['version'])):
    if package['id'] not in resolved or package['source'] is None:
        continue
    directory = Path(package['manifest_path']).parent
    files = sorted(file for file in directory.iterdir() if file.is_file() and file.name.lower().startswith(('license', 'copying', 'notice')))
    if package.get('license_file'):
        declared = directory / package['license_file']
        if declared.is_file() and declared not in files:
            files.append(declared)
    if not files:
        for subdir in ('LICENSES', 'licenses'):
            files.extend(file for file in (directory / subdir).rglob('*') if file.is_file())
    supplement = root / 'third-party/license-supplements' / f"{package['name']}-{package['version']}.txt"
    if not files and supplement.exists():
        files = [supplement]
    if not files:
        missing.append(package['name'] + ' ' + package['version'])
    inventory.append({'name': package['name'], 'version': package['version'], 'license': package['license'], 'repository': package['repository'], 'sourceArchive': f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download"})
    for file in files:
        texts.append(f"\n{'=' * 72}\n{package['name']} {package['version']} | {package['license']} | {file.name}\n{'=' * 72}\n" + file.read_text(encoding='utf-8-sig') + '\n')
(output / 'RUST-LICENSES.txt').write_text(''.join(texts), encoding='utf-8')
(output / 'dependency-licenses.json').write_text(json.dumps(inventory, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps({'packages': len(inventory), 'missingLicenseTexts': missing}))
if missing:
    raise SystemExit('Add exact-version upstream license supplements before publishing.')
