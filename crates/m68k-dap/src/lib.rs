//! Debug Adapter Protocol server for Amiga hunk executables running in a
//! UAE emulator with a GDB remote debugger.
//!
//! One event loop handles both sides: DAP requests from the editor and RSP
//! packets from the emulator arrive on the same channel, so a stop reply
//! that shows up while a request is in flight is queued, never lost.

pub mod program;
pub mod rsp;

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use m68k_core::addressing::InstructionStream;
use m68k_disasm::decoder::{DecodeResult, decode_next};
use serde_json::{Value, json};

use program::Program;

/// The CPU thread; UAE's stub also reports the copper as thread 2.
const CPU: u32 = 1;
/// Bus error, address error, illegal instruction, zero divide.
const EXCEPTION_MASK: u32 = 0b11_1100;
/// Upper bound on single steps for one line step, so a tight loop on a
/// single line cannot hang the session.
const MAX_STEPS: usize = 100_000;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const CALL_TIMEOUT: Duration = Duration::from_secs(24 * 3600);

pub enum Msg {
    Dap(Value),
    DapClosed,
    Gdb(String),
    GdbClosed,
}

/// Read DAP messages (`Content-Length` framed JSON) until EOF.
pub fn read_dap(input: impl Read, tx: Sender<Msg>) {
    let mut input = std::io::BufReader::new(input);
    loop {
        let mut len = None;
        loop {
            let mut header = String::new();
            if input.read_line(&mut header).unwrap_or(0) == 0 {
                let _ = tx.send(Msg::DapClosed);
                return;
            }
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            if let Some(v) = header.strip_prefix("Content-Length:") {
                len = v.trim().parse::<usize>().ok();
            }
        }
        let mut body = vec![0; len.unwrap_or(0)];
        if input.read_exact(&mut body).is_err() {
            let _ = tx.send(Msg::DapClosed);
            return;
        }
        if let Ok(v) = serde_json::from_slice(&body) {
            let _ = tx.send(Msg::Dap(v));
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Step {
    Into,
    Over,
    Out,
}

pub struct Session<W: Write> {
    out: W,
    seq: i64,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    deferred: VecDeque<Msg>,
    gdb: Option<TcpStream>,
    emulator: Option<Child>,
    program: Program,
    /// Breakpoint addresses per source path, as last set by the editor.
    breakpoints: HashMap<String, Vec<u32>>,
    stop_on_entry: bool,
    pausing: bool,
    /// Work that must follow the response: DAP wants the reply to `next`
    /// (or `configurationDone`) before the `stopped` event it leads to.
    after_response: Option<After>,
}

enum After {
    Step(Step, bool),
    Stopped(&'static str),
}

impl<W: Write> Session<W> {
    pub fn new(out: W, tx: Sender<Msg>, rx: Receiver<Msg>) -> Self {
        Session {
            out,
            seq: 1,
            tx,
            rx,
            deferred: VecDeque::new(),
            gdb: None,
            emulator: None,
            program: Program::default(),
            breakpoints: HashMap::new(),
            stop_on_entry: false,
            pausing: false,
            after_response: None,
        }
    }

    /// Serve until the editor disconnects or the emulator goes away.
    pub fn run(mut self) {
        loop {
            let msg = match self.deferred.pop_front() {
                Some(m) => m,
                None => match self.rx.recv() {
                    Ok(m) => m,
                    Err(_) => break,
                },
            };
            match msg {
                Msg::Dap(req) => {
                    let command = req["command"].as_str().unwrap_or_default().to_string();
                    let result = self.handle(&command, &req["arguments"]);
                    match result {
                        Ok(body) => self.respond(&req, true, body, None),
                        Err(e) => self.respond(&req, false, Value::Null, Some(e)),
                    }
                    match self.after_response.take() {
                        Some(After::Step(kind, by_instruction)) => {
                            if let Err(e) = self.step(kind, by_instruction) {
                                self.output(&format!("step failed: {e}\n"));
                                self.stopped("step");
                            }
                        }
                        Some(After::Stopped(reason)) => self.stopped(reason),
                        None => {}
                    }
                    if command == "disconnect" {
                        break;
                    }
                }
                Msg::Gdb(packet) => self.on_async_packet(&packet),
                Msg::DapClosed => break,
                Msg::GdbClosed => {
                    self.event("terminated", json!({}));
                    break;
                }
            }
        }
        self.shutdown();
    }

    fn handle(&mut self, command: &str, args: &Value) -> Result<Value, String> {
        match command {
            "initialize" => Ok(json!({
                "supportsConfigurationDoneRequest": true,
                "supportsReadMemoryRequest": true,
                "supportsSteppingGranularity": true,
                "supportsEvaluateForHovers": true,
            })),
            "launch" => {
                self.launch(args)?;
                self.event("initialized", json!({}));
                Ok(Value::Null)
            }
            "setBreakpoints" => self.set_breakpoints(args),
            "configurationDone" => {
                if self.stop_on_entry {
                    self.after_response = Some(After::Stopped("entry"));
                } else {
                    self.resume()?;
                }
                Ok(Value::Null)
            }
            "threads" => Ok(json!({ "threads": [{ "id": CPU, "name": "CPU" }] })),
            "stackTrace" => self.stack_trace(),
            "scopes" => Ok(json!({
                "scopes": [{ "name": "Registers", "variablesReference": 1, "expensive": false }]
            })),
            "variables" => self.variables(),
            "continue" => {
                self.resume()?;
                Ok(json!({ "allThreadsContinued": true }))
            }
            "next" | "stepIn" | "stepOut" => {
                let kind = match command {
                    "next" => Step::Over,
                    "stepIn" => Step::Into,
                    _ => Step::Out,
                };
                let by_instruction = args["granularity"].as_str() == Some("instruction");
                self.after_response = Some(After::Step(kind, by_instruction));
                Ok(Value::Null)
            }
            "pause" => {
                self.pausing = true;
                self.send_gdb(&format!("vCont;t:{CPU}"))?;
                Ok(Value::Null)
            }
            "readMemory" => self.read_memory(args),
            "evaluate" => self.evaluate(args),
            "disconnect" => Ok(Value::Null),
            other => Err(format!("unsupported request: {other}")),
        }
    }

    // ----- launch ---------------------------------------------------------

    fn launch(&mut self, args: &Value) -> Result<(), String> {
        let program = PathBuf::from(
            args["program"]
                .as_str()
                .ok_or("launch: 'program' is required")?,
        );
        let exe = std::fs::read(&program).map_err(|e| format!("{}: {e}", program.display()))?;
        let port = args["serverPort"].as_u64().unwrap_or(2345) as u16;
        self.stop_on_entry = args["stopOnEntry"].as_bool().unwrap_or(false);

        if let Some(emulator) = args["emulator"].as_str() {
            let extra: Vec<String> = args["emulatorArgs"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            self.start_emulator(emulator, &program, port, &extra)?;
        }
        self.connect(port)?;

        self.request(&format!(
            "Z1,0,0;X{:x},{:x}",
            format!("{EXCEPTION_MASK:x}").len(),
            EXCEPTION_MASK
        ))
        .ok(); // older stubs lack exception breakpoints; not fatal
        let offsets = self.request("qOffsets")?;
        let segments: Vec<u32> = offsets
            .split(';')
            .filter_map(|p| u32::from_str_radix(p.rsplit('=').next()?, 16).ok())
            .collect();
        self.program = Program::load(&exe, &segments)?;
        if self.program.lines.is_empty() {
            self.output("No line debug info in the program; assemble with --linedebug for source stepping.\n");
        }
        Ok(())
    }

    /// Start the emulator with the program's directory as `SYS:` and the
    /// remote debugger waiting for the program to start.
    fn start_emulator(
        &mut self,
        emulator: &str,
        program: &Path,
        port: u16,
        extra: &[String],
    ) -> Result<(), String> {
        let dir = program
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = program
            .file_name()
            .ok_or("launch: 'program' has no file name")?
            .to_string_lossy();
        // The stub breaks when a CLI command named exactly like the trigger
        // runs, so the startup-sequence must spell it `SYS:name`. Only
        // written when missing: a hand-made one is left alone.
        let trigger = format!("SYS:{name}");
        let s_dir = dir.join("s");
        let has_startup = std::fs::read_dir(&s_dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().eq_ignore_ascii_case("startup-sequence"))
        });
        if !has_startup {
            let startup = s_dir.join("Startup-Sequence");
            std::fs::create_dir_all(&s_dir).map_err(|e| e.to_string())?;
            std::fs::write(&startup, format!("{trigger}\n")).map_err(|e| e.to_string())?;
            self.output(&format!(
                "Created {} to start {trigger}.\n",
                startup.display()
            ));
        }
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let winuae = Path::new(emulator)
            .file_name()
            .is_some_and(|f| f.to_string_lossy().to_lowercase().contains("winuae"));
        let args: Vec<String> = if winuae {
            vec![
                "-s".into(),
                format!("filesystem=rw,dh0:${}", dir.display()),
                "-s".into(),
                "debugging_features=gdbserver".into(),
                "-s".into(),
                format!("debugging_trigger={trigger}"),
            ]
        } else {
            vec![
                format!("--hard_drive_0={}", dir.display()),
                "--remote_debugger=60".into(),
                format!("--remote_debugger_port={port}"),
                format!("--remote_debugger_trigger={trigger}"),
            ]
        };
        let emulator_dir = Path::new(emulator)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Emulator builds ship their shared libraries next to the binary.
        let lib_path = match std::env::var_os("LD_LIBRARY_PATH") {
            Some(old) => {
                let mut p = emulator_dir.as_os_str().to_owned();
                p.push(":");
                p.push(old);
                p
            }
            None => emulator_dir.as_os_str().to_owned(),
        };
        let child = Command::new(emulator)
            .env("LD_LIBRARY_PATH", lib_path)
            .args(extra)
            .args(&args)
            .current_dir(emulator_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start emulator '{emulator}': {e}"))?;
        self.emulator = Some(child);
        Ok(())
    }

    fn connect(&mut self, port: u16) -> Result<(), String> {
        let started = Instant::now();
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(e) => {
                    if let Some(status) = self
                        .emulator
                        .as_mut()
                        .and_then(|c| c.try_wait().ok().flatten())
                    {
                        return Err(format!(
                            "emulator exited ({status}) before the debugger connected"
                        ));
                    }
                    if started.elapsed() > CONNECT_TIMEOUT {
                        return Err(format!("no remote debugger on port {port}: {e}"));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        };
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = rsp::read_packets(reader, |p| {
                let _ = tx.send(Msg::Gdb(p));
            });
            let _ = tx.send(Msg::GdbClosed);
        });
        self.gdb = Some(stream);
        // The stub answers only once the program has started, which takes
        // an emulator boot.
        self.send_gdb("QStartNoAckMode")?;
        self.wait_reply(false, CONNECT_TIMEOUT)?;
        // Ack the OK; from here on neither side acks.
        self.gdb
            .as_mut()
            .unwrap()
            .write_all(b"+")
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ----- GDB requests ---------------------------------------------------

    fn send_gdb(&mut self, payload: &str) -> Result<(), String> {
        let gdb = self.gdb.as_mut().ok_or("not connected to the emulator")?;
        rsp::send(gdb, payload).map_err(|e| format!("emulator connection lost: {e}"))
    }

    /// Send a request and wait for its reply. Stop notifications and editor
    /// requests arriving meanwhile are queued for the main loop.
    fn request(&mut self, payload: &str) -> Result<String, String> {
        self.send_gdb(payload)?;
        self.wait_reply(false, REPLY_TIMEOUT)
    }

    /// Like [`Self::request`], but the reply is a stop packet (`S`/`T`).
    fn request_stop(&mut self, payload: &str) -> Result<String, String> {
        self.send_gdb(payload)?;
        self.wait_reply(true, REPLY_TIMEOUT)
    }

    fn wait_reply(&mut self, want_stop: bool, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            let packet = match self.rx.recv_timeout(timeout) {
                Ok(Msg::Gdb(p)) => p,
                Ok(Msg::GdbClosed) => {
                    self.deferred.push_back(Msg::GdbClosed);
                    return Err("emulator connection closed".into());
                }
                // Pausing is how the editor gets out of a step over a call
                // that never returns, so it cannot wait in the queue.
                Ok(Msg::Dap(req)) if req["command"] == "pause" && want_stop => {
                    self.pausing = true;
                    self.send_gdb(&format!("vCont;t:{CPU}"))?;
                    self.respond(&req, true, Value::Null, None);
                    continue;
                }
                Ok(other) => {
                    self.deferred.push_back(other);
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => return Err("emulator did not reply".into()),
                Err(RecvTimeoutError::Disconnected) => return Err("session closed".into()),
            };
            if is_output(&packet) {
                self.output(&hex_to_text(&packet[1..]));
            } else if is_stop(&packet) != want_stop {
                self.deferred.push_back(Msg::Gdb(packet));
            } else if packet.len() == 3 && packet.starts_with('E') {
                return Err(format!("emulator error {packet}"));
            } else {
                return Ok(packet);
            }
        }
    }

    fn registers(&mut self) -> Result<Vec<u32>, String> {
        let hex = self.request("g")?;
        Ok(hex
            .as_bytes()
            .chunks(8)
            .filter_map(|c| u32::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
            .collect())
    }

    fn pc(&mut self) -> Result<u32, String> {
        self.registers()?
            .get(17)
            .copied()
            .ok_or_else(|| "short register reply".into())
    }

    fn read_bytes(&mut self, addr: u32, len: u32) -> Result<Vec<u8>, String> {
        let hex = self.request(&format!("m{addr:x},{len:x}"))?;
        Ok((0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect())
    }

    // ----- execution control ----------------------------------------------

    fn resume(&mut self) -> Result<(), String> {
        self.send_gdb(&format!("vCont;c:{CPU}"))
    }

    fn stopped(&mut self, reason: &str) {
        self.event(
            "stopped",
            json!({ "reason": reason, "threadId": CPU, "allThreadsStopped": true }),
        );
    }

    /// A stop that arrives while the program runs: breakpoint, exception,
    /// pause, or the program ending.
    fn on_async_packet(&mut self, packet: &str) {
        if is_output(packet) {
            self.output(&hex_to_text(&packet[1..]));
        } else if packet.starts_with('W') || packet.starts_with('X') {
            self.event("exited", json!({ "exitCode": 0 }));
            self.event("terminated", json!({}));
        } else if is_stop(packet) {
            let reason = self.stop_reason(packet);
            self.pausing = false;
            self.stopped(reason);
        }
    }

    fn stop_reason(&self, packet: &str) -> &'static str {
        if self.pausing {
            "pause"
        } else if packet.contains("swbreak") || packet.starts_with("T05") {
            "breakpoint"
        } else {
            // The UAE stub reports unmasked CPU exceptions as a bare S05.
            "exception"
        }
    }

    /// Decode the instruction at `pc`: its mnemonic and length.
    fn instruction_at(&mut self, pc: u32) -> Result<(String, u32), String> {
        let bytes = self.read_bytes(pc, 22)?;
        let mut stream = InstructionStream::new(&bytes, pc);
        Ok(match decode_next(&mut stream, "68060") {
            Ok((_, DecodeResult::Instruction(i))) => {
                (i.mnemonic.to_lowercase(), i.raw_bytes.len() as u32)
            }
            _ => (String::new(), 2),
        })
    }

    /// Execute one instruction, running over subroutine calls when `over`.
    /// Returns the stop packet, and whether something other than the step
    /// itself stopped the CPU (a breakpoint or exception inside the call).
    fn step_instruction(&mut self, over: bool) -> Result<(String, bool), String> {
        let pc = self.pc()?;
        let (mnemonic, len) = self.instruction_at(pc)?;
        if over && (mnemonic.starts_with("jsr") || mnemonic.starts_with("bsr")) {
            let ret = pc + len;
            self.request(&format!("Z0,{ret:x}"))?;
            self.resume()?;
            // A call may run for as long as it likes; `pause` still gets through.
            let packet = self.wait_reply(true, CALL_TIMEOUT);
            self.request(&format!("z0,{ret:x}"))?;
            let packet = packet?;
            let interrupted = self.pc()? != ret;
            return Ok((packet, interrupted));
        }
        let packet = self.request_stop(&format!("vCont;s:{CPU}"))?;
        Ok((packet, false))
    }

    fn step(&mut self, kind: Step, by_instruction: bool) -> Result<(), String> {
        let start = self.pc()?;
        let start_line = self
            .program
            .line_at(start)
            .map(|l| (l.file.clone(), l.line));
        let mut reason = "step";
        for _ in 0..MAX_STEPS {
            let returning = kind == Step::Out && {
                let pc = self.pc()?;
                matches!(self.instruction_at(pc)?.0.as_str(), "rts" | "rte" | "rtr")
            };
            let (packet, interrupted) = self.step_instruction(kind != Step::Into)?;
            if interrupted || !is_stop(&packet) {
                reason = "breakpoint";
                break;
            }
            if kind == Step::Out {
                if returning {
                    break;
                }
                continue;
            }
            if by_instruction {
                break;
            }
            let pc = self.pc()?;
            let line = self.program.line_at(pc).map(|l| (l.file.clone(), l.line));
            // Left the line, or left the program (into ROM): stop either way.
            if line.is_none() || line != start_line {
                break;
            }
        }
        self.stopped(reason);
        Ok(())
    }

    // ----- inspection -----------------------------------------------------

    fn set_breakpoints(&mut self, args: &Value) -> Result<Value, String> {
        let path = args["source"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        for addr in self.breakpoints.remove(&path).unwrap_or_default() {
            self.request(&format!("z0,{addr:x}")).ok();
        }
        let mut result = Vec::new();
        let mut addrs = Vec::new();
        for bp in args["breakpoints"].as_array().into_iter().flatten() {
            let line = bp["line"].as_u64().unwrap_or(0) as u32;
            match self.program.address_of(&path, line) {
                Some((addr, actual)) if self.request(&format!("Z0,{addr:x}")).is_ok() => {
                    addrs.push(addr);
                    result.push(json!({ "verified": true, "line": actual }));
                }
                _ => result.push(json!({
                    "verified": false,
                    "line": line,
                    "message": "no code at or after this line",
                })),
            }
        }
        self.breakpoints.insert(path, addrs);
        Ok(json!({ "breakpoints": result }))
    }

    fn stack_trace(&mut self) -> Result<Value, String> {
        let pc = self.pc()?;
        let name = self
            .program
            .symbolize(pc)
            .unwrap_or_else(|| format!("${pc:08x}"));
        let mut frame = json!({
            "id": 0,
            "name": name,
            "line": 0,
            "column": 0,
            "instructionPointerReference": format!("0x{pc:x}"),
        });
        if let Some(line) = self.program.line_at(pc) {
            let file = Path::new(&line.file);
            frame["source"] = json!({
                "name": file.file_name().map(|n| n.to_string_lossy()),
                "path": line.file,
            });
            frame["line"] = json!(line.line);
            frame["column"] = json!(1);
        }
        Ok(json!({ "stackFrames": [frame], "totalFrames": 1 }))
    }

    fn variables(&mut self) -> Result<Value, String> {
        let regs = self.registers()?;
        let reg = |i: usize| regs.get(i).copied().unwrap_or(0);
        let mut vars: Vec<Value> = (0..8)
            .map(|i| (format!("d{i}"), reg(i)))
            .chain((0..8).map(|i| (format!("a{i}"), reg(8 + i))))
            .chain([("pc".to_string(), reg(17))])
            .map(|(name, v)| json!({ "name": name, "value": format!("${v:08x}"), "variablesReference": 0 }))
            .collect();
        let sr = reg(16);
        let flags: String = [(4, 'X'), (3, 'N'), (2, 'Z'), (1, 'V'), (0, 'C')]
            .iter()
            .map(|&(bit, c)| if sr & (1 << bit) != 0 { c } else { '-' })
            .collect();
        vars.push(json!({ "name": "sr", "value": format!("${sr:04x}  {flags}"), "variablesReference": 0 }));
        Ok(json!({ "variables": vars }))
    }

    fn read_memory(&mut self, args: &Value) -> Result<Value, String> {
        let base = parse_address(args["memoryReference"].as_str().unwrap_or_default())
            .ok_or("invalid memory reference")?;
        let addr = base.wrapping_add(args["offset"].as_i64().unwrap_or(0) as u32);
        let count = args["count"].as_u64().unwrap_or(0).min(64 * 1024) as u32;
        let mut data = Vec::new();
        while (data.len() as u32) < count {
            let chunk = (count - data.len() as u32).min(512);
            match self.read_bytes(addr + data.len() as u32, chunk) {
                Ok(b) if !b.is_empty() => data.extend(b),
                _ => break,
            }
        }
        Ok(json!({
            "address": format!("0x{addr:x}"),
            "data": base64(&data),
            "unreadableBytes": count as usize - data.len(),
        }))
    }

    /// Registers and labels, for hovers and the watch view.
    fn evaluate(&mut self, args: &Value) -> Result<Value, String> {
        let expr = args["expression"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let index = match expr.as_str() {
            "sp" => Some(15),
            "sr" => Some(16),
            "pc" => Some(17),
            e if e.len() == 2 && e.as_bytes()[1].is_ascii_digit() && e.as_bytes()[1] < b'8' => {
                let n = (e.as_bytes()[1] - b'0') as usize;
                match e.as_bytes()[0] {
                    b'd' => Some(n),
                    b'a' => Some(8 + n),
                    _ => None,
                }
            }
            _ => None,
        };
        let value = match index {
            Some(i) => self.registers()?.get(i).copied(),
            None => self
                .program
                .symbols
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&expr))
                .map(|(_, addr)| *addr),
        }
        .ok_or_else(|| format!("unknown: {expr}"))?;
        Ok(json!({
            "result": format!("${value:08x} ({value})"),
            "variablesReference": 0,
            "memoryReference": format!("0x{value:x}"),
        }))
    }

    // ----- DAP output -----------------------------------------------------

    fn write(&mut self, mut msg: Value) {
        msg["seq"] = json!(self.seq);
        self.seq += 1;
        let body = msg.to_string();
        let _ = write!(self.out, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = self.out.flush();
    }

    fn respond(&mut self, req: &Value, success: bool, body: Value, message: Option<String>) {
        let mut msg = json!({
            "type": "response",
            "request_seq": req["seq"],
            "command": req["command"],
            "success": success,
        });
        if !body.is_null() {
            msg["body"] = body;
        }
        if let Some(m) = message {
            msg["message"] = json!(m);
        }
        self.write(msg);
    }

    fn event(&mut self, event: &str, body: Value) {
        self.write(json!({ "type": "event", "event": event, "body": body }));
    }

    fn output(&mut self, text: &str) {
        self.event("output", json!({ "category": "console", "output": text }));
    }

    fn shutdown(&mut self) {
        if let Some(mut child) = self.emulator.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn is_stop(packet: &str) -> bool {
    packet.starts_with('S') || packet.starts_with('T')
}

/// `O` + hex text is console output; a bare `OK` is a reply.
fn is_output(packet: &str) -> bool {
    packet.len() > 1
        && packet != "OK"
        && packet.starts_with('O')
        && packet[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn hex_to_text(hex: &str) -> String {
    (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .map(char::from)
        .collect()
}

fn parse_address(s: &str) -> Option<u32> {
    let s = s.trim();
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix('$'));
    match hex {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i)) as usize & 63] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn output_is_not_ok() {
        assert!(is_output("O48690a"));
        assert!(!is_output("OK"));
        assert_eq!(hex_to_text("48690a"), "Hi\n");
    }
}
