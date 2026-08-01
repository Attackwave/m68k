# plan.md — Audit August 2026

Vollständiges Review nach v2.0.1. Alle als **verifiziert** markierten Punkte wurden empirisch gegen einen Referenzassembler reproduziert — keine Spekulation. Die gefixten Punkte (P0) sind im Working Tree, aber noch nicht committet.

**Ausgangslage:** 595 Tests grün, clippy/fmt sauber, Byte-Parität 12/12 + 13/13 auf zwei Korpora echter Amiga-Quellen, ROM-Roundtrip 93,9%/94,7%.

**Methodik:** Drei systematische Teilanalysen (EA-Kategorien gegen die PRM-Tabellen, Decoder-Pattern-Überdeckung, Pass-1/Pass-2-Divergenz über alle Direktiven) plus ein eigener Sweep über ~100 Instruktionsformen aller Familien. Jeder gemeldete Befund wurde anschließend **selbst gegen die Referenz nachgeprüft** — zwei Meldungen erwiesen sich dabei als Fehlalarm (siehe „Geprüft und entkräftet" am Ende).

---

## P0 — Bereits gefixt (uncommitted, brauchen Tests + Commit)

Diese drei wurden während dieses Audits gefunden und behoben. Sie brauchen noch Regressionstests und einen Commit.

### P0.1 `MOVE from CCR` fehlte komplett — silently wrong ✅ gefixt

`MOVE.W CCR,D0` assemblierte zu `303C FFFF` (= `MOVE.W #-1,D0`) statt `42C0`. Ursache: der CCR-Marker (`Operand::Immediate(-1)`) hatte keinen Dispatch-Arm für die *Lese*-Richtung und fiel auf den generischen MOVE-Pfad durch, der ihn als echtes Immediate behandelte.

**Das ist die gefährlichste Fehlerklasse:** kein Fehler, sondern eine völlig andere Instruktion.

Fix in `crates/m68k-asm/src/encoder.rs`: neuer Arm für `(Immediate(-1), dst)` **vor** dem bestehenden `(src, Immediate(-1))`-Arm, plus CPU-Gating (68010+).
Verifiziert: alle 8 SR/CCR/USP-Formen jetzt byte-identisch.

### P0.2 `FSINCOS <ea>,FPc:FPs` wurde abgelehnt ✅ gefixt

Die Doppelpunkt-Schreibweise — die einzige, die reale Quellen verwenden — war nicht implementiert; nur eine 3-Operanden-Form `src,FPc,FPs`, die kein Assembler so schreibt.

Fix: `parse_operand_text` (`assembler.rs`) erkennt jetzt FP-Registerpaare als `Operand::RegPair`; `encoder.rs` hat einen `FSINCOS`-Arm dafür.
Verifiziert: 25 FPU-Formen byte-identisch.

### P0.3 `PFLUSH #fc,#mask` (ohne EA) wurde abgelehnt ✅ gefixt

Nur die 3-Operanden-Form war implementiert. Die 2-Operanden-Form hat ein anderes Extension-Layout (Mode `100` statt `110`, Opword ohne EA-Feld).

Fix: neue `enc_pflush_no_ea` in `enc_mmu.rs`, Operandenzahl-Prüfung in `assembler.rs` auf 2-oder-3 gelockert, Dispatch in `encoder.rs`.
Verifiziert: alle 6 PFLUSH/PFLUSHA-Formen byte-identisch.

### P0.4 `CONTROL_ALT` enthielt `(An)+` und `-(An)` — akzeptierte ungültige Bitfeld-EAs ✅ gefixt

`CONTROL_ALT` war als Alias auf `ALTERABLE_MEMORY` definiert und schloss damit die Auto-Increment/Decrement-Modi ein. Per PRM ist „Control Alterable" aber `Control ∩ Alterable` — und `(An)+`/`-(An)` sind **keine** Control-Modi, weil sie keine feste effektive Adresse haben.

**Verifiziert:** `BFCLR (A0)+{0:8}` wurde von uns akzeptiert und zu Bytes kodiert, während die Referenz es ablehnt. Wir erzeugten also Code für eine Instruktion, die die CPU nicht kennt.

Fix in `crates/m68k-core/src/ea_categories.rs`: `CONTROL_ALT = AREG_IND | AREG_DISP | AINDEXED | ABSW | ABSL`. Die Aufrufer, die Predecrement legitim zulassen (FMOVEM/FSAVE/FRESTORE in `enc_fpu.rs`), ORen `APREDEC` bereits explizit dazu — die bleiben unverändert.
Nachgeprüft: 5 Bitfeld-Formen jetzt referenzkonform, FPU unverändert byte-identisch, keine Regression.

**Aufgabe:** Regressionstests für alle vier schreiben (Sollbytes stehen oben), dann committen.

---

## P1 — Echte Lücken mit Risiko

### P1.1 MMU-Familie hat **null** Roundtrip-Abdeckung

`crates/m68k-asm/tests/instruction_coverage.rs` deckt ~280 Formen ab, aber `PMOVE`, `PFLUSH`, `PTESTR`, `PTESTW` kommen **kein einziges Mal** vor (verifiziert per grep). Genau dort saß P0.3.

Ebenso fehlen sie in `tests/golden/vectors.json`.

**Aufgabe:** MMU-Formen in `instruction_coverage.rs` ergänzen — `PMOVE` in beide Richtungen für TC/SRP/CRP/TT0/TT1/MMUSR, `PFLUSH` in beiden Formen, `PTESTR`/`PTESTW` mit und ohne `An`. Sollbytes vorher gegen die Referenz erzeugen, nicht aus der PRM ableiten.

### P1.2 `amigados.rs` und `adf_writer.rs` sind nicht gefuzzt

`fuzz/fuzz_targets/` hat 6 Targets (`disassemble`, `amiga_hunk_parse`, `assembler_pipeline`, `floppy_adf`, `floppy_uae`, `floppy_ipf`) — aber **keines** deckt `amigados.rs` oder `adf_writer.rs` ab.

Das sind genau die Module, in denen im Juli zwei kritische Bugs steckten (Datenblock-Tabelle rückwärts verankert, `amiga_hash` mit falscher Maske). Beide Module parsen ungeprüfte Strukturen aus Disk-Images: Hash-Table-Traversal, File-Header-Ketten, Extension-Block-Ketten — alles Kandidaten für Endlosschleifen und OOB-Zugriffe bei manipulierten Images.

**Aufgabe:** neues Target `fuzz/fuzz_targets/floppy_amigados.rs`, das ein synthetisches ADF aus den Fuzz-Bytes baut und `AmigaFs::mount` + `list_dir` + `walk` + `read_file_at_path` durchläuft. In den CI-Job `fuzz-smoke` aufnehmen.

### P1.3 README dokumentiert stdin-Unterstützung, die nicht existiert

`README.md:31` (`m68k-disasm`) und `README.md:110` (`m68k-asm`) versprechen beide „use `-` for standard input". **Verifiziert: beide brechen ab** mit `cannot read '-': No such file or directory`.

**Aufgabe:** entweder implementieren (beide Binaries, `-` → stdin lesen; bei `m68k-asm` zusätzlich `-o -` → stdout) oder aus dem README streichen. Implementieren ist vorzuziehen — für Pipelines im Emulator-Workflow ist das nützlich.

### P1.4 `PLPAR`/`PLPAW` sind kodierbar, aber nicht dekodierbar

**Verifiziert:** `PLPAR (A0)` / `PLPAW (A1)` assemblieren korrekt zu `F5C8`/`F589` — byte-identisch zur Referenz. Der Disassembler hat aber **kein Pattern dafür** und gibt stattdessen `cinvl bc,(a0)` / `cinvl ic,(a1)` aus. Der Roundtrip ist damit gebrochen: reassembliert man die Ausgabe, kommen andere Bytes heraus.

Betrifft 68060-Code. Der Assembler kann die Instruktionen (`encoder.rs`), `opcodes.rs` fehlt der Eintrag.

**Aufgabe:** `OpcodePattern`-Einträge für PLPAR/PLPAW in `crates/m68k-core/src/opcodes.rs` ergänzen, **vor** dem CACHE-Pattern (dessen Maske sie sonst verschluckt — anders als bei den Fällen unten scheitert hier das Operanden-Parsing *nicht*, das breitere Pattern greift also wirklich). Sollbytes: `F5C8` = `PLPAR (A0)`, `F589` = `PLPAW (A1)`. Danach in `instruction_coverage.rs` aufnehmen.

### P1.5 `m68k-floppy` validiert `--cpu` nicht

`m68k-asm` und `m68k-disasm` rufen beide `validate_cpu_name` auf, `m68k-floppy` nicht (verifiziert per grep). Aktuell folgenlos, weil das Binary die CPU nicht nutzt — aber inkonsistent, und wenn es später Disassembly-Optionen bekommt, wird es zur stillen Fehlerquelle.

**Aufgabe:** entweder `--cpu` dort entfernen (falls ungenutzt) oder validieren.

---

## P2 — Qualität und Wartbarkeit

### P2.1 `AGENTS.md` ist stale

Zeile 13 nennt „570 passed" — aktuell sind es 595. Die Modul-LOC-Angaben stammen von PR #13. Da die Datei als Projektstand dient, führt das beim nächsten Antasten in die Irre.

**Aufgabe:** Testzahlen, LOC-Angaben und den „Stand"-Block auf v2.0.1 aktualisieren.

### P2.2 14 stille `unwrap_or(0)` / `unwrap_or_default()` in Encoder-Pfaden

`grep` findet 14 Stellen in `crates/m68k-asm/src/` und `crates/m68k-core/src/` außerhalb von Tests. Jede davon kann einen Parse- oder Auswertungsfehler in eine stille 0 verwandeln — exakt das Muster, das bei `evaluate_simple_number` zu den PC-relativen und Displacement-Bugs geführt hat.

**Aufgabe:** jede Stelle einzeln bewerten: Ist 0 ein legitimer Default (dann Kommentar warum) oder verschluckt sie einen Fehler (dann `?` propagieren)? Nicht pauschal ersetzen — bei Pass-1-Größenschätzungen ist der Fallback teils gewollt.

### P2.3 Kein MSRV, keine `rust-toolchain`-Datei

Weder `rust-version` in einer `Cargo.toml` noch eine `rust-toolchain.toml`. Der Code nutzt aber neuere Features (let-chains, `is_multiple_of`), die eine recht aktuelle Toolchain verlangen. CI läuft auf `stable` — ein Nutzer mit älterem Rust bekommt kryptische Compilerfehler.

**Aufgabe:** `rust-version` im Workspace deklarieren (die tatsächlich benötigte Version ermitteln, nicht raten) und optional `rust-toolchain.toml` für reproduzierbare Builds.

### P2.4 CI testet nur eine Plattform, eine Toolchain

`.github/workflows/ci.yml` läuft ausschließlich auf `ubuntu-latest` mit `stable`. Für ein Tool, das laut README auch anderswo laufen soll, fehlt zumindest ein Windows- oder macOS-Job. Auch `cargo audit` läuft nicht in CI (AGENTS.md erwähnt es als manuell ausgeführt).

**Aufgabe:** Matrix um `windows-latest` erweitern; `cargo audit` als eigenen (nicht blockierenden) Job.

### P2.5 Golden-Vektoren sind eingefroren und decken die Neuzugänge nicht

`tests/golden/vectors.json` hat 123 Vektoren und ist laut AGENTS.md „nicht mehr regenerierbar". Alle seit v1.0.2 hinzugekommenen Instruktionsformen (MMU, k-factor, die v2.0.1-Fixes) sind dort nicht vertreten.

**Aufgabe:** entscheiden, ob die Golden-Vektoren eingefroren bleiben (dann dokumentieren, dass `instruction_coverage.rs` die maßgebliche Suite ist) oder ob sie kontrolliert erweitert werden. Nicht beides halb.

---

### P2.6 CNOP: Padding-Formel in beiden Pässen dupliziert

`estimate_directive_size` (Zeile 2602) und `encode_directive` (Zeile 3647) berechnen das CNOP-Padding mit derselben, aber **getrennt hingeschriebenen** Formel `alignment - (target % alignment)`. `EVEN`, `ALIGN` und `INCBIN` nutzen dagegen je einen gemeinsamen `handle_*_pass1`/`pass2`-Kern.

**Verifiziert:** Aktuell kein Zahlenunterschied — auch nicht mit einem `SET`-Symbol als Alignment (getestet: `ALIGNVAL SET 4` → beide Pässe liefern dasselbe, Bytes identisch zur Referenz). Es ist ein Wartungsrisiko, kein Bug: Genau diese Divergenzklasse hat im Juli-Audit sechs Label-Korruptionen verursacht.

**Aufgabe:** Padding-Berechnung in einen gemeinsamen Helfer ziehen, analog zu `handle_align_pass1`.

## Geprüft und entkräftet

Zwei gemeldete Befunde haben sich bei der Nachprüfung **nicht bestätigt**. Hier dokumentiert, damit sie nicht erneut untersucht werden:

### Decoder-Pattern-Überdeckung: formal vorhanden, praktisch folgenlos

Eine Maskenanalyse über alle 137 Patterns findet neun Paare, bei denen ein breiteres Pattern früher steht als ein engeres und dessen Opcodes formal mit abdeckt:

```
AND verdeckt MULU/MULS · CMPI verdeckt CAS · ADDI verdeckt RTM/CALLM
EORI verdeckt CAS · CACHE verdeckt MOVE16 · CMP2 verdeckt RTM/CALLM
```

**Das ist kein Bug.** `decode_next` (`crates/m68k-disasm/src/decoder.rs:129`) *probiert* jedes passende Pattern und geht bei fehlgeschlagenem Operanden-Parsing zum nächsten weiter — das breitere Pattern scheitert dort, das engere greift.

**Verifiziert:** Alle neun Instruktionen dekodieren korrekt; ein Roundtrip über 15 Formen (MULU/MULS/CAS in allen Größen, MOVE16, CMP2/CHK2, RTM, CALLM, plus die verdeckenden AND/ANDI/CMPI/ADDI/EORI) ist **byte-identisch**.

Anders liegt der Fall bei PLPAR/PLPAW (P1.4) — dort scheitert das breitere CACHE-Pattern *nicht*, weshalb es dort tatsächlich greift.

### Pass-1/Pass-2-Divergenz bei Direktiven: keine gefunden

Eine Durchsicht aller Direktiven, die Bytes erzeugen oder den PC verschieben (`dc`, `dcb`, `ds`, `even`, `align`, `cnop`, `org`, `section`, `offset`, `incbin`, `rs`/`rsreset`/`rsset`), ergibt **keine** Divergenz zwischen Schätzung und Encoding. Einzig CNOP hat duplizierte Logik ohne Zahlenunterschied — siehe P2.6.

Ebenso: **keine** Direktive, die in `is_directive_name` als bekannt gilt, aber in `encode_directive` in den Fehlerzweig fällt. Alle 45 Namen haben entweder einen eigenen Arm oder fallen bewusst in die No-Op-Sammelarme (Kontrollfluss- und Deklarations-Direktiven).

## P3 — Offene Fragen, bewusst nicht entschieden

- **`ORI.B #x,An`**: Wir akzeptieren es, die Referenz lehnt es als ungültiges Ziel ab. Permissiver zu sein ist kein Korrektheitsproblem, sollte aber bewusst entschieden und dokumentiert sein.
- **`fileloader.asm`** (Korpus 2) assembliert bei uns, die Referenz bricht mit „branch destination out of range" ab. Unser Verhalten (Word-Form statt Fehler bei disp=0) ist funktional korrekt und nachsichtiger — als Feature dokumentieren.
- **Schreibunterstützung für AmigaDOS**: `amigados.rs` ist read-only, `adf_writer.rs` kann nur leere Images erzeugen und Bootblöcke reparieren. Dateien *schreiben* fehlt. Ob das gebraucht wird, hängt am Emulator-Workflow.

---

## Arbeitsregeln

- Nach jedem Fix: `cargo test --all && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`.
- **Sollbytes immer empirisch gegen die Referenz erzeugen**, nie aus der PRM ableiten und nie raten. Bei diesem Audit lag ich zweimal beim Bitlayout daneben (PFLUSH-Mode, `OR.W (A0),D1`) — der Vergleich hat es sofort gezeigt.
- **`cargo +nightly fuzz` vor dem Merge**, nicht erst in der CI. Bei v2.0.0 fand der CI-Job zwei Panics *nach* dem Merge und blockierte das Release.
- Wenn ein bestehender Test fehlschlägt: erst prüfen, ob der *Test* das falsche Verhalten festschreibt. Ist bei diesem Projekt mehrfach vorgekommen (DC.B-Rundung, EVEN-Padding, Branch-Verkürzung).
- Byte-Parität nach jeder Encoder-Änderung neu messen — sie ist die schärfste Regressionsschranke.

## Verifikation (gesamt)

1. `cargo test --all` — alle bestehenden + neuen Regressionstests grün.
2. Byte-Parität: beide Korpora unverändert 12/12 und 13/13.
3. ROM-Roundtrip: mindestens 93,9% / 94,7% halten (Skript `romcheck.py`, klassifiziert nach `DATA`/`LABELREF`/`REASM`/`MISMATCH` — **nie** nur eine aggregierte Rate messen, die verbirgt Bugs).
4. Fuzzing: alle Targets inkl. des neuen AmigaDOS-Targets ohne Findings.
5. Workbench-Integrationstest (`M68K_WB_DIR=… cargo test -p m68k-floppy --test workbench_disks`) grün.
