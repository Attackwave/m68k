//! `m68k-dap`: Debug Adapter Protocol server on stdio.

use std::sync::mpsc::channel;

fn main() {
    let (tx, rx) = channel();
    let input_tx = tx.clone();
    std::thread::spawn(move || m68k_dap::read_dap(std::io::stdin(), input_tx));
    m68k_dap::Session::new(std::io::stdout(), tx, rx).run();
}
