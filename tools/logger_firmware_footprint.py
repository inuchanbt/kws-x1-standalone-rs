"""Compare saved ARM ELF load bytes, static RAM and linker stack reservation.

Standard library only. Does not package, flash, or contact a device.
"""
import argparse
import json
import struct
from pathlib import Path

from logger_elf_sections import inspect


def footprint(path):
    report = inspect(path)
    data = path.read_bytes()
    phoff, shoff = struct.unpack_from('<II', data, 28)
    phsize, phnum, shsize, shnum = struct.unpack_from('<HHHH', data, 42)
    sections = [struct.unpack_from('<IIIIIIIIII', data, shoff + i * shsize)
                for i in range(shnum)]
    segments = []
    for i in range(phnum):
        kind, offset, virtual, physical, filesz, memsz, flags, align = struct.unpack_from(
            '<IIIIIIII', data, phoff + i * phsize)
        if kind == 1:
            segments.append(dict(virtual_address=virtual, load_address=physical,
                                 file_bytes=filesz, memory_bytes=memsz))
    symbols = {}
    bss_symbols = []
    for section in sections:
        if section[1] != 2:  # SHT_SYMTAB
            continue
        names = sections[section[6]]
        strings = data[names[4]:names[4] + names[5]]
        for offset in range(section[4], section[4] + section[5], section[9]):
            name, value, size, info, other, index = struct.unpack_from('<IIIBBH', data, offset)
            end = strings.find(b'\0', name)
            label = strings[name:end].decode('utf-8')
            if label in ('_stack_start', '_stack_end', '__sheap', '__sdata', '__edata',
                         '__sbss', '__ebss'):
                symbols[label] = value
            if size and index < shnum and sections[index][1] == 8 and info & 15 == 1:
                bss_symbols.append(dict(name=label, bytes=size, address=value))
    main = [s for s in segments if s['file_bytes'] and 0x10000 <= s['load_address'] < 0x433fc]
    upper = [s for s in segments if s['file_bytes'] and 0x44000 <= s['load_address'] < 0x62000]
    for segment in main + upper:
        limit = 0x433fc if segment in main else 0x62000
        if segment['load_address'] + segment['file_bytes'] > limit:
            raise ValueError('flash load image exceeds linker region')
    allocated = report['allocated_sections']
    main_ram = sum(allocated.get(s, {}).get('bytes', 0) for s in ('.data', '.bss', '.uninit'))
    ram2 = allocated['.sram2']['bytes']
    main_end = max(s['load_address'] + s['file_bytes'] for s in main)
    report.update(
        load_segments=segments,
        linker_symbols=symbols,
        main_flash_load_bytes=sum(s['file_bytes'] for s in main),
        upper_flash_load_bytes=sum(s['file_bytes'] for s in upper),
        total_flash_load_bytes=sum(s['file_bytes'] for s in main + upper),
        main_flash_span_bytes=main_end - 0x10000,
        main_flash_remaining_bytes=0x433fc - main_end,
        main_static_ram_bytes=main_ram,
        sram2_static_bytes=ram2,
        total_static_ram_bytes=main_ram + ram2,
        linker_stack_reserved_bytes=symbols['_stack_start'] - symbols['_stack_end'],
        largest_noload_symbols=sorted(bss_symbols, key=lambda s: s['bytes'], reverse=True)[:12],
    )
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--elf', action='append', required=True, help='LABEL=path/to/firmware.elf')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    reports = {}
    for entry in args.elf:
        label, path = entry.split('=', 1)
        if label in reports:
            parser.error('duplicate ELF label')
        reports[label] = footprint(Path(path))
    result = dict(builds=reports, hardware_tested=False,
                  note='ELF static allocation and load bytes; not runtime stack high-water or an update package.')
    if 'full-logger' in reports and 'selective-logger' in reports:
        base, selected = reports['full-logger'], reports['selective-logger']
        result['selective_minus_full'] = {
            key: selected[key] - base[key] for key in (
                'main_flash_load_bytes', 'upper_flash_load_bytes', 'total_flash_load_bytes',
                'main_flash_span_bytes', 'main_static_ram_bytes', 'sram2_static_bytes',
                'total_static_ram_bytes', 'linker_stack_reserved_bytes')}
        result['section_deltas'] = {key: selected['allocated_sections'][key]['bytes'] - value['bytes']
                                    for key, value in base['allocated_sections'].items()}
    args.out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: v for k, v in result.items() if k != 'builds'}, indent=2))


if __name__ == '__main__':
    main()
