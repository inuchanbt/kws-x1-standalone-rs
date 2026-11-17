"""Package/audit matching logger ELFs and run only offline flasher dry runs.

Build the current firmware, unpacker and host tools first. Output must be a
new directory. No USB discovery, device query, flash or erase is requested.
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('firmware', 'unpacker', 'packer', 'flasher', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--mismatched-firmware', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    for name in ('firmware', 'unpacker', 'packer', 'flasher'):
        path = getattr(args, name).resolve()
        if not path.is_file():
            parser.error(f'{name} must be an existing file')
        setattr(args, name, path)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    inputs = out / 'inputs'
    logs = out / 'logs'
    inputs.mkdir()
    logs.mkdir()
    copied = {}
    for name, filename in (('firmware', 'firmware.elf'), ('unpacker', 'unpacker.elf'),
                           ('packer', 'firmware-pack.exe'), ('flasher', 'flasher.exe')):
        copied[name] = inputs / filename
        shutil.copyfile(getattr(args, name), copied[name])
        if sha256(copied[name]) != sha256(getattr(args, name)):
            raise ValueError('input copy mismatch')
    report = dict(
        source_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        hardware_access=False,
        inputs={name: dict(source=str(getattr(args, name)), sha256=sha256(path))
                for name, path in copied.items()},
        commands=[], images={}, negative_checks={},
    )

    def run(label, role, arguments, success=True):
        if role == 'flasher' and ('--dry-run' not in arguments or '--info' in arguments):
            raise ValueError('only non-discovering flasher dry runs are permitted')
        command = [str(copied[role]), *map(str, arguments)]
        print(label, flush=True)
        result = subprocess.run(command, cwd=root, capture_output=True)
        (logs / (label + '.stdout')).write_bytes(result.stdout)
        (logs / (label + '.stderr')).write_bytes(result.stderr)
        report['commands'].append(dict(label=label, arguments=command, exit_code=result.returncode,
                                       expected_success=success))
        if (result.returncode == 0) != success:
            raise RuntimeError(f'{label}: unexpected exit {result.returncode}; inspect {logs}')
        return result

    def source(firmware=None):
        return ['--firmware', firmware or copied['firmware'], '--unpacker', copied['unpacker']]

    complete = out / 'firmware-complete.X1C'
    # A size rejection is evidence, not a reason to change features or fences.
    command = [str(copied['packer']), 'pack', '--stage', 'complete', *map(str, source()), '--out', str(complete)]
    print('complete-pack-attempt', flush=True)
    result = subprocess.run(command, cwd=root, capture_output=True)
    (logs / 'complete-pack.stdout').write_bytes(result.stdout)
    (logs / 'complete-pack.stderr').write_bytes(result.stderr)
    report['commands'].append(dict(label='complete-pack', arguments=command, exit_code=result.returncode))
    if result.returncode == 0:
        stages = ['complete']
        report['complete'] = dict(created=True)
    else:
        reason = result.stderr.decode('utf-8', errors='replace').strip()
        match = re.search(r'exceeds the application space by (\d+) bytes', reason)
        if not match or complete.exists():
            raise RuntimeError('complete package failed for a reason other than the capacity check')
        report['complete'] = dict(created=False, excess_bytes=int(match[1]), content_budget_bytes=209916,
                                  reason=reason)
        stages = ['upper', 'main']

    for stage in stages:
        filename = 'firmware-upper.X1C' if stage == 'upper' else ('firmware.X1C' if stage == 'main' else complete.name)
        image = out / filename
        if stage != 'complete':
            result = run(stage + '-pack', 'packer', ['pack', '--stage', stage, *source(), '--out', image])
        packed = json.loads(result.stdout)
        digest = sha256(image)
        if packed['sha256'] != digest:
            raise ValueError('package hash mismatch')
        audited = run(stage + '-audit', 'packer', ['audit', '--stage', stage, *source(),
                      '--image', image, '--confirm-sha256', digest])
        audit = json.loads(audited.stdout)
        dry = run(stage + '-dry-run', 'flasher', ['--dry-run', '--image', image, '--confirm-sha256', digest])
        if b'no device was discovered or opened' not in dry.stderr + dry.stdout:
            raise ValueError('missing offline dry-run confirmation')
        if audit['erase_envelope'] != [0x10000, 0x44000]:
            raise ValueError('unexpected bootloader erase envelope')
        report['images'][stage] = dict(filename=filename, file_bytes=image.stat().st_size,
                                       sha256=digest, package=packed, audit=audit, dry_run_passed=True)

    # All negative executions also use the dry-run branch, or the offline packer.
    chosen = report['images'][stages[-1]]
    image = out / chosen['filename']
    failed = run('wrong-hash', 'flasher', ['--dry-run', '--image', image, '--confirm-sha256', '0' * 64], False)
    if b'Image SHA-256' not in failed.stderr + failed.stdout:
        raise ValueError('wrong-hash check failed for an unexpected reason')
    report['negative_checks']['wrong_confirmed_hash_rejected'] = True
    negative = out / 'negative-tests'
    negative.mkdir()
    damaged = negative / 'corrupt-prefix.X1C'
    data = bytearray(image.read_bytes())
    data[0] ^= 1
    damaged.write_bytes(data)
    failed = run('corrupt-image', 'flasher', ['--dry-run', '--image', damaged,
                 '--confirm-sha256', sha256(damaged)], False)
    if b'Invalid container' not in failed.stderr + failed.stdout:
        raise ValueError('corruption check failed for an unexpected reason')
    report['negative_checks']['corrupt_image_with_matching_hash_rejected'] = True
    if args.mismatched_firmware:
        other = negative / 'mismatched-firmware.elf'
        shutil.copyfile(args.mismatched_firmware, other)
        failed = run('mismatched-elf', 'packer', ['audit', '--stage', stages[-1], *source(other),
                     '--image', image, '--confirm-sha256', chosen['sha256']], False)
        if b'canonical layout' not in failed.stderr + failed.stdout:
            raise ValueError('mismatched-ELF check failed for an unexpected reason')
        report['negative_checks']['different_firmware_elf_rejected'] = True
    for name, path in copied.items():
        if sha256(path) != report['inputs'][name]['sha256'] or sha256(getattr(args, name)) != sha256(path):
            raise ValueError('input modified during audit')
    for value in report['images'].values():
        if sha256(out / value['filename']) != value['sha256']:
            raise ValueError('package modified during negative tests')
    sums = [f"{v['sha256']}  {v['filename']}" for v in report['images'].values()]
    sums += [f"{sha256(path)}  inputs/{path.name}" for path in copied.values()]
    (out / 'SHA256SUMS').write_text('\n'.join(sums) + '\n', encoding='utf-8')
    (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(dict(output=str(out), complete=report['complete'],
                          images={k:dict(filename=v['filename'], sha256=v['sha256']) for k,v in report['images'].items()},
                          negative_checks=report['negative_checks'], hardware_access=False), indent=2))


if __name__ == '__main__':
    main()
