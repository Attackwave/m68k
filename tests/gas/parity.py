#!/usr/bin/env python3
"""Compare `m68k-asm --syntax gas` with GNU as + ld on a set of files.

    parity.py [--asm PATH] [--stubs] FILE.s...

With --stubs, every symbol the files use but none defines (libc, libgcc)
is defined as a label in one more file, given last to both sides.

The files are linked by both, in the given order; m68k-asm gets the last
file with `.reloc_table` appended to its .data, which ends the image. The image without the table
must equal the GNU image byte for byte, and the table must list exactly
the R_68K_32 relocations ld kept. Needs the GNU m68k binutils.

M68K_CPU (default 68000) selects the CPU for both assemblers.
"""

import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gnu_reference  # noqa: E402


def decode_table(table):
    first = int.from_bytes(table[:4], "big")
    if first == 0:
        return [], 4
    fixups, at, i = [first], first, 4
    while table[i] != 0:
        if table[i] == 1:
            at += 254
        else:
            at += table[i]
            fixups.append(at)
        i += 1
    return fixups, i + 1


def write_stubs(work, sources):
    defined, used = set(), set()
    for i, src in enumerate(sources):
        obj = os.path.join(work, f"stub{i}.o")
        gnu_reference.run(gnu_reference.PREFIX + "as", "-m" + gnu_reference.CPU, "-o", obj, src)
        for line in gnu_reference.output(gnu_reference.PREFIX + "nm", obj).splitlines():
            fields = line.split()
            if fields[-2] == "U":
                used.add(fields[-1])
            elif fields[-2].isupper():
                defined.add(fields[-1])
    path = os.path.join(work, "stubs.s")
    with open(path, "w") as f:
        f.write("\t.text\n")
        for name in sorted(used - defined):
            f.write(f"\t.globl {name}\n{name}:\n")
    return path


def main():
    args = sys.argv[1:]
    asm = "m68k-asm"
    if args[:1] == ["--asm"]:
        asm, args = args[1], args[2:]
    stubs = args[:1] == ["--stubs"]
    if stubs:
        args = args[1:]
    if not args:
        sys.exit(__doc__)
    with tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(args[0]))) as work:
        if stubs:
            args = args + [write_stubs(work, args)]
        gnu_reference.reference(os.path.join(work, "ref"), args)
        with open(os.path.join(work, "ref", "image.bin"), "rb") as f:
            ref = f.read()
        with open(os.path.join(work, "ref", "fixups.txt")) as f:
            ref_fixups = [int(x, 16) for x in f.read().split()]
        plain = os.path.join(work, "plain.bin")
        subprocess.run([asm, "--syntax", "gas", "--cpu", gnu_reference.CPU, "-o", plain, *args], check=True)
        with open(plain, "rb") as f:
            ours = f.read()
        # The table goes at the end of the last file's .data, which a file
        # of its own would move (its empty sections are aligned too). It
        # moves BSS, so this image is read for its table only.
        last = os.path.join(work, "last-" + os.path.basename(args[-1]))
        with open(args[-1]) as f, open(last, "w") as g:
            g.write(f.read() + "\n\t.data\n\t.reloc_table\n")
        out = os.path.join(work, "out.bin")
        subprocess.run([asm, "--syntax", "gas", "--cpu", gnu_reference.CPU, "-o", out, *args[:-1], last], check=True)
        with open(out, "rb") as f:
            with_table = f.read()
    fixups, table_len = decode_table(with_table[len(ref):])
    ok = True
    if ours != ref or len(ref) + table_len != len(with_table):
        diff = next((i for i in range(min(len(ref), len(ours))) if ref[i] != ours[i]), None)
        print(f"IMAGE DIFFERS: gnu {len(ref)} bytes, m68k-asm {len(ours)} bytes, first difference at {diff}")
        ok = False
    if fixups != ref_fixups:
        missing = sorted(set(ref_fixups) - set(fixups))[:10]
        extra = sorted(set(fixups) - set(ref_fixups))[:10]
        print(f"FIXUPS DIFFER: gnu {len(ref_fixups)}, m68k-asm {len(fixups)}; "
              f"missing {[hex(x) for x in missing]} extra {[hex(x) for x in extra]}")
        ok = False
    print(f"{'OK' if ok else 'FAIL'}: {len(args)} files, image {len(ref)} bytes, "
          f"{len(ref_fixups)} fixups")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
