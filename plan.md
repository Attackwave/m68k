# plan.md — Bug-Fixes & Feature-Roadmap (Arbeitsplan für Sonnet 5)

Ergebnis eines vollständigen Code-Audits (2026-07-24, drei parallele Bereichs-Audits: Encoder-Familien / Zwei-Pass-Treiber+Output / Core+Disasm+Floppy). Alle als **kritisch** markierten Funde wurden mit dem gebauten Release-Binary empirisch reproduziert — keine Spekulation.

## Umsetzungsstatus (Stand 2026-07-25)

**Erledigt: Phase 1 (alle 6), Phase 2 (alle 10), Phase 3 (alle 6), sowie aus Phase 4: Punkt 1 (B10) und Punkt 8 (Bcc.l-Relaxation).**
Alle Fixes committet als Arbeitsstand im Working Tree (noch nicht gepusht/committed als Git-Commit — `git status` prüfen). Nach jedem Einzelfix verifiziert: `cargo test --all` (aktuell 374 Tests in m68k-asm, alle Crates grün), `cargo clippy --all-targets -- -D warnings` sauber, `cargo fmt --all -- --check` sauber, Golden-Vektoren (`tests/golden/vectors.json`) unverändert grün — keine Vektor-Korrektur nötig.

**Offen aus Phase 4** (jeweils eigenständige Feature-Arbeit, kein Bugfix): 2 (systematisches CPU-Gating), 3 (automatisierter Roundtrip-Test), 4 (Fuzzing-Setup), 5 (DC.X/DC.P echte Konvertierung), 6 (Expression-Evaluatoren konsolidieren), 7 (m68k-floppy-Ausbau), 9 (N2 OPT-State-Tracking).

### Bemerkenswerte Zusatzfunde während der Umsetzung (nicht im ursprünglichen Audit)

Diese wurden erst beim tatsächlichen Implementieren/Testen sichtbar und sind mit demselben Fix miterledigt:

- **1.1 zusätzlich:** `recalculate_pcs` nutzte für Label-PCs eine lokale `pc`-Variable statt `self.pc` — ORG/SECTION-Direktiven innerhalb der Relaxationsschleife hatten dadurch keinen Effekt auf nachfolgende Label-Adressen. Zudem: `SymbolTable::force_set` (für Relaxations-Iterationen) verwarf die Section-Zuordnung des Symbols, was einen ELF/IEEE-695-Regressionstest brach (Section-Info jetzt über neue `force_set_in_section` erhalten). Und: die Größenermittlung `determine_branch_size` konnte bei disp genau 0 (durch die 1.1(f)-Fix-Interaktion) zwischen Short/Word oszillieren — behoben durch monotones Wachstum (nie wieder verkleinern) statt Neu-Bewertung bei jeder Iteration.
- **1.2 zusätzlich:** Der zugehörige bestehende Test `test_addr_reg_indirect_index_l_suffix_sets_long_bit` hatte selbst falsche erwartete Werte (maskierte den Bug durch symmetrische Testdaten disp=0/Xn=D0-D1) — korrigiert.
- **1.5 zusätzlich:** `Expr::evaluate` (rekursiv über generisches `impl Fn`) sprengte Rusts Monomorphisierungs-Rekursionslimit bereits bei einfachen Testausdrücken, weil jeder rekursive Aufruf `&resolve_symbol` eine neue Referenzebene im Typsystem erzeugt — strukturell behoben durch internen `&dyn Fn`-Rekursionspfad (`evaluate_dyn`), nicht nur für die drei neuen Tests, sondern für beliebig tiefe reale Ausdrücke.
- **2.1/2.9 mussten zusammen gefixt werden:** Der `check_ea`-Escape-Hatch (`allowed==0xFFFF` überspringt jede Prüfung) ließ sich nicht entfernen, ohne dass `ea_mode_to_bit`s Fehlklassifizierung von Modus 7/Reg 0-1 (abs.W/abs.L, vorher komplett ungemappt) sofort sichtbar wurde — `AbsoluteShort`/`AbsoluteLong` wären sonst überall abgelehnt worden. Eigene `AREG_DISP`-Bit-Konstante für Modus 5 eingeführt (vorher implizit mit `AINDEXED`/Modus 6 zusammengefasst).
- **2.3 zusätzlich:** Erster Fix-Versuch nutzte Opcode-Basis `0xD1C0`/`0x91C0` für die Dn→ea-Richtung — das kollidierte mit den Opmode-Bits und erzeugte ADDA/SUBA-Encodings. Korrigiert auf `0xD100`/`0x9100`.
- **3.2 zusätzlich:** Label-Erkennung berücksichtigt jetzt auch Misalignment (Ziel liegt im dekodierten Bereich, aber nicht auf einer tatsächlichen Instruktionsgrenze) — nicht nur „außerhalb des Bereichs“.
- **4.1 (B10):** Die eigentliche Ursache war nicht (nur) fehlendes Label-Parsing, sondern dass `enc_fbcc`/`enc_fdbcc`s Dispatch in `encoder.rs` ausschließlich `Operand::Address` matchte — ein zum Parse-Zeitpunkt bereits auflösbares Label liefert aber `AbsoluteShort`/`AbsoluteLong`/`Memory`. Neue Hilfsfunktion `branch_target_address()` fasst alle branch-tauglichen Operand-Varianten zusammen (analog zum bestehenden Muster in `encode_branch`/`encode_dbcc_branch`).

## Arbeitsregeln

- Nach jedem Fix: `cargo test --all && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check` (CI verlangt alle drei sauber).
- Jeder Bugfix bekommt einen Regressionstest, der den Repro-Fall aus diesem Dokument abdeckt.
- **Achtung Golden-Vektoren** (`tests/golden/vectors.json`, fixierter Snapshot): Falls ein Fix einen Golden-Test bricht, ist möglicherweise der *Vektor* falsch (er könnte den Bug als Erwartung enthalten). Dann: Encoding von Hand gegen die M68000PRM prüfen (oder gegen `vasm -m68040`, wurde bei B9 als Ground Truth benutzt) und den Vektor korrigieren — mit Begründung im Commit.
- Encoding-Gotchas und Projektstand: siehe `AGENTS.md` und `CLAUDE.md`. Erledigtes (B1–B9, N1, N4) nicht erneut anfassen.
- Die Bugs sind unabhängig voneinander fixbar, außer wo vermerkt. Empfohlene Reihenfolge: Phase 1 → 2 → 3 → 4.

---

## Phase 1 — KRITISCH (stillschweigend falscher Code oder Panic) ✅ ALLE ERLEDIGT

### 1.1 Zwei-Pass-Kern: Branch-Relaxation ist ein No-Op, Pass 1/Pass 2 divergieren (Sammel-Bug) ✅

**Der schwerwiegendste Befund.** Vier Einzelbugs mit gemeinsamer Wurzel: Pass-1-Größenschätzung weicht vom Pass-2-Encoder ab, und kein Mechanismus fängt das ein — Labels behalten die Pass-1-Werte, der Code liegt physisch woanders.

**(a) Relaxation tot** — `assembler.rs:1724f`: `BranchInfo.target_symbol`/`target_address` werden auf `None` gesetzt und nie befüllt; `relax_branches` (Z. 2000-2043) läuft dadurch immer leer. Pass 1 schätzt jeden Branch auf 4 Bytes (Word), Pass 2 kodiert Short sobald möglich.
Repro (verifiziert):
```asm
    ORG $1000
start:
    BRA next
next:
    RTS
```
→ `6002 4E75`, Symbol `next=$1004`, aber RTS liegt bei `$1002`; die BRA springt 2 Bytes HINTER das RTS.
**Fix:** Relaxation real machen — `target_symbol` in Pass 1 befüllen, Iteration bis Fixpunkt (monoton nur widening, terminiert garantiert), Labels nach jeder Iteration neu berechnen. Alternativ (einfacher, konservativ): Pass 2 zwingt die in Pass 1 angenommene Größe (Word), gibt also nie Short aus, außer der Nutzer schreibt explizit `.s`/`.b` — dann müssen aber Reichweitenfehler sauber gemeldet werden. Entscheidung dokumentieren; echte Relaxation ist die richtige Lösung.

**(b) DS-Rundungsdivergenz** — Pass 1 (`assembler.rs:1786-1802`) rundet `DS.B` auf gerade Bytezahl (`(total+1)&!1`), Pass 2 (`encode_ds`, Z. 2975ff) nicht.
Repro: `DS.B 3` + Label danach → Symbol `$1004`, Code bei `$1003`. **Fix:** identische Regel in beiden Pässen (klären, was gewollt ist: klassische Assembler runden `DS` *nicht* automatisch; vermutlich Pass-1-Rundung entfernen).

**(c) DC.B-String-Größe** — Pass 1 (`assembler.rs:1769-1785`) zählt Argumente statt Bytes: `DC.B "HELLO"` = 1 Element = 1 Byte geschätzt, Pass 2 emittiert 6 Bytes.
Repro (verifiziert): Symbol nach `DC.B "HELLO"` = `$1002`, NOP physisch bei `$1006`.
**Fix:** Pass-1-Schätzung muss Strings/Chars byte-genau zählen (gleiche Logik wie `encode_dc` in Pass 2, idealerweise geteilte Funktion). **Dabei auch klären:** Pass 2 emittiert `"HELLO"` als 6 Bytes *inkl. Null-Terminator* (`48 45 4C 4C 4F 00`) — Motorola-Assembler (vasm/Devpac) null-terminieren NICHT. Prüfen, ob das ein Even-Padding oder ein echter Terminator ist; gegen vasm angleichen.

**(d) Forward-Referenz → Absolute.W/.L-Divergenz** — undefiniertes Symbol wird in Pass 1 zu `Operand::Address(0)` (`assembler.rs:435ff`), `encode_ea` wählt am Wert 0 die Short-Form (1 Ext-Word); Pass 2 löst z. B. `$20000` auf → Long-Form (2 Ext-Words), alles danach verschiebt sich.
**Fix:** Forward-Referenzen in Pass 1 konservativ als Absolute.L schätzen (Standard-Ansatz), oder in die Relaxationsschleife aufnehmen.

**(e) Bcc-Grenzwerte (latent, wird beim Relaxations-Fix scharf)** — `determine_branch_size` (`assembler.rs:2148`) erlaubt Short für `-128..=127`, `enc_bcc` (`enc_flow.rs:42`) nur `-127..=127`. Zudem ist `-127` als untere Grenze falsch (Bcc.b kann `-128`).
**Fix:** beide auf `-128..=127` vereinheitlichen, MINUS der reservierten Werte aus (f).

**(f) disp=0 und disp=-1 bei Byte-Branches (reservierte Encodings)** — `encode_bra`/`encode_bsr` (`assembler.rs:2568/2600`) und `enc_bcc` emittieren bei disp=0 z. B. `0x6000` als 1-Word-Instruktion — das ist das *Word-Form-Präfix*, die CPU frisst das Folgewort als Displacement. disp=-1 (`0xFF`) kollidiert mit dem 68020-Long-Marker `0x60FF`.
**Fix:** disp==0 und disp==-1 erzwingen Word-Form. Regressionstest: `BRA next` direkt gefolgt von `next:`.

**Tests für 1.1 gesamt:** Programm mit Vorwärts-Short-Branch + Folgelabels (Symbol == physische Adresse prüfen), `DS.B 3`, `DC.B "HELLO"`, Forward-Ref auf `> $FFFF`, Branch auf direkt folgendes Label, Branch mit disp genau -128/+127/-129/+128.

### 1.2 Brief-Format-Index-EA: Xn und disp beim Encoding vertauscht (alle CPUs) ✅

`Operand::AddrRegIndirectIndex(An, Xn, disp, scale, is_long)` (`m68k-core/src/operands.rs:20`; Parser konstruiert dokumentationskonform), aber `ea_encode.rs:51` destrukturiert `(n, disp, xreg, …)` — **Xn↔disp vertauscht gebunden** (kompiliert wegen Integer-Casts). Ebenso `PcRelativeIndex`: `operands.rs:29` = `(Xn, disp, …)` vs. `ea_encode.rs:82` = `(disp, xreg, …)`.

Repro (verifiziert): `MOVE.W (4,A0,D1.W),D0` → `3030 4001` (Index D4, disp 1); korrekt `3030 1404`-Layout-Regel: Ext-Word = D/A(15) | Xn(14-12) | W/L(11) | Scale(10-9) | 0(8) | disp(7-0), hier `0x1004`. `LEA ($7F,A0,D2.L),A2` → `0xF802`: disp 0x7F läuft als „Registernummer" ins Feld über und setzt sogar das A-Register-Bit.

**Betrifft jede `(d8,An,Xn)`- und `(d8,PC,Xn)`-EA, sobald Xn-Nummer ≠ disp-Wert — auch auf 68000.** Die bisherigen Tests decken nur symmetrische Fälle (disp=0, D0) bzw. Scale/Long-Bit ab.

**Fix (2 Zeilen):** Destrukturierung in `ea_encode.rs:51/82` an die Felddefinition angleichen. **Tests:** asymmetrische Werte für beide Operand-Typen, z. B. `(4,A0,D1.W)` → Ext `0x1004`, `(8,PC,A3.L)`, negatives disp, plus Roundtrip über den Disassembler.

### 1.3 ADDA.L / SUBA.L / CMPA.L: Größenbit an falscher Position ✅

`enc_flow.rs`: `enc_adda_imm` (Z. 442f), `enc_adda_ea` (Z. 463ff), `enc_suba_imm` (Z. 473f), `enc_suba_ea` (Z. 494ff), `enc_cmpa` (Z. 704ff) schieben die Größe in Bits 12/13 (`sz<<12` bzw. `(sz-1)<<12`). Korrekt ist Opmode-Bit 8: word=`011`, long=`111`.

Repro (verifiziert):
| Instruktion | Emittiert | Korrekt |
|---|---|---|
| `ADDA.L (A2),A1` | `F2D2` (F-Line-Trap!) | `D3D2` |
| `SUBA.L #-1,A1` | `B2FC FFFF FFFF` (= CMPA.W + 4-Byte-Immediate → Stream-Desync) | `93FC …` |
| `CMPA.L (A2),A1` | `B2D2` (= CMPA.W) | `B3D2` |

`.W`-Formen sind zufällig korrekt (Bit 12 im Base bereits gesetzt). **Null Testabdeckung** für ADDA/SUBA/CMPA in den Golden-Vektoren.

**Fix:** `op | if size=="l" { 0x0100 } else { 0 }` (Word-Opmode steckt schon im Base). **Zusätzlich (1.3b):** die EA-Formen benutzen Kategorie `DATA`, die Adressregister ausschließt — `CMPA.L A0,A1` wird fälschlich abgelehnt (verifiziert). ADDA/SUBA/CMPA erlauben als Quelle ALLE EA-Modi inkl. `An` → Kategorie auf `ALL` ändern (aber echtes `ALL` mit Prüfung, siehe 2.1!). **Tests:** alle 6 Formen × .w/.l, plus `An`-Quelle.

### 1.4 Panics bei DS/DCB-Größen (Integer-Overflow auf Nutzereingabe) ✅

`assembler.rs:1800/1857/1782`: `element_size * count` ohne `checked_mul`. Repro (verifiziert vom Audit): `DS.L $80000000` → Panic „attempt to multiply with overflow"; `DCB.L $80000000,0` ebenso. Gleiches Muster in Pass 2 (`encode_ds` Z. 2975, `encode_dcb` Z. 3053); die `vec![…; count]`-Allokationen sind zusätzlich OOM-anfällig.
**Fix:** `checked_mul` + Fehler „size too large", sinnvolles Limit für Reservierungen (z. B. 16 MB = 68k-Adressraum). Ein Assembler darf auf kaputten Input nie panicken.

### 1.5 `m68k-core/src/expr.rs`: Shift/Neg-Panics in öffentlicher API ✅

Z. 89f: `l << (r as u32)` paniced (Debug) bei `1<<64` oder `8>>-1`; Z. 58: `-i64::MIN` ebenso. Der Zweit-Evaluator in `directives.rs:1094ff` macht es korrekt mit `wrapping_shl/shr` — Inkonsistenz. Aktuell nicht auf dem CLI-Hot-Path (der läuft über directives.rs), aber `pub` exportiert.
**Fix:** `wrapping_*`/maskierte Shift-Counts analog directives.rs. **Langfristig (siehe 4.6):** einen der beiden Evaluatoren eliminieren.

### 1.6 Disassembler verwirft Scale-Bits im Brief-Format (68020+) ✅

`m68k-core/src/addressing.rs:336-343`: `parse_index_extension` liest Bits 9-10 nicht; Formatierung (Z. 160-162) hardcodet Scale 1. `LEA (4,A0,D1.W*4),A2` disassembliert ohne `*4` → stillschweigend falsche Disassembly, Roundtrip bricht. (Der Full-Format-Pfad `decode_full_ea` macht es korrekt, Z. 356.)
**Fix:** Scale aus Bits 9-10 extrahieren, bei Scale>1 als `*n` formatieren. **Test:** Roundtrip mit 1.2-Fix zusammen.

---

## Phase 2 — MITTEL (falsche Ablehnung, falscher Output, DoS, Randfälle) ✅ ALLE ERLEDIGT

### 2.1 Bit-Ops: EA-Prüfung komplett deaktiviert ✅ (zusammen mit 2.9 gefixt, siehe Zusatzfunde oben)
`enc_logic.rs:222/247`: Memory-Form von BTST/BSET/BCLR/BCHG nutzt `allowed = ALL` (0xFFFF), was in `check_ea` (`ea_encode.rs:148-154`) als Escape-Hatch jede Prüfung überspringt. `BTST #1,A0` oder `BSET #1,(label,PC)` kodieren stillschweigend Unsinn. Korrekt: BTST-Memory=`DATA`, BSET/BCLR/BCHG=`DATA_ALT`. **Dabei den `0xFFFF`-Escape-Hatch in `check_ea` generell entfernen** (echte ALL-Maske stattdessen) — betrifft auch 1.3b.

### 2.2 ADDQ/SUBQ lehnen `An`-Ziel ab ✅
`enc_math.rs` `enc_quick` (Z. 295-301): nur DataReg + ALTERABLE_MEMORY (ohne AREG). `ADDQ #4,A0` (extrem üblich, Stack-Manipulation!) → Fehler. Gültig für `.w`/`.l`; nur `.b` auf `An` ist illegal. **Zudem** fehlt der Range-Check: `ADDQ #0,…` kodiert still als `#8`, `#9` als `#1`. **Fix:** `An`-Arm ergänzen (normale EA-Mode-1-Kodierung), Range 1-8 validieren, `.b`+`An` ablehnen.

### 2.3 ADD/SUB `Dn,<ea>`-Richtung fehlt ✅
`enc_math.rs` `enc_add`/`enc_sub`: nur `<ea>,Dn`. `ADD D0,(A0)` → Fehler. Fix: Opmode-Bit-8-Form (Base `0xD1C0`/`0x91C0`, Ziel-Kategorie ALTERABLE_MEMORY) ergänzen — AND/OR in `enc_logic.rs` haben beide Richtungen bereits als Vorbild.

### 2.4 Intel-Hex: Typ-04-Record (Extended Linear Address) mit falscher Checksumme ✅
`output.rs:378-383`: die zwei ULBA-Bytes fließen nicht in die Checksumme ein. Verifiziert: `ORG $10000` → `:020000040001FA`, korrekt wäre `…F9`. Jeder Output über 64K ist für konforme Loader ungültig. **Fix:** ULBA-Bytes in `compute_hex_checksum` einbeziehen; Test mit bekannt-gutem Referenzwert.

### 2.5 CLI-Binary-Output verschluckt ORG-Lücken ✅
`m68k-cli/src/bin/m68k-asm.rs:144-155`: konkateniert `instr.words` direkt statt `output::generate_binary` (das Lücken via `instr.pc` füllt) zu nutzen. `ORG $1000 / NOP / ORG $1010 / RTS` → 4-Byte-Datei ohne Lücke; inkonsistent zu S-Record/Hex/ELF, DS-Reservierungen entfallen. **Fix:** auf `generate_binary` umstellen. **Dabei entscheiden:** Füllbyte-Policy und ein Schutz gegen absurde Lücken (`ORG $0` + `ORG $FF0000` → 16-MB-Datei — Warnung oder Limit).

### 2.6 `ERROR`-Direktive führt nicht zu Exit-Code ≠ 0 ✅
`assembler.rs:1873-1877` sammelt nur; CLI (`m68k-asm.rs:105-114`) prüft `has_errors()` nie → Exit-Code 0, Build-Systeme übersehen den Fehler. **Fix:** CLI prüft `has_errors()` und beendet mit ≠ 0; kein Output-File schreiben.

### 2.7 DC.X/DC.P: Float-Literal wird still zu 0 ✅ (unwrap_or(0) durch Fehler ersetzt; echte Konvertierung weiterhin offen als Feature 5)
`assembler.rs:3196-3212`: `text.parse::<u128>().unwrap_or(0)` — `DC.X 3.14` emittiert still Nullen. Hex/Raw-only ist eine dokumentierte Einschränkung, aber das stille `unwrap_or(0)` muss ein Fehler werden („DC.X requires hex literal; float conversion not supported"). Echte Extended/Packed-Konvertierung → Feature 4.5.

### 2.8 Unbegrenzte Allokationen aus Datei-Headern (DoS, m68k-floppy + Hunk-Reader) ✅
Analoge Muster zu den bereits gefixten Panics (Commit 2d9b812), noch offen:
- `ipf.rs:92,99,111,117,131` — `vec![0u8; size as usize]`, Chunk-Size bis 4 GB aus Datei
- `uae.rs:106` — Track-Size ungekappt (`num_entries` ist bereits gekappt — gut)
- `m68k-core/src/amiga_hunk.rs:214,234` — `hunk_count`/Hunk-Größen roh aus Datei, `with_capacity` bis 16 GB
**Fix:** Größen vor Allokation gegen die tatsächliche Dateigröße (bzw. Rest-Länge) validieren. Regressionstests mit synthetischen Bösartig-Headern.

### 2.9 `ea_mode_to_bit` klassifiziert Modi 5/6 falsch ✅ (zusammen mit 2.1 gefixt, siehe Zusatzfunde oben)
`m68k-core/src/ea_categories.rs:42-58`: `reg==7`-Sonderfälle mappen `(d16,A7)`→ABSW und `(d8,A7,Xn)`→ABSL. Per PRM gelten Modi 5/6 für ALLE An; abs.W/L sind Modus 7/Reg 0-1. Aktuell zufällig folgenlos (alle benutzten Masken enthalten die betroffenen Bits gemeinsam), bricht aber bei jeder neuen Kategorie. **Fix:** Sonderfälle entfernen, eigenes DISP-Bit für Modus 5 einführen, Masken-Konstanten gegen die PRM-Tabelle abgleichen.

### 2.10 Verschachtelte Makro-Aufrufe werden nicht expandiert ✅
`assembler.rs:1333-1572`: einziger linearer Durchlauf; expandierte Zeilen werden nicht erneut gescannt. Makro ruft Makro → „unknown mnemonic". **Fix:** Re-Scan der expandierten Zeilen mit Rekursionstiefen-Limit (z. B. 64) gegen Endlos-Expansion (aktuell schützt nur die Nicht-Rekursion vor dem Hang!).

---

## Phase 3 — NIEDRIG / Kosmetik ✅ ALLE ERLEDIGT

- **3.1** ✅ Long-Branch (Bcc 32-bit) nicht CPU-gegated: bei `--cpu 68000` wird bei Out-of-Range-Ziel still die 68020-Form emittiert (`enc_bcc`/`enc_bsr`) — Fehler stattdessen.
- **3.2** ✅ Disassembler: Branch-Ziele außerhalb des Bereichs / mitten in Instruktionen → dangling Labels (`bra label3` ohne `label3:`) — `disassembler.rs:79-99`. Ziele außerhalb als `$adresse` rendern statt Label.
- **3.3** ✅ `$0(a0)` statt `(a0)` bei disp=0 — `addressing.rs:310-315`.
- **3.4** ✅ Toter Code: Char-Literal-Zweig unerreichbar in `tokens.rs:97-107` (`'A'` wird Str statt Char) und in `assembler.rs:2878` (Länge-2-Check kann nie treffen). Aufräumen, Verhalten klären.
- **3.5** ✅ `amiga_hunk_writer.rs:123`: `value - base` kann unterlaufen, wenn Symbolwert < Section-Base (`saturating_sub` + Warnung).
- **3.6** ✅ CNOP Pass 2 ohne Alignment-Guard (`assembler.rs:2765-2792`) — Division durch 0 möglich, wenn sich das Alignment zwischen Pässen ändert (SET-Symbol).

---

## Phase 4 — Features (priorisiert)

1. ✅ **B10 — FBcc/FDBcc Label-Support + Relaxation** (bekannt offen, Fix-Skizze in `AGENTS.md` Z. 90): eigene `is_fbcc_mnemonic()`/`is_fdbcc_mnemonic()`-Checks analog DBcc-Vorbild (B8), 32 FPU-Bedingungscodes, Word-Default mit Long-Upgrade. *Nach 1.1 machen — baut auf funktionierender Relaxation auf.* — Umgesetzt über `branch_target_address()` in `encoder.rs`, siehe Zusatzfunde oben. Volle Bcc-artige Relaxation (Word→Long) war nicht nötig, da FBcc/FDBcc laut B9-Design nur expliziten `.l`-Suffix nutzen, kein automatisches Upgrade.
2. **(offen) Systematisches CPU-Gating**: `--cpu` existiert, wird aber nur punktuell durchgereicht. Zentrale Tabelle Mnemonic→Mindest-CPU (68000/010/020/030/040/060, FPU/MMU separat) + EA-Feature-Gating (Scale, Full-Format, Long-Branch). Fehler wie vasm: „instruction not available on 68000".
3. **(offen) Systematischer Roundtrip-Test** Assembler↔Disassembler: jede unterstützte Instruktionsform einmal encode→decode→reencode, Byte-Vergleich (B9 hat das manuell für ~35 Formen gemacht — automatisieren). Hätte 1.2, 1.3 und 1.6 sofort gefunden. Höchster Test-ROI im ganzen Projekt.
4. **(offen) Fuzzing** (`cargo-fuzz`): Targets für `Disassembler::disassemble`, alle vier Floppy-Backends, `amiga_hunk::parse`, Assembler-Gesamtpipeline. Historie (2 Panics in m68k-floppy, 1.4, 1.5) zeigt, dass es lohnt. CI-Smoke-Run (60 s pro Target).
5. **(offen) DC.X/DC.P echte Konvertierung**: IEEE-754-Extended (80-bit) aus Dezimal-Literal, echtes Packed-Decimal — hebt die Hex/Raw-Einschränkung auf (2.7 hat nur das stille `unwrap_or(0)` durch einen Fehler ersetzt, keine echte Konvertierung). FMOVE-k-factor-Syntax (`FMOVE.P FP0,(A0){#3}`) als Anschlussfeature (Parser + Bits 6-0 im Ext-Word, `enc_fpu.rs:109-116`).
6. **(offen) Expression-Evaluatoren konsolidieren**: `m68k-core/src/expr.rs` (AST, ungenutzt vom CLI-Pfad) vs. `directives.rs`-Evaluator — einen kanonischen machen, den anderen löschen oder als Wrapper. Verhindert Drift wie 1.5. **Hinweis:** `expr.rs` wurde bei 1.5 bereits robuster gemacht (wrapping-Arithmetik, `&dyn Fn`-Rekursionsfix) — falls hier `directives.rs` als kanonisch gewählt wird, prüfen ob dessen Evaluator dieselbe Rekursionslimit-Anfälligkeit hat (`expr.rs`s ursprüngliches `impl Fn`-Problem trat erst bei echten Testfällen zutage, könnte in `directives.rs` unentdeckt vorliegen).
7. **(offen) m68k-floppy-Ausbau**: OFS/FFS-Dateisystem (Verzeichnis-Listing, Datei-Extraktion aus ADF), ADF-Schreiben/Erzeugen, Bootblock-Checksummen-Berechnung/-Reparatur (`--fix-bootblock`).
8. ✅ **Bcc.l-Relaxation (68020+)**: `determine_branch_size` kann kein Long — Ziel außerhalb ±32K auf 68020+ automatisch als 32-bit-Form (gehört logisch zu 1.1/B10). Umgesetzt: dritte `BranchSize::Long`-Stufe, monoton (nie zurück auf Word), 68000 gibt sauberen Fehler statt still Long zu emittieren.
9. **(offen) N2 — OPT-State-Tracking** (bekannt offen, niedrig): mindestens `OPT`-Flags parsen und für Warnungen nutzen.

---

## Verifikation (gesamt)

1. `cargo test --all` — alle bestehenden + neuen Regressionstests grün.
2. `cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`.
3. End-to-End-Smoke: die Repro-Snippets aus Phase 1 assemblieren und Bytes/Symboltabelle gegen die dokumentierten Soll-Werte prüfen (insbesondere: `BRA next`/`next:` direkt dahinter; `DC.B "HELLO"`+Label; `MOVE.W (4,A0,D1.W),D0` → `3030 1004`; `ADDA.L (A2),A1` → `D3D2`; `CMPA.L A0,A1` akzeptiert).
4. Roundtrip: assemblierte Testdatei durch `m68k-disasm` und wieder durch `m68k-asm` — Byte-identisch (nach 1.2+1.6+Feature 3).
5. Intel-Hex-Datei mit `ORG $10000` gegen einen Referenz-Parser (z. B. `srec_cat` oder Python `intelhex`) validieren.
6. Golden-Vektoren: Abweichungen einzeln gegen PRM/vasm begründen (siehe Arbeitsregeln).
