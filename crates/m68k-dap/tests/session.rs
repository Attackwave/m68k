//! Drives a full debug session against a fake UAE GDB stub over TCP.

use std::io::{self, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use m68k_asm::amiga_hunk_writer::generate_relocatable_hunk_exe;
use m68k_asm::assembler::Assembler;
use m68k_dap::{Msg, Session, rsp};
use serde_json::{Value, json};

const LOAD: u32 = 0x2_0000;
/// A copper list in the fake machine's chip RAM, pointed at by COP1LC.
const COPPER: u32 = 0x3000;
const COPPER_LIST: [u16; 6] = [0x0180, 0x0F80, 0x2C07, 0xFFFE, 0xFFFF, 0xFFFE];

/// The fake machine's memory: the program, a copper list and the custom
/// chip block the emulator reports at $DFF000.
fn peek(code: &[u8], addr: u32) -> u8 {
    let word = |words: &[u16], off: u32| {
        words
            .get((off / 2) as usize)
            .map(|w| w.to_be_bytes()[(off % 2) as usize])
    };
    if let Some(off) = addr
        .checked_sub(LOAD)
        .filter(|&o| (o as usize) < code.len())
    {
        return code[off as usize];
    }
    if let Some(b) = addr
        .checked_sub(COPPER)
        .and_then(|off| word(&COPPER_LIST, off))
    {
        return b;
    }
    let mut custom = [0u16; 0xFF];
    custom[0x080 / 2] = (COPPER >> 16) as u16; // COP1LCH
    custom[0x082 / 2] = COPPER as u16; // COP1LCL
    custom[0x100 / 2] = 0x4200; // BPLCON0
    custom[0x096 / 2] = 0x0380; // DMACON state
    addr.checked_sub(0xDFF000)
        .filter(|&o| o < 0x1FE)
        .and_then(|o| word(&custom, o))
        .unwrap_or(0)
}
const SOURCE: &str = "start:\n moveq #1,d0\n\n moveq #2,d1\n moveq #3,d2\n rts\n";

/// DAP output: the adapter flushes after every message.
struct Out(Sender<Value>, Vec<u8>);

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.1.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let text = String::from_utf8(std::mem::take(&mut self.1)).unwrap();
        let body = text.split("\r\n\r\n").nth(1).unwrap();
        self.0.send(serde_json::from_str(body).unwrap()).unwrap();
        Ok(())
    }
}

/// A stub that runs straight-line 2-byte instructions: enough to exercise
/// breakpoints, stepping and register reads.
fn fake_stub(listener: TcpListener, code: Vec<u8>) {
    let (mut sock, _) = listener.accept().unwrap();
    let reader = sock.try_clone().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || rsp::read_packets(reader, |p| tx.send(p).unwrap()));
    let mut pc = LOAD;
    let mut breakpoints: Vec<u32> = Vec::new();
    let reply = |sock: &mut std::net::TcpStream, p: &str| rsp::send(sock, p).unwrap();
    for packet in rx {
        match packet.as_str() {
            "QStartNoAckMode" | "Z1,0,0;X2,3c" => reply(&mut sock, "OK"),
            "qOffsets" => reply(&mut sock, &format!("{LOAD:x}")),
            "g" => {
                let mut regs = [0u32; 18];
                regs[0] = (pc - LOAD) / 2; // d0 counts executed instructions
                regs[15] = 0x3_0000; // stack: zeroes, no return addresses
                regs[17] = pc;
                reply(
                    &mut sock,
                    &regs.iter().map(|r| format!("{r:08x}")).collect::<String>(),
                );
            }
            p if p.starts_with('m') => {
                let (addr, len) = p[1..].split_once(',').unwrap();
                let addr = u32::from_str_radix(addr, 16).unwrap();
                let len = u32::from_str_radix(len, 16).unwrap();
                let bytes: String = (addr..addr + len)
                    .map(|a| format!("{:02x}", peek(&code, a)))
                    .collect();
                reply(&mut sock, &bytes);
            }
            p if p.starts_with("Z0,") => {
                breakpoints.push(u32::from_str_radix(&p[3..], 16).unwrap());
                reply(&mut sock, "OK");
            }
            p if p.starts_with("z0,") => {
                let addr = u32::from_str_radix(&p[3..], 16).unwrap();
                breakpoints.retain(|&b| b != addr);
                reply(&mut sock, "OK");
            }
            "vCont;s:1" => {
                pc += 2;
                reply(&mut sock, "T05thread:1;");
            }
            "vCont;c:1" => {
                pc = breakpoints
                    .iter()
                    .copied()
                    .filter(|&b| b > pc)
                    .min()
                    .unwrap();
                reply(&mut sock, "T05swbreak:;thread:1;");
            }
            other => panic!("unexpected packet {other}"),
        }
    }
}

struct Client {
    tx: Sender<Msg>,
    rx: Receiver<Value>,
    seq: i64,
}

impl Client {
    fn request(&mut self, command: &str, arguments: Value) -> Value {
        self.seq += 1;
        let seq = self.seq;
        self.tx
            .send(Msg::Dap(json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments })))
            .unwrap();
        loop {
            let msg = self.next();
            if msg["type"] == "response" && msg["request_seq"] == seq {
                assert_eq!(msg["success"], true, "{command} failed: {msg}");
                return msg["body"].clone();
            }
        }
    }

    fn next(&self) -> Value {
        self.rx
            .recv_timeout(Duration::from_secs(5))
            .expect("no message from adapter")
    }

    fn expect_event(&self, event: &str) -> Value {
        loop {
            let msg = self.next();
            if msg["type"] == "event" && msg["event"] == event {
                return msg["body"].clone();
            }
        }
    }

    fn current_line(&mut self) -> Value {
        self.request("stackTrace", json!({ "threadId": 1 }))["stackFrames"][0]["line"].clone()
    }
}

#[test]
fn breakpoint_continue_step_and_registers() {
    let dir = std::env::temp_dir().join(format!("m68k-dap-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("main.s").to_string_lossy().into_owned();
    let assemble = |origin: u32, overrides: &[(m68k_asm::directives::SectionKind, u32)]| {
        let mut asm = Assembler::new(origin);
        for (kind, at) in overrides {
            asm.sections.set_origin_override(kind.clone(), *at);
        }
        asm.assemble(SOURCE)?;
        Ok(asm)
    };
    let base = assemble(0, &[]).unwrap();
    let line_of = |n: usize| Some((src_path.clone(), n as u32));
    let exe = generate_relocatable_hunk_exe(&base, Some(&line_of), assemble).unwrap();
    let exe_path = dir.join("prog");
    std::fs::write(&exe_path, &exe).unwrap();
    let code = m68k_core::amiga_hunk::read_hunk_executable(&exe, 0)
        .unwrap()
        .sections[0]
        .data
        .clone();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || fake_stub(listener, code));

    let (tx, rx) = channel();
    let (out_tx, out_rx) = channel();
    let session_tx = tx.clone();
    std::thread::spawn(move || Session::new(Out(out_tx, Vec::new()), session_tx, rx).run());
    let mut c = Client {
        tx,
        rx: out_rx,
        seq: 0,
    };

    c.request("initialize", json!({}));
    c.request("launch", json!({ "program": exe_path, "serverPort": port }));

    // Line 3 is empty: the breakpoint moves to line 4.
    let bps = c.request(
        "setBreakpoints",
        json!({ "source": { "path": src_path }, "breakpoints": [{ "line": 3 }] }),
    );
    assert_eq!(
        bps["breakpoints"][0],
        json!({ "verified": true, "line": 4 })
    );

    c.request("configurationDone", json!({}));
    assert_eq!(c.expect_event("stopped")["reason"], "breakpoint");
    assert_eq!(c.current_line(), 4);
    let frame = &c.request("stackTrace", json!({ "threadId": 1 }))["stackFrames"][0];
    assert_eq!(frame["source"]["path"], src_path);
    assert_eq!(frame["name"], "start+2");

    c.request("next", json!({ "threadId": 1 }));
    assert_eq!(c.expect_event("stopped")["reason"], "step");
    assert_eq!(c.current_line(), 5);

    let vars = c.request("variables", json!({ "variablesReference": 1 }));
    assert_eq!(
        vars["variables"][0],
        json!({ "name": "d0", "value": "$00000002", "variablesReference": 0 })
    );

    // One frame: the zeroed stack holds no return addresses.
    let trace = c.request("stackTrace", json!({ "threadId": 1 }));
    assert_eq!(trace["totalFrames"], 1);

    // Backwards from the current instruction, resynced on the line table.
    let dis = c.request(
        "disassemble",
        json!({ "memoryReference": format!("0x{:x}", LOAD + 4), "instructionOffset": -2, "instructionCount": 4 }),
    );
    let insns = dis["instructions"].as_array().unwrap();
    let addrs: Vec<&str> = insns
        .iter()
        .map(|i| i["address"].as_str().unwrap())
        .collect();
    assert_eq!(addrs, ["0x20000", "0x20002", "0x20004", "0x20006"]);
    assert_eq!(insns[0]["symbol"], "start");
    assert_eq!(insns[1]["line"], 4);
    assert!(
        insns[3]["instruction"].as_str().unwrap().starts_with("rts"),
        "{}",
        insns[3]
    );

    // A breakpoint set from the disassembly view stops `continue` there.
    let bps = c.request(
        "setInstructionBreakpoints",
        json!({ "breakpoints": [{ "instructionReference": format!("0x{:x}", LOAD + 6) }] }),
    );
    assert_eq!(bps["breakpoints"][0]["verified"], true);
    c.request("continue", json!({ "threadId": 1 }));
    assert_eq!(c.expect_event("stopped")["reason"], "breakpoint");
    assert_eq!(c.current_line(), 6);

    // Custom chip state, including write-only registers, and the copper
    // list COP1LC points at.
    let scopes = c.request("scopes", json!({ "frameId": 0 }));
    let names: Vec<&str> = scopes["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Registers",
            "Custom chips",
            "Copper list (COP1LC)",
            "Copper list 2 (COP2LC)"
        ]
    );
    let custom = c.request("variables", json!({ "variablesReference": 2 }));
    let bplcon0 = custom["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "BPLCON0")
        .unwrap();
    assert_eq!(bplcon0["value"], "$4200  BPU=4|COLOR");
    let copper = c.request("variables", json!({ "variablesReference": 3 }));
    let lines: Vec<(&str, &str)> = copper["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["name"].as_str().unwrap(), v["value"].as_str().unwrap()))
        .collect();
    assert_eq!(
        lines,
        [
            ("$003000", "MOVE COLOR00 = $0F80  R=15 G=8 B=0"),
            ("$003004", "WAIT v=$2C h=$06"),
            ("$003008", "END"),
        ]
    );
    let copper2 = c.request("variables", json!({ "variablesReference": 4 }));
    assert_eq!(copper2["variables"][0]["value"], "not set");

    let hover = c.request("evaluate", json!({ "expression": "start" }));
    assert_eq!(hover["result"], format!("${LOAD:08x} ({LOAD})"));

    c.request("disconnect", json!({}));
    std::fs::remove_dir_all(&dir).ok();
}
