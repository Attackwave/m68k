# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Retro-Entwickler, Demoszene-Programmierer und Hardware-Enthusiasten, die Software, Demos und Spiele für Motorola 68000–68060 basierte Systeme (Commodore Amiga 500/1200, Sega Mega Drive / Genesis, Atari ST / TT / Falcon und Bare-Metal 68k Boards) entwickeln. Zudem moderne Entwickler, die intuitive Sprachen wie Python nutzen möchten, um direkt nativen 68k-Maschinencode für Retro-Plattformen zu generieren.

## Product Purpose

`m68k Studio` ist eine vollständige, moderne All-in-One Entwicklungsumgebung (IDE) im VS Code-Design, die den gesamten Workflow der Retro-Programmierung vereint: von modernem Code-Editing (Monaco-Engine, Syntax-Highlighting, Autocomplete, Hover-Docs, LSP-Diagnostics), AOT-Python-Transpilierung und 2-Pass-Assemblierung über spezialisierte Retro-Hardware-Visualizer (Amiga Copperlist, Blitter Studio, Planar Bitplanes, Virtual ADF Floppy Manager, Memory Map) bis hin zum 1-Klick Emulator-Start (FS-UAE, WinUAE, BlastEm, Hatari).

## Positioning

Im Gegensatz zu isolierten Command-Line-Assemblern oder unhandlichen Retro-Emulatoren kombiniert `m68k Studio` modernste IDE-Ergonomie (VS Code Shell, Multi-File Tabs, Command Palette, hierarchischer Projektexplorer, globale Suche) mit tiefem Hardware-Verständnis der Zielsysteme (Cycle-Counting, Scanline-Budgeting, Custom-Chip-Register-Docs, Live-Disassembly und ADF/ROM-Erzeugung) direkt im Browser und als portable Desktop-App.

## Operating Context

- Schnelle Iterationszyklen: Schreiben von Python- oder ASM-Code, sofortiges Kompilieren (F7), automatische Fehleranalyse im Problems-Panel und direkter Testlauf im passenden Emulator (F5).
- Demoszene- & Spieleentwicklung: Präzise Abstimmung von Rasterstrahl-Effekten (Copper), Bitplane-Grafikkonvertierung (RGB444 / Planar) und Blitter-Minterm-Berechnungen.
- Multi-Plattform-Targeting: Nahtloser Wechsel zwischen Zielplattformen (Amiga 500 OCS, Amiga 1200 AGA, Sega Mega Drive, Atari ST, Bare Metal) über einheitliche Zielprofile.

## Capabilities and Constraints

- **Sprachen**: Pure Motorola 68000–68060 Assembly sowie Python mit m68k Hardware-Bibliotheken (`m68k.amiga.custom`, `m68k.megadrive.vdp`, etc.).
- **Assembler & Toolchain**: Hochoptimierter Rust 2-Pass Assembler (`m68k-asm`), Branch-Relaxation, Disassembler (`m68k-disasm`), Floppy Image Writer (`m68k-floppy`), LSP Language Server (`m68k-lsp`).
- **Hardware-Tools**:
  - 🧮 Amiga Blitter Studio (Minterm Equation Engine)
  - 🎨 Planar Bitplane Studio (RGB444 Farbquantisierer & ILBM/Raw Exporter)
  - 🌈 Copperlist Visualizer (PAL/NTSC CRT Rasterstrahl-Simulation)
  - 💾 Virtual Floppy (ADF) Manager mit OFS/FFS Unterstützung
  - 🗺️ Memory Map & Cycle Analyzer (Scanline- und Frame-Budget)
- **UI & UX**: VS Code Studio Layout mit Activity Bar, Primary Sidebar (Explorer, Search, Retro Tools, Debug), Multi-File Tabs, Breadcrumbs, Bottom Panel (Problems, Output, Disassembly, Hex-Dump), Status Bar und Command Palette (Ctrl+P / F1).

## Brand Commitments

- **Name**: `m68k Studio` / `m68k`
- **Design-Philosophie**: Professionelle, hochmoderne Dark-Studio-Ästhetik (inspiriert von VS Code) mit optionalem Amiga Workbench 1.3 Retro-Theme.
- **Tonalität**: Präzise, entwicklerfreundlich, technisch fundiert und reaktionsschnell.

## Evidence on Hand

- Vollständige Rust-Workspace-Architektur mit 756 bestandenen Unit- und Integrationstests (`m68k-core`, `m68k-asm`, `m68k-disasm`, `m68k-floppy`, `m68k-ide`, `m68k-lsp`).
- Integrierter Python-to-m68k Transpiler (`languages/python.rs`).
- Volle Suite an Projekt-Vorlagen für Amiga 500 (Python & ASM), Sega Mega Drive (Python & ASM) und Bare-Metal.

## Product Principles

1. **Zero-Friction Retro Development**: Ein Entwickler muss innerhalb von Sekunden vom ersten Code-Entwurf zum laufenden Ergebnis im Emulator kommen, ohne manuelle Toolchain-Konfiguration.
2. **First-Class Studio Ergonomics**: Volle Einhaltung von modernen IDE-Standards (Tastenkürzel, Kontextmenüs, Multi-File Tabs, Problems-Navigation, Command Palette).
3. **Deep Hardware Fidelity**: Alle hardwarenahen Berechnungen (Opcodes, Cycles, Rasterlines, Minterms, Bitplanes, Floppy-Blöcke) sind mathematisch und architektonisch exakt.

## Accessibility & Inclusion

- Klare visuelle Kontraste im Dark-Mode und Retro-Modus.
- Vollständige Tastaturbedienbarkeit aller Kernfunktionen über globale Tastenkombinationen (F1, F5, F7, Ctrl+P, Ctrl+B, Ctrl+J, Shift+Alt+F).
- Standardisierte Schriftarten (`JetBrains Mono`, `Fira Code`, System-Monospace) für ermüdungsfreies Coden.
