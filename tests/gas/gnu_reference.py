#!/usr/bin/env python3
"""Build the reference for `m68k-asm --syntax gas` with the GNU tools.

Every file is assembled on its own with `m68k-linux-gnu-as -m68000`
(`-m$M68K_CPU` when that is set),
the objects are linked with a script that gives the layout m68k-asm
produces (text, read-only data, constructor and destructor arrays and
data stored from 0, BSS behind it), and `objcopy -O binary` writes the
image. The fixups are the R_68K_32 relocations `ld --emit-relocs` keeps
for the stored sections, except those against a weak symbol nothing
defines: it stays 0, which a loader must not move.

    gnu_reference.py OUT_DIR FILE.s...

writes OUT_DIR/image.bin, OUT_DIR/fixups.txt (one hex offset per line)
and OUT_DIR/symbols.txt (`__image_end`, `__bss_start`, `__bss_end`).

    gnu_reference.py --regen-fixtures

rebuilds every fixture set under tests/gas/fixtures/ (each directory's
`files.txt` lists its sources in link order, an optional `cpu.txt` the
CPU, 68000 without it).
"""

import os
import subprocess
import sys
import tempfile

PREFIX = os.environ.get("M68K_GNU_PREFIX", "m68k-linux-gnu-")
# The CPU both assemblers target: `-m<cpu>` for GNU as, `--cpu` for m68k-asm.
CPU = os.environ.get("M68K_CPU", "68000")
# readelf and nm are parsed: their messages must not be translated.
os.environ["LC_ALL"] = "C"

LINKER_SCRIPT = """\
SECTIONS {
  . = 0;
  .image : {
    *(.text .text.*)
    *(.rodata .rodata.*)
  }
  .init_array : {
    __init_array_start = .;
    KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
    KEEP(*(.init_array))
    __init_array_end = .;
  }
  .fini_array : {
    __fini_array_start = .;
    KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*)))
    KEEP(*(.fini_array))
    __fini_array_end = .;
  }
  /* At `.` itself: the data sections align one by one. */
  .data . : {
    *(.data .data.*)
    __image_end = .;
  }
  __bss_start = .;
  .bss : { *(.bss .bss.*) *(COMMON) }
  __bss_end = .;
  /DISCARD/ : { *(.comment) *(.note.*) *(.eh_frame) }
}
"""


def run(*cmd):
    subprocess.run(cmd, check=True)


def output(*cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def reference(out_dir, sources, cpu=CPU):
    os.makedirs(out_dir, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=out_dir) as work:
        objects = []
        for i, src in enumerate(sources):
            obj = os.path.join(work, f"{i:03d}.o")
            run(PREFIX + "as", "-m" + cpu, "-o", obj, src)
            objects.append(obj)
        script = os.path.join(work, "image.ld")
        with open(script, "w") as f:
            f.write(LINKER_SCRIPT)
        elf = os.path.join(work, "image.elf")
        run(PREFIX + "ld", "-T", script, "--emit-relocs", "--no-warn-rwx-segments",
            "-e", "0", "-o", elf, *objects)
        stored = (".image", ".init_array", ".fini_array", ".data")
        run(PREFIX + "objcopy", "-O", "binary",
            *(x for name in stored for x in ("-j", name)),
            elf, os.path.join(out_dir, "image.bin"))

        undefined_weak = set()
        for line in output(PREFIX + "nm", elf).splitlines():
            fields = line.split()
            if len(fields) == 2 and fields[0] in ("w", "v"):
                undefined_weak.add(fields[1])

        fixups = []
        section = None
        for line in output(PREFIX + "readelf", "-rW", elf).splitlines():
            if line.startswith("Relocation section"):
                section = line.split("'")[1]
                continue
            fields = line.split()
            if (section not in tuple(".rela" + name for name in stored)
                    or len(fields) < 3 or not fields[2].startswith("R_68K")):
                continue
            kind = fields[2]
            if len(fields) > 4 and fields[4] in undefined_weak:
                continue
            if kind == "R_68K_32":
                fixups.append(int(fields[0], 16))
            elif not kind.startswith("R_68K_PC"):
                sys.exit(f"{kind} at {fields[0]}: not a 32-bit absolute relocation")
        with open(os.path.join(out_dir, "fixups.txt"), "w") as f:
            f.writelines(f"{x:x}\n" for x in sorted(fixups))

        symbols = {}
        for line in output(PREFIX + "nm", elf).splitlines():
            fields = line.split()
            # An undefined (weak) symbol has no value column.
            if len(fields) != 3:
                continue
            value, _, name = fields
            if name in ("__image_end", "__bss_start", "__bss_end"):
                symbols[name] = int(value, 16)
        with open(os.path.join(out_dir, "symbols.txt"), "w") as f:
            for name in ("__image_end", "__bss_start", "__bss_end"):
                f.write(f"{name} {symbols[name]:x}\n")


def regen_fixtures():
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
    for name in sorted(os.listdir(root)):
        fixture = os.path.join(root, name)
        listing = os.path.join(fixture, "files.txt")
        if not os.path.isfile(listing):
            continue
        with open(listing) as f:
            sources = [os.path.join(fixture, line.strip()) for line in f if line.strip()]
        cpu = "68000"
        if os.path.isfile(os.path.join(fixture, "cpu.txt")):
            with open(os.path.join(fixture, "cpu.txt")) as f:
                cpu = f.read().strip()
        reference(os.path.join(fixture, "expected"), sources, cpu)
        print(f"{name}: {len(sources)} file(s)")


def main():
    if sys.argv[1:] == ["--regen-fixtures"]:
        regen_fixtures()
    elif len(sys.argv) >= 3:
        reference(sys.argv[1], sys.argv[2:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
