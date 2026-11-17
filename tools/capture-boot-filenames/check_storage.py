"""Test the current capture writer in a RAM flash model, without device access.

All generated Rust sources/logs/executables stay in a new output directory.
Uses the host Rust toolchain; on Windows set the same GNU host linker flags as
for normal firmware-core tests. Optional backup input is read only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def run(command, output, name, env):
    result = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, encoding='utf-8', errors='replace')
    (output / name).write_text(result.stdout, encoding='utf-8')
    if result.returncode:
        print(result.stdout[-7000:])
        raise RuntimeError(f'{name}: exit {result.returncode}')
    return result.stdout


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--target-dir', type=Path, default=ROOT/'target/capture-boot-filenames-host')
    ap.add_argument('--out-dir', type=Path,
                    default=ROOT/('build/capture-writer-model-'+uuid.uuid4().hex[:8]))
    ap.add_argument('--backup', type=Path, help='Read-only private six-sector capture backup')
    args = ap.parse_args()
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = ROOT/'firmware/src/screenshot.rs'
    text = source.read_text(encoding='utf-8')
    old = 'core::slice::from_raw_parts(address as *const u8, size)'
    if text.count(old) != 1 or text.count('async fn save(size:') != 1:
        raise ValueError('Writer shape changed; review the adapter')
    adapter = text.replace('//!', '//').replace('#[embassy_executor::task]', '')
    adapter = adapter.replace(old, 'crate::flash::bytes(address, size)')
    adapter = adapter.replace('async fn save(size:', 'pub async fn save(size:')
    adapter += '\npub fn simulate_boot() { BOOT_GROUP.store(0, Ordering::Relaxed); }\n'
    (output/'storage_adapter.rs').write_text(adapter, encoding='utf-8')
    harness = (HERE/'flash_model.rs').read_text(encoding='utf-8')
    harness += (HERE/'storage_tests.rs').read_text(encoding='utf-8')
    (output/'storage_tests.rs').write_text(harness, encoding='utf-8')
    env = dict(os.environ)
    if args.backup:
        env['KWS_CAPTURE_BACKUP'] = str(args.backup.resolve())
    else:
        env.pop('KWS_CAPTURE_BACKUP', None)
    cargo = run(['cargo','build','-p','firmware-core','--lib','--locked','--offline',
                 '--target-dir',str(args.target_dir.resolve()),'--message-format=json'],
                output,'cargo.log',env)
    library = None
    for line in cargo.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get('reason') == 'compiler-artifact' and event['target']['name'] == 'firmware_core':
            for filename in event['filenames']:
                if filename.endswith('.rlib'):
                    library = Path(filename)
    if library is None:
        raise ValueError('Host firmware_core artifact not found')
    encoded = env.get('CARGO_ENCODED_RUSTFLAGS')
    flags = encoded.split('\x1f') if encoded else shlex.split(env.get('RUSTFLAGS',''))
    executable = output/('storage-tests.exe' if os.name=='nt' else 'storage-tests')
    dependencies = library.parent/'deps'
    if not dependencies.is_dir():
        dependencies = library.parent
    command = ['rustc','--edition=2024','--test',str(output/'storage_tests.rs'),
               '--extern','firmware_core='+str(library),'-L','dependency='+str(dependencies),
               '-o',str(executable),*flags]
    run(command,output,'rustc.log',env)
    result = run([str(executable),'--test-threads=1','--nocapture'],output,'tests.log',env)
    receipt = dict(writer_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                   hardware_access=False,backup_check_requested=bool(args.backup),
                   commands={'rustc':command},result=result)
    (output/'result.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    print(result)
    print('Saved model evidence:',output)


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8',errors='backslashreplace')
    main()
