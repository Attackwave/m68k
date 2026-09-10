---
name: m68k Studio
description: Precision Workbench for Motorola 68000-68060 and Retro Systems Development
colors:
  primary: "#3B82F6"
  primary-hover: "#2563EB"
  neutral-bg: "#0F1117"
  neutral-sidebar: "#161922"
  neutral-panel: "#1E222D"
  neutral-border: "#2A2F3D"
  neutral-hover: "#2D3344"
  neutral-text: "#E2E8F0"
  neutral-muted: "#94A3B8"
  statusbar-bg: "#007acc"
  retro-amiga-blue: "#0055AA"
  retro-amiga-orange: "#FF8800"
  success-emerald: "#10B981"
  warning-amber: "#F59E0B"
  danger-rose: "#EF4444"
typography:
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "0.02em"
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "14px"
    fontWeight: 700
    lineHeight: 1.3
    letterSpacing: "0.01em"
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "12px"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "0.02em"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, Monaco, Consolas, monospace"
    fontSize: "11px"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0.01em"
rounded:
  sm: "4px"
  md: "8px"
  lg: "12px"
  xl: "16px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
    padding: "6px 14px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-panel:
    backgroundColor: "{colors.neutral-panel}"
    textColor: "{colors.neutral-text}"
    rounded: "{rounded.md}"
    padding: "6px 12px"
  button-success:
    backgroundColor: "{colors.success-emerald}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
    padding: "6px 14px"
  tab-active:
    backgroundColor: "{colors.neutral-panel}"
    textColor: "#ffffff"
    rounded: "0px"
    padding: "6px 12px"
---

# Design System: m68k Studio

## Overview

**Creative North Star: "The Precision Workbench"**

`m68k Studio` ist eine hochpräzise, fokussierte Dark-Studio-Umgebung für Entwickler, die Motorola 68000–68060 Maschinencode, Python-Transpilate und hardwarenahe Demos bauen. Das Design verbindet die funktionale Klarheit moderner IDEs (VS Code) mit der unverwechselbaren Ästhetik ingenieurmäßiger Retro-Hardware-Werkzeuge.

Die Oberfläche setzt auf maximale Informationsdichte ohne visuelles Rauschen: tiefe, unaufdringliche Graphit- und Obsidianschichten lassen den Code und die farbcodierten Hardware-Visualizer (Copperlists, Bitplanes, Register) als primäre Leuchtquellen im Raum stehen.

**Key Characteristics:**
- **Tonal Layering**: Konsequente räumliche Schichtung über vier dunkle Helligkeitsstufen statt unruhiger Schlagschatten.
- **Micro-Precision Monospace**: Präzise Ausrichtung von Opcodes, Hex-Dumps, Cycles und Registern in gestochen scharfer Festbreitenschrift.
- **Restrained Vibrant Accents**: Reines Cobalt-Blau (`#3B82F6`) und Horizon-Blau (`#007acc`) führen den Blick; Retro-Farben (Amiga-Orange/Blau) markieren Plattform-Spezifika.
- **Tactile Density**: Kompakte Bedienelemente (8px/12px Rhythmus, 4px/8px Radien) für maximalen Code-Editor- und Tool-Raum.

## Colors

Die Farbwelt von m68k Studio ist dunkel, tief und neutral, sodass Syntax-Highlighting und Hardware-Pixeldaten im Fokus stehen.

### Primary
- **Electric Cobalt Blue** (`#3B82F6`): Primäre Aktionsfarbe für Build-Buttons, aktive Tabs, Selektionsrahmen und Tastaturfokus.
- **Deep Royal Cobalt** (`#2563EB`): Hover- und Active-Status für primäre Interaktionselemente.
- **Horizon Status Blue** (`#007acc`): Markante Fußleiste im VS Code-Stil als visuelles Fundament der IDE.

### Secondary
- **Emerald Green** (`#10B981`): Ausführungs- und Erfolgsstatus (`▶ Run (F5)`, erfolgreiche Assemblierung, bootfähige Disketten).
- **Amber Warning** (`#F59E0B`): Compiler-Warnungen, Main-File-Badges und Scanline-Budget-Indikatoren.
- **Rose Error** (`#EF4444`): Syntaxfehler, Assembler-Abbrüche und destruktive Aktionen (Datei löschen).

### Tertiary (Retro Palette)
- **Amiga Copper Orange** (`#FF8800`): Signaturfarbe für Amiga-Rasterbars und spezifische OCS/AGA-Features.
- **Amiga Workbench Blue** (`#0055AA`): Nostalgische Akzentfarbe im Amiga-Retro-Theme.

### Neutral
- **Obsidian Canvas** (`#0F1117`): Basis-Hintergrund des Hauptarbeitsbereichs und Editors.
- **Midnight Slate** (`#161922`): Hintergrund für Activity Bar, Primary Sidebar und Panels.
- **Steel Graphite** (`#1E222D`): Panel-, Card- und Dialog-Oberflächen.
- **Muted Steel Border** (`#2A2F3D`): 1px feine Grenzlinie zwischen Arbeitsbereichen und Tabs.
- **Graphite Hover** (`#2D3344`): Interaktiver Hover-Zustand für Buttons und Listenzeilen.
- **Chalk White** (`#E2E8F0`): Primärer Text und Werte mit hohem Kontrast.
- **Cool Fog Gray** (`#94A3B8`): Sekundäre Metadaten, Labels, Shortcuts und Hinweise.

### Named Rules
**The Emitted Light Rule.** Das Studio selbst ist dunkel und ruhig; Farbe leuchtet nur dort auf, wo der Code, die Hardware (Copper/Raster) oder der Compiler-Zustand direkte Information ausgibt.

**The Functional Accent Rule.** Primärblau wird auf weniger als 5% der Bildschirmfläche verwendet. Jeder blaue Pixel signalisiert unmittelbare Interaktivität oder aktiven Fokus.

## Typography

**Display / UI Font:** `-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif` (mit System-Fallbacks)
**Code / Monospace Font:** `"JetBrains Mono", "Fira Code", Consolas, monospace` (mit nativer Ligaturunterstützung)
**Retro Font:** `"Topaz Plus", "Courier New", monospace`

**Character:** Technisch präzise, raumsparend und ermüdungsfrei für mehrstündige Coding-Sessions.

### Hierarchy
- **Display** (Bold 700, 16px, Line-Height 1.2): TitleBar-Brand und modale Dialog-Titel.
- **Headline** (Bold 700, 14px, Line-Height 1.3): Bereichsüberschriften, Wizard-Karten und Panel-Header.
- **Title** (Semi-Bold 600, 12px, Line-Height 1.3): Tab-Titel, Menüeinträge, Kategorien und Tool-Buttons.
- **Body** (Regular 400, 12px, Line-Height 1.5): Fließtext, Einstellungs-Beschreibungen und Toast-Nachrichten.
- **Label / Code** (Medium 500, 11px, Line-Height 1.4): Quellcode, Opcodes, Hex-Werte, Dateipfade und Tastatur-Shortcuts.

### Named Rules
**The Strict Alignment Rule.** Alle Zahlen, Adressen (`$0000`), Register (`D0-D7`, `A0-A7`) und Zyklenwerte werden ausnahmslos in Monospace gesetzt, um tabellarische Lesbarkeit ohne Versatz zu garantieren.

## Layout

Das Studio basiert auf dem bewährten, hocheffizienten Drei-Zonen-Raster:

1. **TitleBar (36px Höhe)**: Menüs (`Datei`, `Bearbeiten`, `Ansicht`, `Ausführen`), globales Suchfeld (`Ctrl+P`) und Profilsteuerung.
2. **Workbench-Bereich (flexibel, responsive)**:
   - **Activity Bar (48px Breite)**: Schmale Icon-Leiste am linken Rand.
   - **Primary Sidebar (260px Breite, einklappbar via `Ctrl+B`)**: Explorer, Suche, Retro Tools oder Run/Debug.
   - **Center Editor (flex-1)**: Multi-File Tableiste (32px), Breadcrumbs (24px) und Monaco-Editor.
   - **Bottom Panel (224px Höhe / maximierbar, einklappbar via `Ctrl+J`)**: Probleme, Build Output, Disassembly, Hex Dump.
3. **Status Bar (24px Höhe)**: Fest verankerte Informationsleiste am unteren Bildschirmrand (`#007acc`).

## Elevation & Depth

Das System verzichtet auf künstliche Weichzeichnungs-Schatten im flachen Arbeitsbereich und setzt stattdessen auf **Tonal Layering** (vier Helligkeitsstufen) und feine `1px border`-Abgrenzungen:

- **Ebene 0 (Hintergrund)**: `#0F1117` (Editor-Canvas)
- **Ebene 1 (Struktur)**: `#161922` (Sidebar & Activity Bar)
- **Ebene 2 (Container & Karten)**: `#1E222D` (Panels, Dialoge, Dropdowns)
- **Ebene 3 (Interaktion & Hover)**: `#2D3344` (Hover-Status, selektierte Zeilen)

### Shadow Vocabulary
- **Modal & Palette Elevation** (`box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.7)`): Schwebende Dialoge (Command Palette `Ctrl+P`, Projekt-Wizard, Kontextmenüs) über abgedunkeltem Backdrop (`backdrop-blur-md`).

### Named Rules
**The Clean Surface Rule.** Innerhalb des regulären Workbench-Layouts existieren keine diffusen Schatten. Tiefe entsteht ausschließlich durch Tonwertabstufung und 1px Konturlinien.

## Shapes

- **Ecken-Radien**:
  - Buttons & Eingabefelder: `rounded-lg` (8px Radius) für moderne, angenehme Griffigkeit.
  - Modale Fenster & Karten: `rounded-xl` / `rounded-2xl` (12px–16px Radius) zur klaren Abhebung vom eckigen Editor-Raster.
  - Tabs & Status-Badges: `rounded-none` bzw. `rounded-md` (4px Radius).
- **Konturlinien**: Durchgehend `1px solid #2A2F3D` für saubere, randscharfe Trennung.

## Components

### Buttons
- **Primary Action (Build / Run):**
  - Form: `rounded-md` (6px Radius), Padding: `4px 12px` (Toolbar) bzw. `8px 16px` (Dialoge).
  - Hintergrund: `#3B82F6` (Build) bzw. `#10B981` (Run), Text: `#ffffff` Bold.
  - Hover: Farbaufhellung (`#2563EB` / `#059669`) mit weichem `transition: 0.15s ease`.
- **Panel / Secondary Button:**
  - Hintergrund: `#1E222D` mit `1px solid #2A2F3D`, Text: `#E2E8F0`.
  - Hover: Hintergrund `#2D3344`, Text `#ffffff`.

### Multi-File Tabs
- **Aktiver Tab:**
  - Hintergrund: `#1E222D`, Text: `#ffffff` font-medium.
  - Akzent: `2px solid #3B82F6` an der oberen Kante.
  - Schließen-Button (`✕`): `16px x 16px`, rundet sich bei Hover ab (`#2D3344`).
- **Inaktiver Tab:**
  - Hintergrund: `#161922` mit reduzierter Opazität, Text: `#94A3B8`.
  - Hover: Text wird `#E2E8F0` mit weicher Hintergrundaufhellung.

### Activity Bar Icons
- **Größe & Form:** `40px x 40px`, `rounded-lg`, zentriertes Icon (18px).
- **Aktiver Status:** Leuchtender `2px x 24px` Cobalt-Balken am linken Außenrand (`#3B82F6`).

### Tree Explorer Rows
- **Ordner / Dateien:**
  - Höhe: `26px`, Padding: `2px 8px`, eingerückt nach Verzeichnistiefe.
  - Datei-Icons: Farblich differenziert nach Endung (🐍 Python, ⚙️ ASM, 📄 Include, 💾 ADF/ROM).
  - Aktive Datei: `bg-blue-600/20 text-blue-400 font-semibold border-l-2 border-blue-500`.

### Bottom Panel Tabs
- **Tab-Leiste:** 32px Höhe, `border-b border-[#2A2F3D]`.
- **Aktiver Reiter:** `border-b-2 border-blue-500 text-white font-bold tracking-wider`.
- **Badge Pill:** Rund (`rounded-full`), rot für Fehler (`bg-red-500/20 text-red-400`), gelb für Warnungen.

### Command Palette (Ctrl+P / F1)
- **Container:** `max-w-xl`, schwebend bei `top: 64px`, `rounded-xl`, `bg-[#1E222D]/95 backdrop-blur-md`.
- **Items:** 32px Höhe, `rounded-lg`, Tastaturfokus mit `#3B82F6` Hintergrund und weißen Shortcuts.

## Do's and Don'ts

### Do:
- **Do** alle Hexadezimal-Adressen, Disassembler-Bytes, Cycle-Zahlen und Register in `font-mono` setzen.
- **Do** Farbcodierungen einheitlich halten (Rot = Fehler, Gelb = Warnung, Grün = Erfolg/Run, Blau = Selektion).
- **Do** Tastaturkürzel (`Ctrl+P`, `F1`, `F5`, `F7`, `Ctrl+B`, `Ctrl+J`) in allen Tooltips und Menüs anzeigen.
- **Do** Modale Dialoge mit weichem `backdrop-blur-md` vom Hintergrund isolieren.

### Don't:
- **Don't** bunte, unruhige Verläufe oder Schmuckgrafiken in den Code-Editor einbauen.
- **Don't** mehr als einen prominenten Primär-Button pro Toolbar-Gruppe platzieren.
- **Don't** Hex-Werte ohne einheitliches Prefix (`$` für m68k, `0x` für Python) mischen.
- **Don't** native Browser-Alerts verwenden; alle Rückmeldungen laufen über das `NotificationSystem` (Toasts & Modale).
