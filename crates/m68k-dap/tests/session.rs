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
                regs[17] = pc;
                reply(
                    &mut sock,
                    &regs.iter().map(|r| format!("{r:08x}")).collect::<String>(),
                );
            }
            p if p.starts_with('m') => {
                let (addr, len) = p[1..].split_once(',').unwrap();
                let addr = u32::from_str_radix(addr, 16).unwrap() - LOAD;
                let len = u32::from_str_radix(len, 16).unwrap();
                let bytes: String = (addr..addr + len)
                    .map(|a| format!("{:02x}", code.get(a as usize).copied().unwrap_or(0)))
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

    let hover = c.request("evaluate", json!({ "expression": "start" }));
    assert_eq!(hover["result"], format!("${LOAD:08x} ({LOAD})"));

    c.request("disconnect", json!({}));
    std::fs::remove_dir_all(&dir).ok();
}
