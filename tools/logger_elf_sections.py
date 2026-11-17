"""Read ELF32 section sizes for the coexistence study (stdlib only)."""
import argparse
import hashlib
import json
import struct
from pathlib import Path


def inspect(path):
    data = path.read_bytes()
    if data[:6] != b'\x7fELF\x01\x01':
        raise ValueError('expected little endian ELF32')
    offset = struct.unpack_from('<I', data, 32)[0]
    stride, count, names = struct.unpack_from('<HHH', data, 46)
    sections = [struct.unpack_from('<IIIIIIIIII', data, offset + i * stride) for i in range(count)]
    name_section = sections[names]
    strings = data[name_section[4]:name_section[4] + name_section[5]]
    result = {}
    for row in sections:
        if row[2] & 2:
            end = strings.find(b'\0', row[0])
            name = strings[row[0]:end].decode()
            result[name] = {'address': row[3], 'bytes': row[5], 'file_backed': row[1] != 8}
    return {'sha256': hashlib.sha256(data).hexdigest(), 'allocated_sections': result,
            'upper_remaining_bytes': 122880 - result['.upper']['bytes']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('elf', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    report = inspect(args.elf)
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
