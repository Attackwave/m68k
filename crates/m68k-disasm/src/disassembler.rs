//! Two-pass disassembly driver: label discovery followed by formatting.

use std::collections::{HashMap, HashSet};

use m68k_core::addressing::InstructionStream;

use crate::decoder::{DecodeResult, decode_next};

/// Strip a mnemonic's size suffix (`bra.w` -> `bra`, `ftrapeq.l` -> `ftrapeq`).
///
/// The decoder carries the displacement/operand width in the mnemonic so
/// the assembler can rebuild the exact encoding, so control-flow tests
/// have to compare against the stem.
fn mnemonic_stem(mnemonic: &str) -> &str {
    match mnemonic.split_once('.') {
        Some((stem, _)) => stem,
        None => mnemonic,
    }
}

/// True if execution never continues at the following address.
///
/// Only unconditional transfers and returns qualify. A conditional branch
/// falls through by definition, and `bsr`/`jsr` are expected to return —
/// treating either as terminal would cut the trace short at the first
/// call. `trap`/`trapv`/`chk` are not terminal either: their handlers
/// normally return to the following instruction.
fn is_flow_terminator(mnemonic: &str) -> bool {
    matches!(
        mnemonic_stem(mnemonic),
        "rts" | "rte" | "rtr" | "rtd" | "rtm" | "bra" | "jmp" | "stop" | "illegal"
    )
}

/// What a run of non-instruction bytes is, which decides how it renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataKind {
    /// A relocated 32-bit pointer — rendered as `dc.l`, by label where the
    /// target has one.
    Pointer,
    /// A run of printable bytes — rendered as a quoted `dc.b`.
    Text,
    /// Anything else the caller declared as data — rendered as `dc.w`.
    Word,
}

/// Parse a PC trace table: one program counter value per line.
///
/// The format is the one `Oxore/m68k-disasm` uses, so trace files can be
/// shared between the two tools: decimal by default, one value per line.
/// `0x`/`$` prefixed hexadecimal is also accepted, since that is what
/// most emulator logging patches emit without extra formatting work.
/// Blank lines, `#` and `;` comments, and surrounding whitespace are
/// ignored.
///
/// Returns the line number (1-based) and text of the first line that is
/// neither blank, a comment, nor a number — a malformed trace should be
/// reported rather than silently disassembled with most of it missing.
pub fn parse_pc_trace(text: &str) -> Result<Vec<u32>, (usize, String)> {
    let mut pcs = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let parsed = if let Some(hex) = line.strip_prefix("0x").or_else(|| line.strip_prefix('$')) {
            u32::from_str_radix(hex, 16)
        } else {
            line.parse::<u32>()
        };
        match parsed {
            Ok(pc) => pcs.push(pc),
            Err(_) => return Err((i + 1, line.to_string())),
        }
    }
    Ok(pcs)
}

/// Shortest run of consecutive in-image longword pointers taken as a jump
/// table by [`Disassembler::seed_entry_points_from_pointer_tables`].
///
/// Isolated longwords that happen to look like in-image addresses are
/// common in both code and data; four in a row are not.
const MIN_POINTER_TABLE: usize = 4;

/// Shortest run of printable bytes taken as a deliberate string.
///
/// Four is low enough to catch `"OK",0` style fragments padded to a
/// longword, and high enough that ordinary code does not reach it by
/// accident: an m68k instruction pair landing in `0x20..=0x7e` four bytes
/// running is possible but uncommon, and the trace has usually claimed
/// real code before this test is consulted.
const MIN_STRING_RUN: usize = 4;

/// True if `b` would appear inside an ordinary ASCII string constant.
///
/// Tab, newline and carriage return are included because Amiga message
/// strings routinely embed them; other control bytes are not, since they
/// occur far more often in code and data than in text.
fn is_string_byte(b: u8) -> bool {
    matches!(b, 0x20..=0x7E | b'\t' | b'\n' | b'\r')
}

/// Length of the NUL-terminated printable run starting at `data[start]`,
/// including the terminator and any pad byte needed to end on a word.
///
/// Returns 0 when this is not a string, so callers can treat "not a
/// string" and "no bytes consumed" as the same answer. Three conditions
/// must hold:
///
/// - at least [`MIN_STRING_RUN`] printable bytes, so short accidents in
///   code do not qualify;
/// - a NUL terminator, since m68k string constants essentially always
///   have one, and requiring it rejects most printable code;
/// - it ends on a word boundary, padding by one byte if needed.
///
/// **Known imprecision:** when the instruction immediately before a string
/// ends in printable bytes, they are pulled into the string — `rts`
/// (`4E 75`) in front of `"ciab.resource"` renders as
/// `"Nuciab.resource"`. Scanning forward cannot distinguish that from a
/// string genuinely beginning `"Nu"`. The cost is bounded at one word per
/// string and does not shift anything: the line still starts and ends on a
/// word boundary, so following lines stay aligned. Fixing it properly
/// needs the trace to have claimed the preceding instruction, which is
/// exactly what does not happen in the ROM regions where this matters.
fn string_run_len(data: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < data.len() && is_string_byte(data[end]) {
        end += 1;
    }
    if end - start < MIN_STRING_RUN {
        return 0;
    }
    // A terminating NUL belongs to the string rather than to whatever
    // follows it; its absence means this is not a string constant.
    if end >= data.len() || data[end] != 0 {
        return 0;
    }
    end += 1;
    // Instructions are word-aligned, so a data line of odd length would
    // leave every following line decoding at an odd address — the whole
    // listing after it desynchronizes. Round up to the word boundary,
    // absorbing the pad byte (usually a second NUL) into this line.
    let mut len = end - start;
    if !len.is_multiple_of(2) && start + len < data.len() {
        len += 1;
    }
    len
}

/// True if the instruction's `target_address` is somewhere execution can
/// continue, rather than a data address.
///
/// `lea`/`pea` and the many EA-bearing instructions also report a
/// `target_address` (a PC-relative operand address), but that names data
/// being addressed, not code being entered — following those would seed
/// the trace with string and table addresses.
fn targets_code(mnemonic: &str) -> bool {
    let stem = mnemonic_stem(mnemonic);
    matches!(stem, "bra" | "bsr" | "jmp" | "jsr")
        || (stem.starts_with('b') && stem.len() == 3)
        || stem.starts_with("db")
        || stem.starts_with("fb")
        || stem.starts_with("fdb")
}

/// One decoded line of output: an instruction or a data word, with its
/// address and raw bytes preserved for callers that want to render their
/// own listing format (e.g. with a raw-hex column).
#[derive(Debug, Clone)]
pub struct DisassembledLine {
    pub address: u32,
    pub raw_bytes: Vec<u8>,
    pub text: String,
    /// Label defined at this address, when one was discovered.
    ///
    /// Pass 1 assigns names to branch/jump targets and pass 2 substitutes
    /// them into operands, but nothing ever emitted the definitions — the
    /// output referenced 7271 labels and defined none of them, so it could
    /// not be reassembled at all. Callers print this ahead of `text`.
    pub label: Option<String>,
    /// True if decoding failed at this address; `text` then holds an
    /// error description instead of formatted instruction/data output.
    pub is_error: bool,
}

/// Orchestrates the two-pass disassembly of an m68k binary: pass 1 scans
/// the whole input to discover branch/jump targets and assign label names,
/// pass 2 decodes again and formats each instruction, substituting
/// discovered labels for addresses.
pub struct Disassembler {
    data: Vec<u8>,
    origin: u32,
    cpu: String,
    labels: HashMap<u32, String>,
    known_labels: HashMap<u32, String>,
    /// Half-open `[start, end)` address ranges known to hold data rather
    /// than code, from a caller that knows the container format (hunk
    /// relocations, `HUNK_DATA` sections, an explicit `--data` flag).
    data_ranges: Vec<(u32, u32)>,
    /// Addresses a caller knows execution can begin at (a hunk's entry
    /// point, a ROM's reset vector, an explicit `--entry` flag).
    entry_points: Vec<u32>,
    /// Program counter values observed during a real execution, supplied
    /// by [`Disassembler::add_pc_trace`]. Each is an instruction start
    /// that actually ran, so unlike anything derived from the bytes it
    /// cannot be a false positive.
    pc_trace: HashSet<u32>,
    /// Instruction starts proven reachable: the union of `pc_trace` and
    /// what [`Disassembler::trace_reachable_code`] reaches from
    /// `entry_points`, filled at the start of
    /// [`Disassembler::disassemble`]. Empty when the caller supplied
    /// neither, which reduces both passes to a linear scan.
    traced: HashSet<u32>,
}

impl Disassembler {
    pub fn new(data: Vec<u8>, origin: u32) -> Self {
        Self {
            data,
            origin,
            cpu: "68000".to_string(),
            labels: HashMap::new(),
            known_labels: HashMap::new(),
            data_ranges: Vec::new(),
            entry_points: Vec::new(),
            pc_trace: HashSet::new(),
            traced: HashSet::new(),
        }
    }

    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
    }

    /// Seed the label table with known names (e.g. symbols recovered from
    /// an executable's symbol table) before [`Disassembler::disassemble`]
    /// runs. These take priority over auto-generated `label<N>` names at
    /// the same address, and are shown even if pass 1 finds no branch/jump
    /// referencing that address.
    pub fn add_known_labels<I: IntoIterator<Item = (u32, String)>>(&mut self, labels: I) {
        self.known_labels.extend(labels);
    }

    /// Mark half-open `[start, end)` address ranges as data rather than
    /// code.
    ///
    /// This is caller-supplied ground truth, not a guess: a `HUNK_DATA`
    /// section is data by the linker's own account, and so is every
    /// longword named by a relocation. Nothing here is inferred from the
    /// bytes themselves — a caller that only has a raw blob supplies
    /// nothing and gets the previous behavior.
    pub fn add_data_ranges<I: IntoIterator<Item = (u32, u32)>>(&mut self, ranges: I) {
        self.data_ranges
            .extend(ranges.into_iter().filter(|(s, e)| e > s));
    }

    /// Mark the four bytes at each given address as a relocated 32-bit
    /// pointer — a convenience wrapper over [`Disassembler::add_data_ranges`]
    /// for `amiga_hunk`'s reloc lists.
    pub fn add_pointer_sites<I: IntoIterator<Item = u32>>(&mut self, addresses: I) {
        let ranges: Vec<(u32, u32)> = addresses
            .into_iter()
            // A pointer within 4 bytes of the address-space end cannot be
            // represented as a half-open range; such an executable could
            // not have loaded anyway.
            .filter_map(|a| a.checked_add(4).map(|end| (a, end)))
            .collect();
        self.add_data_ranges(ranges);
    }

    /// Register addresses where execution is known to begin.
    ///
    /// Supplying any entry point switches [`Disassembler::disassemble`]
    /// from a purely linear scan to following control flow: only what is
    /// reachable from these addresses is decoded as instructions, and
    /// everything else is emitted as data. Supplying none keeps the linear
    /// behavior.
    ///
    /// Because unreached bytes become data, blind spots in the trace have
    /// to be named here — a ROM's exception vectors, the destinations of
    /// computed jumps, any handler entered from outside the image.
    pub fn add_entry_points<I: IntoIterator<Item = u32>>(&mut self, entries: I) {
        self.entry_points.extend(entries);
    }

    /// Supply program counter values recorded during a real execution of
    /// this image, e.g. from an emulator.
    ///
    /// This is the strongest evidence available about what is code, and
    /// the only kind that does not degrade on the constructs static
    /// analysis cannot see: a PC that was observed executing settles
    /// `jmp (a6)` and jump-table targets without having to reason about
    /// them at all. Every value is taken as an instruction start, and
    /// each is also used as a trace entry point, so straight-line code
    /// between two recorded PCs is recovered without needing a sample for
    /// every instruction.
    ///
    /// Values outside the image and odd values are ignored: the 68000
    /// fetches instructions word-aligned, so an odd PC cannot be an
    /// instruction start and indicates a trace taken from a different
    /// image or a mis-parsed file.
    pub fn add_pc_trace<I: IntoIterator<Item = u32>>(&mut self, pcs: I) {
        let end = self.origin.saturating_add(self.data.len() as u32);
        self.pc_trace.extend(
            pcs.into_iter()
                .filter(|pc| pc.is_multiple_of(2) && *pc >= self.origin && *pc < end),
        );
    }

    /// How many PC trace values were kept — i.e. fell inside the image and
    /// were word-aligned. Lets a caller tell "no trace given" from "a
    /// trace that does not match this image".
    pub fn pc_trace_len(&self) -> usize {
        self.pc_trace.len()
    }

    /// Seed entry points from longword-aligned pointer tables found in the
    /// image itself.
    ///
    /// A ROM has no relocations and no vector table of its own — a
    /// Kickstart image starts with a reset `jmp` and a version string, and
    /// its exception vectors are written to RAM at run time. What it does
    /// contain is jump tables: runs of longwords that all point back into
    /// the image. Those are the targets of the `jmp (a6)`/`jsr (a5)` calls
    /// a trace cannot follow, so seeding them recovers code that is
    /// otherwise unreachable.
    ///
    /// This is a heuristic, unlike [`Disassembler::add_pointer_sites`]:
    /// nothing certifies that a longword which happens to look like an
    /// in-image address is one. Requiring [`MIN_POINTER_TABLE`]
    /// consecutive such longwords is what keeps the false-positive rate
    /// down — isolated coincidences are common, runs of four are not.
    /// Callers that have authoritative entry points should prefer those.
    pub fn seed_entry_points_from_pointer_tables(&mut self) {
        let end = self.origin.saturating_add(self.data.len() as u32);
        let is_in_image = |v: u32| v >= self.origin && v < end && v.is_multiple_of(2);

        // Longword offsets whose value points back into the image.
        let mut pointer_at: Vec<bool> = vec![false; self.data.len() / 4];
        for (i, slot) in pointer_at.iter_mut().enumerate() {
            let raw = &self.data[i * 4..i * 4 + 4];
            *slot = is_in_image(u32::from_be_bytes(raw.try_into().unwrap()));
        }

        // Collect every run of at least MIN_POINTER_TABLE consecutive
        // pointer slots, and take each run's values as entry points.
        let mut found: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < pointer_at.len() {
            if !pointer_at[i] {
                i += 1;
                continue;
            }
            let start = i;
            while i < pointer_at.len() && pointer_at[i] {
                i += 1;
            }
            if i - start >= MIN_POINTER_TABLE {
                for j in start..i {
                    let raw = &self.data[j * 4..j * 4 + 4];
                    found.push(u32::from_be_bytes(raw.try_into().unwrap()));
                }
            }
        }

        self.entry_points.extend(found);
    }

    /// True if `addr` falls inside any range passed to
    /// [`Disassembler::add_data_ranges`].
    fn is_data_address(&self, addr: u32) -> bool {
        self.data_ranges
            .iter()
            .any(|&(start, end)| addr >= start && addr < end)
    }

    /// How many bytes to emit as one data line starting at `addr`.
    ///
    /// A relocated pointer is exactly one longword and is emitted whole, so
    /// its four bytes read as an address rather than two unrelated words.
    /// Any other data run is emitted a word at a time, which keeps the
    /// output word-aligned and lets a range end on an odd boundary without
    /// the line straddling it. Always at least 2 and never past the end of
    /// the input.
    fn data_span_at(&self, addr: u32) -> usize {
        let remaining = (self.data.len() as u64)
            .saturating_sub((addr as u64).saturating_sub(self.origin as u64))
            as usize;
        // A 4-byte range starting exactly here is a pointer site (see
        // `add_pointer_sites`); emit it as one longword.
        let is_pointer = self
            .data_ranges
            .iter()
            .any(|&(start, end)| start == addr && end == addr.saturating_add(4));
        if is_pointer && remaining >= 4 {
            4
        } else {
            2.min(remaining)
        }
    }

    /// Discovered labels after [`Disassembler::disassemble`] has run: maps
    /// absolute address to the generated label name (`label0`, `label1`, ...).
    pub fn labels(&self) -> &HashMap<u32, String> {
        &self.labels
    }

    /// Runs both passes and returns one [`DisassembledLine`] per decoded
    /// instruction or data word. Decode errors produce a line describing
    /// the error and resume two bytes past the failure point, mirroring
    /// the CLI's prior inline behavior.
    pub fn disassemble(&mut self) -> Vec<DisassembledLine> {
        self.traced = self.trace_reachable_code();
        // Recorded PCs are instruction starts by observation, so they hold
        // even where the walk above declined to follow (an undecodable
        // word, a target it could not resolve).
        self.traced.extend(self.pc_trace.iter().copied());
        self.pass1_discover_labels();
        self.pass2_format()
    }

    /// Follow control flow from every entry point, returning the set of
    /// addresses that are provably the start of an instruction.
    ///
    /// This is the answer to what a linear scan cannot know: which bytes
    /// are code. A linear scan starts decoding at every even address in
    /// turn, so a copyright string or jump table becomes instructions.
    /// Tracing only ever decodes what something actually branches to.
    ///
    /// The result is deliberately incomplete — it finds only what is
    /// reachable from the given entry points. Interrupt handlers, vectored
    /// entry points and computed jumps (`jmp (a0,d0.w*4)`, whose target
    /// lives in a table rather than the opword) are invisible here, which
    /// is why callers must name those as entry points themselves.
    fn trace_reachable_code(&self) -> HashSet<u32> {
        let mut reached: HashSet<u32> = HashSet::new();
        // Every recorded PC is also a place to walk from: a trace samples
        // where execution was, not every instruction it passed through, so
        // walking each one recovers the straight-line code between samples
        // and the branches leading out of it.
        let mut queue: Vec<u32> = self
            .entry_points
            .iter()
            .chain(self.pc_trace.iter())
            .copied()
            .collect();

        while let Some(start) = queue.pop() {
            let Some(offset) = self.offset_of(start) else {
                continue;
            };
            let mut stream = InstructionStream::new(&self.data, self.origin);
            stream.seek(offset);

            // Walk straight-line from this entry until the basic block
            // ends, queueing branch targets as we go.
            while stream.remaining() >= 2 {
                let pc = stream.current_pc();
                // Known data is not code, however we got here — a bad
                // target must not turn a pointer table into instructions.
                if self.is_data_address(pc) {
                    break;
                }
                // Already traced: everything downstream of here is too, so
                // stopping avoids re-walking shared tails and makes loops
                // terminate.
                if !reached.insert(pc) {
                    break;
                }
                let Ok((_, DecodeResult::Instruction(inst))) = decode_next(&mut stream, &self.cpu)
                else {
                    // An undecodable word is not code we can follow; the
                    // formatting pass will render it as a data word.
                    reached.remove(&pc);
                    break;
                };
                if let Some(target) = inst.target_address
                    && targets_code(&inst.mnemonic)
                    && self.offset_of(target).is_some()
                {
                    queue.push(target);
                }
                if is_flow_terminator(&inst.mnemonic) {
                    break;
                }
            }
        }

        reached
    }

    /// Byte offset of `addr` within `self.data`, or `None` if it lies
    /// outside the image.
    fn offset_of(&self, addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(self.origin)? as usize;
        (offset < self.data.len()).then_some(offset)
    }

    /// Shorten `len` so a data line at `addr` stops before the first
    /// address the trace proved is an instruction start.
    ///
    /// A string run is found by scanning bytes, which knows nothing about
    /// where code resumes; without this a run of printable bytes ending
    /// just before a traced routine would swallow its first words.
    fn clamp_to_traced(&self, addr: u32, len: usize) -> usize {
        // Traced starts are word-aligned, so only even cut points are
        // reachable; cutting at an odd one would desynchronize every
        // following line the same way an odd-length run does.
        (2..len)
            .step_by(2)
            .find(|&d| self.traced.contains(&addr.wrapping_add(d as u32)))
            .unwrap_or(len)
    }

    /// True if a line starting at `addr` would run into an address the
    /// trace proved is a different instruction's start.
    ///
    /// This catches the case where the trace reached `addr` itself but the
    /// decode there spans further than the trace's own walk did — the
    /// extension words would swallow a known instruction start.
    fn overruns_traced_code(&self, addr: u32, len: usize) -> bool {
        (2..len)
            .step_by(2)
            .any(|d| self.traced.contains(&addr.wrapping_add(d as u32)))
    }

    /// Decide what occupies the line starting at `addr`, given a stream
    /// positioned there.
    ///
    /// Both passes must agree on every line boundary — if they disagreed,
    /// pass 1 would assign labels to addresses pass 2 never emits. Sharing
    /// this decision is what keeps them in lockstep. Returns the decoded
    /// instruction, or `None` when the line is data, leaving the stream
    /// positioned after the line either way.
    fn decode_line(
        &self,
        stream: &mut InstructionStream,
        addr: u32,
    ) -> Result<Result<DecodeResult, DataKind>, String> {
        // Known data outranks everything: a relocated pointer is data on
        // the linker's authority, whatever its bytes look like.
        if self.is_data_address(addr) {
            let span = self.data_span_at(addr);
            stream.seek(stream.offset + span);
            return Ok(Err(if span == 4 {
                DataKind::Pointer
            } else {
                DataKind::Word
            }));
        }

        // Bytes the trace never reached are not thereby data: a ROM's
        // handlers are entered through vector tables and `jmp (a6)`, so
        // "unreached" covers almost all of a Kickstart image. Treating
        // that as data would hide 99.7% of the code. What is decided here
        // is only the narrower question of whether *these* bytes look
        // deliberately like data; everything else falls through to a
        // linear decode, exactly as before tracing existed.
        // A longword pointing at a proven instruction start, sitting where
        // no proven instruction is, is a jump-table entry — the table the
        // `jmp (a1)` read its target from. This is what makes a PC trace
        // pay off twice: the recorded PCs say what is code, and thereby
        // also identify the tables dispatching to it, which a linear
        // decode renders as invented arithmetic. Unlike
        // `seed_entry_points_from_pointer_tables` this needs no run of
        // several entries, because the evidence per entry is far stronger:
        // the target is known-executed code, not merely a plausible
        // address.
        if !self.traced.is_empty()
            && !self.traced.contains(&addr)
            && stream.remaining() >= 4
            && let Ok(raw) = <[u8; 4]>::try_from(&self.data[stream.offset..stream.offset + 4])
            && self.traced.contains(&u32::from_be_bytes(raw))
        {
            stream.seek(stream.offset + 4);
            return Ok(Err(DataKind::Pointer));
        }

        // Only meaningful once a trace has run: without entry points every
        // address is "unreached", and short printable runs are common in
        // ordinary code — `nop; rts` is `4E 71 4E 75`, four printable
        // bytes. Applying the text test then would rewrite plain code
        // listings, so a caller that supplies no entry points keeps the
        // purely linear behavior.
        if !self.traced.is_empty() && !self.traced.contains(&addr) {
            let offset = stream.offset;
            let run = string_run_len(&self.data, offset);
            if run > 0 {
                // Never run past code the trace proved is real.
                let span = self.clamp_to_traced(addr, run);
                stream.seek(offset + span);
                return Ok(Err(DataKind::Text));
            }
        }

        let start_offset = stream.offset;
        let result = decode_next(stream, &self.cpu);

        // A traced address is a proven instruction start, so a decode that
        // would swallow one is wrong however plausible it looks; fall back
        // to a single data word and resynchronize.
        if let Ok((_, DecodeResult::Instruction(ref inst))) = result
            && self.overruns_traced_code(addr, inst.raw_bytes.len())
        {
            stream.seek(start_offset + 2);
            return Ok(Err(DataKind::Word));
        }

        result.map(|(_, decoded)| Ok(decoded))
    }

    fn pass1_discover_labels(&mut self) {
        let mut targets: Vec<u32> = Vec::new();
        // Every address pass 2 will actually start decoding a line from —
        // used below to reject targets that land mid-instruction (never
        // get a line, and thus never get a label, of their own in pass 2)
        // in addition to targets outside the decoded range entirely.
        let mut line_starts: HashSet<u32> = HashSet::new();
        let mut scan_stream = InstructionStream::new(&self.data, self.origin);
        while scan_stream.remaining() >= 2 {
            let line_pc = scan_stream.current_pc();
            let offset_before = scan_stream.offset;
            match self.decode_line(&mut scan_stream, line_pc) {
                Ok(Ok(DecodeResult::Instruction(inst))) => {
                    line_starts.insert(line_pc);
                    if let Some(target) = inst.target_address {
                        targets.push(target);
                    }
                }
                Ok(Ok(DecodeResult::DataWord(dw))) => {
                    line_starts.insert(line_pc);
                    targets.push(dw.address);
                }
                Ok(Err(kind)) => {
                    line_starts.insert(line_pc);
                    // A relocated pointer names an address in this image,
                    // so it deserves a label at the target just as a
                    // branch does.
                    if kind == DataKind::Pointer && scan_stream.offset - offset_before == 4 {
                        let raw = &self.data[offset_before..offset_before + 4];
                        targets.push(u32::from_be_bytes(raw.try_into().unwrap()));
                    }
                }
                Err(_) => break,
            }
        }

        // Branch/jump targets outside the decoded range, or that land
        // mid-instruction rather than on one of pass 2's actual line
        // starts, have no line of their own in pass 2's output — a
        // generated `labelN` name for one would be a dangling reference
        // (`bra labelN` with no `labelN:` anywhere in the listing).
        // format() falls back to rendering the raw address for anything
        // not in `self.labels`. `known_labels` (externally supplied, e.g.
        // from a symbol table) are exempt: the caller vouches for those
        // independently of what this binary slice covers.
        let mut label_idx = 0usize;
        targets.sort();
        targets.dedup();
        self.labels.clone_from(&self.known_labels);
        for addr in targets {
            if !line_starts.contains(&addr) {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(e) = self.labels.entry(addr) {
                e.insert(format!("label{}", label_idx));
                label_idx += 1;
            }
        }
    }

    /// Render a run of known-data bytes as a `dc.l`/`dc.w` directive.
    ///
    /// A 4-byte run is a relocated pointer, so its value is looked up in
    /// the label table — the point of tracking relocations at all is that
    /// `dc.l label3` reassembles to the same pointer, while `dc.l $21f04`
    /// hardcodes a load address.
    fn format_data(&self, raw: &[u8], kind: DataKind) -> String {
        match kind {
            // A relocated pointer, rendered by name where the target has
            // one so the listing reassembles to the same pointer rather
            // than hardcoding a load address.
            DataKind::Pointer if raw.len() == 4 => {
                let value = u32::from_be_bytes(raw.try_into().unwrap());
                match self.labels.get(&value) {
                    Some(name) => format!("dc.l     {}", name),
                    None => format!("dc.l     ${:08x}", value),
                }
            }
            // Printable bytes go inside quotes, anything else (notably the
            // terminating NUL) as a hex byte, so the line reassembles to
            // exactly these bytes.
            DataKind::Text => {
                let mut parts: Vec<String> = Vec::new();
                let mut quoted = String::new();
                for &b in raw {
                    // Only *printable* bytes go inside the quotes. Tab,
                    // newline and carriage return count as string content
                    // for run detection, but emitting them literally would
                    // break the line in two and produce output that cannot
                    // be reassembled.
                    if (0x20..=0x7E).contains(&b) && b != b'"' {
                        quoted.push(b as char);
                    } else {
                        if !quoted.is_empty() {
                            parts.push(format!("\"{}\"", quoted));
                            quoted.clear();
                        }
                        parts.push(format!("${:02x}", b));
                    }
                }
                if !quoted.is_empty() {
                    parts.push(format!("\"{}\"", quoted));
                }
                format!("dc.b     {}", parts.join(","))
            }
            _ => {
                let value = u16::from_be_bytes([raw[0], *raw.get(1).unwrap_or(&0)]);
                format!("dc.w     ${:04x}", value)
            }
        }
    }

    fn pass2_format(&self) -> Vec<DisassembledLine> {
        let mut lines = Vec::new();
        let mut stream = InstructionStream::new(&self.data, self.origin);

        while stream.remaining() >= 2 {
            let inst_pc = stream.current_pc();

            let offset_before = stream.offset;
            match self.decode_line(&mut stream, inst_pc) {
                // Data: declared by the caller, or bytes that positively
                // look like text rather than code.
                Ok(Err(kind)) => {
                    let raw = self.data[offset_before..stream.offset].to_vec();
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        text: self.format_data(&raw, kind),
                        raw_bytes: raw,
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: false,
                    });
                }
                Ok(Ok(DecodeResult::Instruction(inst))) => {
                    let text = inst.format(&self.labels);
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: inst.raw_bytes.clone(),
                        text,
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: false,
                    });
                }
                Ok(Ok(DecodeResult::DataWord(dw))) => {
                    let text = dw.format(&self.labels);
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: dw.raw_bytes.clone(),
                        text,
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: false,
                    });
                }
                Err(e) => {
                    lines.push(DisassembledLine {
                        address: inst_pc,
                        raw_bytes: Vec::new(),
                        text: format!("error at {:08x}: {}", inst_pc, e),
                        label: self.labels.get(&inst_pc).cloned(),
                        is_error: true,
                    });
                    stream.seek(stream.offset + 2);
                }
            }
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disassemble_simple_instructions() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75]; // NOP; RTS
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].address, 0x1000);
        assert_eq!(lines[0].text.trim(), "nop");
        assert!(!lines[0].is_error);
        assert_eq!(lines[1].address, 0x1002);
        assert_eq!(lines[1].text.trim(), "rts");
    }

    #[test]
    fn test_disassemble_discovers_branch_label() {
        // BRA.w disp=$0002 targets pc_after_opword(0x2002)+2 = 0x2004,
        // i.e. the RTS immediately following this 4-byte BRA.w encoding.
        let bytes = vec![0x60, 0x00, 0x00, 0x02, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x2004), Some(&"label0".to_string()));
        assert!(lines[0].text.contains("label0"));
    }

    /// Regression: a branch target outside the decoded range (or landing
    /// mid-instruction rather than on an actual decoded line) must not
    /// get an auto-generated label — pass 2 never emits a line at that
    /// address, so `bra labelN` would reference a `labelN:` that appears
    /// nowhere in the listing. format() must fall back to the raw
    /// address instead.
    #[test]
    fn test_disassemble_out_of_range_target_has_no_dangling_label() {
        // BRA.w with disp pointing well past the end of this 4-byte input.
        let bytes = vec![0x60, 0x00, 0x10, 0x00];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        let lines = disasm.disassemble();

        assert!(disasm.labels().is_empty());
        assert!(lines[0].text.contains("$00003002"));
        assert!(!lines[0].text.contains("label"));
    }

    #[test]
    fn test_disassemble_raw_bytes_preserved() {
        let bytes = vec![0x4E, 0x71];
        let mut disasm = Disassembler::new(bytes.clone(), 0);
        let lines = disasm.disassemble();

        assert_eq!(lines[0].raw_bytes, bytes);
    }

    #[test]
    fn test_disassemble_cpu_gating() {
        // MULS.L Dn (68020+), should decode as data word on 68000
        let bytes = vec![0x4C, 0x00, 0x08, 0x00];
        let mut disasm = Disassembler::new(bytes, 0);
        disasm.set_cpu("68000");
        let lines = disasm.disassemble();

        assert!(lines[0].text.starts_with("dc.w"));
    }

    #[test]
    fn test_disassemble_unmatched_opcode_falls_back_to_data_word() {
        // 0xFFFF matches no opcode pattern; decode_next reports it as a data
        // word rather than an error.
        let bytes = vec![0xFF, 0xFF, 0x4E, 0x75]; // dc.w $ffff; rts
        let mut disasm = Disassembler::new(bytes, 0);
        let lines = disasm.disassemble();

        assert!(!lines[0].is_error);
        assert!(lines[0].text.starts_with("dc.w"));
        assert_eq!(lines[1].address, 2);
        assert_eq!(lines[1].text.trim(), "rts");
    }

    /// A relocated pointer sitting between instructions must be emitted as
    /// a `dc.l`, not decoded. Without the data range its four bytes are
    /// read as an opword and the stream desynchronizes.
    #[test]
    fn test_pointer_site_emits_dc_l_instead_of_decoding() {
        // nop; dc.l $00002000; rts — the longword at 0x1002 is a pointer.
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x20, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text.trim(), "nop");
        assert_eq!(lines[1].address, 0x1002);
        assert_eq!(lines[1].raw_bytes, vec![0x00, 0x00, 0x20, 0x00]);
        assert!(lines[1].text.starts_with("dc.l"));
        // The instruction after the pointer must still be found — i.e. the
        // stream stayed in sync across the data.
        assert_eq!(lines[2].address, 0x1006);
        assert_eq!(lines[2].text.trim(), "rts");
    }

    /// Without the pointer range the same bytes decode as instructions —
    /// this pins down what the range actually changes.
    #[test]
    fn test_pointer_bytes_are_decoded_when_not_marked_as_data() {
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x20, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        assert!(!lines[1].text.starts_with("dc.l"));
    }

    /// A pointer into the image gets a label at its target, and the `dc.l`
    /// references it by name — so the listing reassembles to the same
    /// pointer instead of hardcoding a load address.
    #[test]
    fn test_pointer_target_gets_label_and_is_referenced_by_name() {
        // nop; dc.l $00001008; nop; rts — pointer targets the final rts.
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x10, 0x08, 0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x1008), Some(&"label0".to_string()));
        assert_eq!(lines[1].text.trim(), "dc.l     label0");
        // And the target line carries the definition.
        let target = lines.iter().find(|l| l.address == 0x1008).unwrap();
        assert_eq!(target.label, Some("label0".to_string()));
    }

    /// A pointer whose value lands outside the image must fall back to a
    /// literal, exactly like an out-of-range branch target does.
    #[test]
    fn test_pointer_outside_image_falls_back_to_literal() {
        let bytes = vec![0x4E, 0x71, 0x00, 0xFF, 0x00, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(lines[1].text.trim(), "dc.l     $00ff0000");
        assert!(disasm.labels().is_empty());
    }

    /// A multi-word data range (e.g. a whole HUNK_DATA section) is emitted
    /// word-wise, and decoding resumes at the range's end.
    #[test]
    fn test_data_range_emits_words_and_resumes_after() {
        // nop; <6 bytes of data>; rts
        let bytes = vec![0x4E, 0x71, 0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_data_ranges([(0x1002, 0x1008)]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1].text.trim(), "dc.w     $dead");
        assert_eq!(lines[2].text.trim(), "dc.w     $beef");
        assert_eq!(lines[3].text.trim(), "dc.w     $cafe");
        assert_eq!(lines[4].address, 0x1008);
        assert_eq!(lines[4].text.trim(), "rts");
    }

    /// Empty and inverted ranges are dropped rather than causing the
    /// stream to stall or skip backwards.
    #[test]
    fn test_degenerate_data_ranges_are_ignored() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_data_ranges([(0x1000, 0x1000), (0x1002, 0x1000)]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text.trim(), "nop");
        assert_eq!(lines[1].text.trim(), "rts");
    }

    #[test]
    fn test_mnemonic_stem_strips_size_suffix() {
        assert_eq!(mnemonic_stem("bra.w"), "bra");
        assert_eq!(mnemonic_stem("rts"), "rts");
        assert_eq!(mnemonic_stem("ftrapeq.l"), "ftrapeq");
    }

    /// Conditional branches and calls fall through; only unconditional
    /// transfers and returns end a block. Getting this wrong would either
    /// cut the trace at the first `bsr` or run it past an `rts` into
    /// whatever follows.
    #[test]
    fn test_flow_terminator_classification() {
        for m in ["rts", "rte", "rtr", "bra.w", "jmp", "stop", "illegal"] {
            assert!(is_flow_terminator(m), "{} should terminate", m);
        }
        for m in ["bsr.w", "jsr", "beq.w", "nop", "trap", "dbra", "move.l"] {
            assert!(!is_flow_terminator(m), "{} should not terminate", m);
        }
    }

    /// Only branch/jump targets are code. `lea`/`pea` also report a
    /// target_address, but it names data being addressed — following it
    /// would seed the trace with string and table addresses.
    #[test]
    fn test_targets_code_classification() {
        for m in [
            "bra.w", "bsr.w", "jmp", "jsr", "beq.w", "bne.s", "dbra", "fbeq.w",
        ] {
            assert!(targets_code(m), "{} should target code", m);
        }
        for m in ["lea", "pea", "move.l", "nop"] {
            assert!(!targets_code(m), "{} should not target code", m);
        }
    }

    /// The core of the feature: bytes that are never branched to are not
    /// decoded as instructions. Here an ASCII string sits after an `rts`,
    /// exactly as a copyright message does in a ROM.
    #[test]
    fn test_trace_does_not_decode_string_after_rts() {
        // nop; rts; "Hello!",0  — the string is unreachable.
        let mut bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        bytes.extend_from_slice(b"Hello!\x00\x00");
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        // The two real instructions are traced...
        assert_eq!(lines[0].text.trim(), "nop");
        assert_eq!(lines[1].text.trim(), "rts");
        // ...and "He", "ll", "o!" are not claimed to be instructions.
        let tail: Vec<&str> = lines[2..].iter().map(|l| l.text.trim()).collect();
        assert!(
            tail.iter().all(|t| t.starts_with("dc.")),
            "unreachable string decoded as code: {:?}",
            tail
        );
    }

    /// Without entry points nothing is traced, and the linear scan decodes
    /// the same string as instructions — this pins down what tracing buys
    /// and that the old behavior is still available.
    #[test]
    fn test_without_entry_points_string_is_decoded_linearly() {
        let mut bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        bytes.extend_from_slice(b"Hello!");
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        let tail: Vec<&str> = lines[2..].iter().map(|l| l.text.trim()).collect();
        assert!(
            !tail.iter().all(|t| t.starts_with("dc.")),
            "expected linear scan to decode the string as instructions"
        );
    }

    /// A backward branch must not make the trace loop forever.
    #[test]
    fn test_trace_terminates_on_backward_branch() {
        // loop: bra.w loop  (displacement -4 from pc_after_opword)
        let bytes = vec![0x60, 0x00, 0xFF, 0xFE];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("bra"));
    }

    /// Execution continues after a `bsr`, so the instruction following a
    /// call must still be traced.
    #[test]
    fn test_trace_continues_after_bsr() {
        // bsr.w sub; rts; sub: nop; rts
        let bytes = vec![
            0x61, 0x00, 0x00, 0x04, // bsr.w -> 0x1006
            0x4E, 0x75, // rts
            0x4E, 0x71, // sub: nop
            0x4E, 0x75, // rts
        ];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 4);
        // The rts right after the bsr is reached by fall-through.
        assert_eq!(lines[1].address, 0x1004);
        assert_eq!(lines[1].text.trim(), "rts");
        // The subroutine is reached via the branch target.
        assert_eq!(lines[2].address, 0x1006);
        assert_eq!(lines[2].text.trim(), "nop");
    }

    /// Data ranges outrank the trace: a bad entry point must not turn a
    /// declared pointer into instructions.
    #[test]
    fn test_trace_stops_at_declared_data() {
        // nop; <pointer>; rts — entry point traces into the pointer.
        let bytes = vec![0x4E, 0x71, 0x00, 0x00, 0x20, 0x00, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        disasm.add_pointer_sites([0x1002]);
        let lines = disasm.disassemble();

        assert_eq!(lines[1].address, 0x1002);
        assert!(lines[1].text.starts_with("dc.l"));
    }

    /// An entry point outside the image is ignored rather than panicking.
    #[test]
    fn test_entry_point_outside_image_is_ignored() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x9999, 0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text.trim(), "nop");
    }

    /// Code reached only through a conditional branch must still be
    /// traced — the branch falls through *and* takes its target.
    #[test]
    fn test_trace_follows_both_paths_of_conditional_branch() {
        // beq.w skip; nop; skip: rts
        let bytes = vec![
            0x67, 0x00, 0x00, 0x04, // beq.w -> 0x1006
            0x4E, 0x71, // nop (fall-through)
            0x4E, 0x75, // skip: rts
        ];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].text.trim(), "nop");
        assert_eq!(lines[2].text.trim(), "rts");
    }

    #[test]
    fn test_string_run_requires_minimum_length() {
        // Three printable bytes is below the threshold.
        assert_eq!(string_run_len(b"abc\x00\x00\x00", 0), 0);
        // Four reaches it, and the NUL terminator is absorbed.
        assert_eq!(string_run_len(b"abcd\x00\x00", 0), 6);
    }

    /// A data line of odd length would leave every following line decoding
    /// at an odd address, desynchronizing the whole listing after it.
    #[test]
    fn test_string_run_length_is_always_even() {
        // "abcde" + NUL = 6, already even.
        assert_eq!(string_run_len(b"abcde\x00zz", 0), 6);
        // "abcd" + NUL = 5, padded up to 6.
        assert_eq!(string_run_len(b"abcd\x00zz", 0), 6);
    }

    /// Printable bytes without a NUL terminator are not a string. This is
    /// what keeps most printable *code* from being misread as text.
    #[test]
    fn test_unterminated_run_is_not_a_string() {
        assert_eq!(string_run_len(b"abcde\x01z", 0), 0);
    }

    /// The text test only runs once a trace has: without entry points
    /// every address is "unreached", and `nop; rts` is four printable
    /// bytes (`4E 71 4E 75`) that must not become a string.
    #[test]
    fn test_text_detection_is_off_without_entry_points() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        let lines = disasm.disassemble();

        assert_eq!(lines[0].text.trim(), "nop");
        assert_eq!(lines[1].text.trim(), "rts");
    }

    /// Stage 3: unreached bytes are decoded linearly as before, *unless*
    /// they positively look like text. This is what keeps a ROM readable
    /// — treating everything unreached as data hid 99.7% of Kickstart.
    #[test]
    fn test_unreached_code_is_still_decoded_but_strings_are_not() {
        // entry: rts | <unreached nop> | "Hello!\0"
        let mut bytes = vec![0x4E, 0x75, 0x4E, 0x71];
        bytes.extend_from_slice(b"Hello!\x00\x00");
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines[0].text.trim(), "rts");
        // The string is rendered as text rather than decoded as
        // arithmetic. The preceding `nop` is printable ("Nq") and gets
        // absorbed into it — the documented imprecision on
        // `string_run_len`; it costs one word and shifts nothing.
        assert!(
            lines[1].text.contains("Hello!"),
            "expected quoted string, got {:?}",
            lines[1].text
        );
    }

    /// A string run must stop before code the trace proved is real, or it
    /// would swallow the first words of a routine.
    #[test]
    fn test_string_run_stops_at_traced_code() {
        // bra.w over printable filler, landing on the rts at 0x1008.
        let bytes = vec![
            0x60, 0x00, 0x00, 0x06, // bra.w -> 0x1008
            0x41, 0x42, 0x43, 0x44, // "ABCD" filler (unreached)
            0x4E, 0x75, // rts <- traced
        ];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        let lines = disasm.disassemble();

        let rts = lines.iter().find(|l| l.address == 0x1008);
        assert!(
            rts.is_some_and(|l| l.text.trim() == "rts"),
            "traced rts swallowed by string run: {:?}",
            lines
                .iter()
                .map(|l| (l.address, l.text.trim()))
                .collect::<Vec<_>>()
        );
    }

    /// Non-printable bytes inside a text line are emitted as hex, so the
    /// line reassembles to exactly the original bytes.
    #[test]
    fn test_text_line_renders_terminator_as_hex() {
        let bytes = b"Hi!\x00".to_vec();
        let disasm = Disassembler::new(bytes.clone(), 0x1000);
        assert_eq!(
            disasm.format_data(&bytes, DataKind::Text),
            "dc.b     \"Hi!\",$00"
        );
    }

    /// A run of in-image longwords is taken as a jump table, and its
    /// entries become entry points — this is how code reached only through
    /// `jmp (a6)` is recovered.
    #[test]
    fn test_pointer_table_seeds_entry_points() {
        // A 4-entry table pointing at four routines, then the routines.
        let mut bytes = Vec::new();
        for target in [0x1010u32, 0x1012, 0x1014, 0x1016] {
            bytes.extend_from_slice(&target.to_be_bytes());
        }
        // 0x1010..0x1017: four rts instructions.
        bytes.extend_from_slice(&[0x4E, 0x75, 0x4E, 0x75, 0x4E, 0x75, 0x4E, 0x75]);

        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.seed_entry_points_from_pointer_tables();
        let lines = disasm.disassemble();

        // Each routine is decoded as code rather than left as data.
        for addr in [0x1010u32, 0x1012, 0x1014, 0x1016] {
            let line = lines.iter().find(|l| l.address == addr);
            assert!(
                line.is_some_and(|l| l.text.trim() == "rts"),
                "table target {:#x} not decoded as code",
                addr
            );
        }
    }

    /// Runs shorter than MIN_POINTER_TABLE are ignored: isolated longwords
    /// that happen to look like addresses are common.
    #[test]
    fn test_short_pointer_run_is_not_a_table() {
        let mut bytes = Vec::new();
        // Only two plausible pointers, then non-pointer data.
        for target in [0x1010u32, 0x1012] {
            bytes.extend_from_slice(&target.to_be_bytes());
        }
        bytes.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        bytes.extend_from_slice(&[0x4E, 0x75, 0x4E, 0x75]);

        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.seed_entry_points_from_pointer_tables();
        disasm.disassemble();

        assert!(
            disasm.entry_points.is_empty(),
            "short run wrongly taken as a table: {:?}",
            disasm.entry_points
        );
    }

    /// Longwords pointing outside the image are not table entries.
    #[test]
    fn test_out_of_image_pointers_are_not_a_table() {
        let mut bytes = Vec::new();
        for target in [0x90000u32, 0x90004, 0x90008, 0x9000C] {
            bytes.extend_from_slice(&target.to_be_bytes());
        }
        bytes.extend_from_slice(&[0x4E, 0x75, 0x4E, 0x75]);

        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.seed_entry_points_from_pointer_tables();

        assert!(disasm.entry_points.is_empty());
    }

    #[test]
    fn test_parse_pc_trace_accepts_decimal_hex_and_comments() {
        let text = "512\n518\n\n# a comment\n0x1000\n$2000\n; another\n  520  \n";
        assert_eq!(
            parse_pc_trace(text).unwrap(),
            vec![512, 518, 0x1000, 0x2000, 520]
        );
    }

    /// A malformed trace must be reported, not silently half-ignored:
    /// disassembling with most of the trace missing looks like the option
    /// having no effect.
    #[test]
    fn test_parse_pc_trace_reports_bad_line() {
        let (line, text) = parse_pc_trace("512\nnot-a-number\n520\n").unwrap_err();
        assert_eq!(line, 2);
        assert_eq!(text, "not-a-number");
    }

    /// Recorded PCs are instruction starts by observation. This is the
    /// case static analysis cannot reach: a routine entered only through a
    /// computed jump.
    #[test]
    fn test_pc_trace_recovers_code_no_static_analysis_can_reach() {
        // entry: jmp (a0) — target unknowable statically; then a routine
        // at 0x1004 that nothing branches to, then a string.
        let mut bytes = vec![
            0x4E, 0xD0, // jmp (a0)
            0x4E, 0x71, // nop (unreachable statically)
            0x4E, 0x71, // 0x1004: nop
            0x4E, 0x75, // rts
        ];
        bytes.extend_from_slice(b"Hi!!\x00\x00");

        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_entry_points([0x1000]);
        disasm.add_pc_trace([0x1004]);
        let lines = disasm.disassemble();

        // The traced PC is decoded as code...
        let at = |a: u32| {
            lines
                .iter()
                .find(|l| l.address == a)
                .map(|l| l.text.trim().to_string())
        };
        assert_eq!(at(0x1004).as_deref(), Some("nop"));
        // ...and the walk continues from it to the following rts.
        assert_eq!(at(0x1006).as_deref(), Some("rts"));
        // The string is still recognized as data.
        assert!(at(0x1008).is_some_and(|t| t.contains("Hi!!")));
    }

    /// PCs outside the image or at odd addresses cannot be instruction
    /// starts on m68k and are dropped rather than trusted.
    #[test]
    fn test_pc_trace_rejects_odd_and_out_of_range_values() {
        let bytes = vec![0x4E, 0x71, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pc_trace([0x1001, 0x9999, 0x1002]);

        assert_eq!(disasm.pc_trace_len(), 1);
    }

    /// A trace alone, with no entry points, must be enough to switch on
    /// tracing — otherwise `-t` without `--entry` would silently do
    /// nothing.
    #[test]
    fn test_pc_trace_alone_enables_tracing() {
        let mut bytes = vec![0x4E, 0x75];
        bytes.extend_from_slice(b"Test\x00\x00");
        let mut disasm = Disassembler::new(bytes, 0x1000);
        disasm.add_pc_trace([0x1000]);
        let lines = disasm.disassemble();

        assert_eq!(lines[0].text.trim(), "rts");
        assert!(lines[1].text.contains("Test"));
    }

    #[test]
    fn test_known_labels_take_priority_over_generated_names() {
        // BRA.S +2 (to the RTS below), then RTS — same target as
        // test_disassemble_discovers_branch_label, but the target address
        // is pre-seeded with a known symbol name.
        let bytes = vec![0x60, 0x02, 0x4E, 0x75];
        let mut disasm = Disassembler::new(bytes, 0x2000);
        disasm.add_known_labels([(0x2004, "myFunc".to_string())]);
        let lines = disasm.disassemble();

        assert_eq!(disasm.labels().get(&0x2004), Some(&"myFunc".to_string()));
        assert!(lines[0].text.contains("myFunc"));
    }
}
