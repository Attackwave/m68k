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

### P1.3 README dokumentiert stdin-Unterstützung, die nicht existiert — ✅ erledigt (PR #21)

`README.md:31` (`m68k-disasm`) und `README.md:110` (`m68k-asm`) versprechen beide „use `-` for standard input". **Verifiziert: beide brechen ab** mit `cannot read '-': No such file or directory`.

**Aufgabe:** entweder implementieren (beide Binaries, `-` → stdin lesen; bei `m68k-asm` zusätzlich `-o -` → stdout) oder aus dem README streichen. Implementieren ist vorzuziehen — für Pipelines im Emulator-Workflow ist das nützlich.

### P1.4 `PLPAR`/`PLPAW` sind kodierbar, aber nicht dekodierbar

**Verifiziert:** `PLPAR (A0)` / `PLPAW (A1)` assemblieren korrekt zu `F5C8`/`F589` — byte-identisch zur Referenz. Der Disassembler hat aber **kein Pattern dafür** und gibt stattdessen `cinvl bc,(a0)` / `cinvl ic,(a1)` aus. Der Roundtrip ist damit gebrochen: reassembliert man die Ausgabe, kommen andere Bytes heraus.

Betrifft 68060-Code. Der Assembler kann die Instruktionen (`encoder.rs`), `opcodes.rs` fehlt der Eintrag.

**Aufgabe:** `OpcodePattern`-Einträge für PLPAR/PLPAW in `crates/m68k-core/src/opcodes.rs` ergänzen, **vor** dem CACHE-Pattern (dessen Maske sie sonst verschluckt — anders als bei den Fällen unten scheitert hier das Operanden-Parsing *nicht*, das breitere Pattern greift also wirklich). Sollbytes: `F5C8` = `PLPAR (A0)`, `F589` = `PLPAW (A1)`. Danach in `instruction_coverage.rs` aufnehmen.

### P1.5 `m68k-floppy` validiert `--cpu` nicht — ✅ gegenstandslos (2026-08-03)

**Nachgeprüft:** `m68k-floppy` hat gar kein `--cpu`-Flag mehr (`--help` zeigt nur `--backend`, `--bootblock`, `--sector`, `--list(-all)`, `--extract(-all)`, `-o`, `--tracks`, `--fix-bootblock`). Die im Audit beschriebene Inkonsistenz existiert nicht mehr; nichts zu tun. Ursprünglicher Befund:



`m68k-asm` und `m68k-disasm` rufen beide `validate_cpu_name` auf, `m68k-floppy` nicht (verifiziert per grep). Aktuell folgenlos, weil das Binary die CPU nicht nutzt — aber inkonsistent, und wenn es später Disassembly-Optionen bekommt, wird es zur stillen Fehlerquelle.

**Aufgabe:** entweder `--cpu` dort entfernen (falls ungenutzt) oder validieren.

---

## P2 — Qualität und Wartbarkeit

### P2.1 `AGENTS.md` ist stale — ✅ erledigt (2026-08-03)

Zeile 13 nennt „570 passed" — aktuell sind es 595. Die Modul-LOC-Angaben stammen von PR #13. Da die Datei als Projektstand dient, führt das beim nächsten Antasten in die Irre.

**Aufgabe:** Testzahlen, LOC-Angaben und den „Stand"-Block auf v2.0.1 aktualisieren.

**Ergebnis:** Stand-Block auf 2026-08-03 / 611 Tests, alle LOC-Angaben per `wc -l` neu gemessen (assembler.rs war mit ~4200+ gegen real 6754 am weitesten daneben), CI-Beschreibung, MSRV-Zeile und `cargo audit`-Status ergänzt. Zeile 8 nannte Rust 1.93.0 - jetzt MSRV 1.88 / lokal 1.97.1.

### P2.2 14 stille `unwrap_or(0)` / `unwrap_or_default()` in Encoder-Pfaden — ✅ erledigt (2026-08-03)

`grep` findet 14 Stellen in `crates/m68k-asm/src/` und `crates/m68k-core/src/` außerhalb von Tests. Jede davon kann einen Parse- oder Auswertungsfehler in eine stille 0 verwandeln — exakt das Muster, das bei `evaluate_simple_number` zu den PC-relativen und Displacement-Bugs geführt hat.

**Aufgabe:** jede Stelle einzeln bewerten: Ist 0 ein legitimer Default (dann Kommentar warum) oder verschluckt sie einen Fehler (dann `?` propagieren)? Nicht pauschal ersetzen — bei Pass-1-Größenschätzungen ist der Fallback teils gewollt.

**Ergebnis:** alle 14 einzeln bewertet. **Ein echter Bug gefunden und gefixt:** `REPT $ZZ` (malformiertes Hex) wurde per `unwrap_or(0)` zu `REPT 0` - der Block verschwand ersatzlos, während die Referenz die Zeile ablehnt. **Empirisch verifiziert** (vasm: `error 76: base 16 numerical term expected`; wir: 2 Instruktionen statt 3, der NOP fehlte). Der erste Fixversuch (Zeile nur durchreichen) griff zu kurz, weil `rept` in `is_directive_name` steht und dann in einem No-Op-Arm landet - der Body wäre einmal ungeschützt assembliert worden. Jetzt echter Diagnostic über `self.errors.error`, Exit-Code 1, keine Ausgabe. Regressionstest ergänzt.

Die übrigen 13 sind legitim, jetzt aber begründet: `line_no.unwrap_or(0)` (Zeilennummer unbekannt) x2, Makro-Parametersubstitution (nicht übergeben -> leer, dokumentiertes Verhalten) x2, `chunks(2)`-Restbyte, Pass-1-Größenschätzung (war bereits ausführlich kommentiert), `base_reg: None` = Basisregister unterdrückt (Kommentar ergänzt), `.max()` nach `is_empty()`-Guard = unerreichbar (Kommentar ergänzt), S-Record-Prüfsumme über selbst formatierten Hex-String (`debug_assert!` ergänzt statt Fehlerkanal für einen strukturell unmöglichen Fall), Rest in Testmodulen.

### P2.3 Kein MSRV, keine `rust-toolchain`-Datei — ✅ erledigt (2026-08-03)

Weder `rust-version` in einer `Cargo.toml` noch eine `rust-toolchain.toml`. Der Code nutzt aber neuere Features (let-chains, `is_multiple_of`), die eine recht aktuelle Toolchain verlangen. CI läuft auf `stable` — ein Nutzer mit älterem Rust bekommt kryptische Compilerfehler.

**Aufgabe:** `rust-version` im Workspace deklarieren (die tatsächlich benötigte Version ermitteln, nicht raten) und optional `rust-toolchain.toml` für reproduzierbare Builds.

**Ergebnis:** MSRV **1.88**, empirisch bestimmt statt geraten: 1.87 scheitert mit `E0658` an den let-chains in `m68k-core`, 1.88 hat sie stabilisiert und baut sauber. Obere Kante gegen 1.97.1 (aktuelles stable) mitgeprüft - volle Prüfkette dort grün. `rust-version` im `[workspace.package]`, per `rust-version.workspace = true` an alle fünf Crates vererbt; cargo meldet auf 1.87 jetzt "requires rustc 1.88" statt eines kryptischen Compilerfehlers (verifiziert).

**`rust-toolchain.toml` bewusst weggelassen:** sie würde jeden Contributor auf eine feste Version zwingen und in CI auch den MSRV-Job überschreiben, der damit wirkungslos wäre.

### P2.4 CI testet nur eine Plattform, eine Toolchain — ✅ erledigt (2026-08-03)

`.github/workflows/ci.yml` läuft ausschließlich auf `ubuntu-latest` mit `stable`. Für ein Tool, das laut README auch anderswo laufen soll, fehlt zumindest ein Windows- oder macOS-Job. Auch `cargo audit` läuft nicht in CI (AGENTS.md erwähnt es als manuell ausgeführt).

**Aufgabe:** Matrix um `windows-latest` erweitern; `cargo audit` als eigenen (nicht blockierenden) Job.

**Ergebnis:** `build` ist jetzt eine Matrix über ubuntu/windows/macos mit `fail-fast: false`, damit ein plattformspezifischer Bruch von einer echten Regression unterscheidbar bleibt. Build+Test laufen überall; `fmt`, `clippy` und die Release-Artefakte nur auf ubuntu (`matrix.primary`), da plattformunabhängig. Dazu zwei neue Jobs: `msrv` (`cargo check` auf 1.88.0) und `audit` (`continue-on-error`, weil die Advisory-DB sich unabhängig vom Repo ändert und ein neuer Eintrag nicht jeden unbeteiligten PR rot färben darf).

Vorab lokal auf plattformabhängige Annahmen geprüft: durchweg `std::env::temp_dir()` statt hartkodiertem `/tmp`, `.lines()` verträgt CRLF, die `readelf`-Tests überspringen sich sauber, wenn das Tool fehlt.

### P2.5 Golden-Vektoren sind eingefroren und decken die Neuzugänge nicht — ✅ entschieden (2026-08-03)

`tests/golden/vectors.json` hat 123 Vektoren und ist laut AGENTS.md „nicht mehr regenerierbar". Alle seit v1.0.2 hinzugekommenen Instruktionsformen (MMU, k-factor, die v2.0.1-Fixes) sind dort nicht vertreten.

**Aufgabe:** entscheiden, ob die Golden-Vektoren eingefroren bleiben (dann dokumentieren, dass `instruction_coverage.rs` die maßgebliche Suite ist) oder ob sie kontrolliert erweitert werden. Nicht beides halb.

**Entscheidung: eingefroren.** Der Wert des Snapshots liegt gerade darin, dass er sich nicht bewegt - er stammt von v1.0.2 und fängt damit Regressionen, auf die sich Encoder und eine frisch regenerierte Erwartung gemeinsam einigen würden. Eine aus der aktuellen Implementierung regenerierte Datei könnte das nicht.

Dokumentiert an drei Stellen: Modulkommentar in `golden_assembler.rs` (warum eingefroren), Modulkommentar in `instruction_coverage.rs` (dies ist die maßgebliche Suite, neue Formen kommen hierher) und AGENTS.md.

---

### P2.6 CNOP: Padding-Formel in beiden Pässen dupliziert — ✅ erledigt (2026-08-03)

`estimate_directive_size` (Zeile 2602) und `encode_directive` (Zeile 3647) berechnen das CNOP-Padding mit derselben, aber **getrennt hingeschriebenen** Formel `alignment - (target % alignment)`. `EVEN`, `ALIGN` und `INCBIN` nutzen dagegen je einen gemeinsamen `handle_*_pass1`/`pass2`-Kern.

**Verifiziert:** Aktuell kein Zahlenunterschied — auch nicht mit einem `SET`-Symbol als Alignment (getestet: `ALIGNVAL SET 4` → beide Pässe liefern dasselbe, Bytes identisch zur Referenz). Es ist ein Wartungsrisiko, kein Bug: Genau diese Divergenzklasse hat im Juli-Audit sechs Label-Korruptionen verursacht.

**Aufgabe:** Padding-Berechnung in einen gemeinsamen Helfer ziehen, analog zu `handle_align_pass1`.

**Ergebnis:** neue Methode `Assembler::cnop_padding(args, line_no)` kapselt Argumentauswertung, Validierung und Padding-Formel; beide Pässe rufen sie auf. (Randnotiz: das im Audit genannte Vorbild `handle_align_pass1` existiert nicht - EVEN/ALIGN/INCBIN sind anders strukturiert.) Die Validierung läuft bewusst bei jedem Aufruf statt als aus Pass 1 übernommen, weil ein `SET`-Symbol als Alignment zwischen den Pässen den Wert aendern kann. Regressionstest für genau diesen Fall ergänzt - plan.md nannte ihn als getestet, ein Test existierte aber nicht.

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
