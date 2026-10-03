//! Disassembly Inspector Panel.

import { DisassembleLineItem } from '../api';

export function createDisassemblerPanel(): { element: HTMLElement; updateLines: (lines: DisassembleLineItem[]) => void } {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-hidden font-mono text-xs';

  container.innerHTML = `
    <!-- Top toolbar -->
    <div class="h-9 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-3 shrink-0">
      <div class="flex items-center space-x-2">
        <span class="font-bold text-white uppercase text-[11px] tracking-wider">Disassembly & Trace</span>
        <span id="disasm-count" class="text-studio-muted text-xs">(0 instructions)</span>
      </div>
      <div class="flex items-center space-x-2">
        <span class="text-studio-muted text-xs">Origin:</span>
        <input id="disasm-origin-input" type="text" value="$000000" class="w-20 bg-studio-panel border border-studio-border text-white text-xs px-2 py-0.5 rounded outline-none text-center">
      </div>
    </div>

    <!-- Table Header -->
    <div class="grid grid-cols-12 gap-2 px-3 py-1.5 bg-studio-panel text-studio-muted font-bold text-[10px] uppercase border-b border-studio-border shrink-0">
      <div class="col-span-2">Address</div>
      <div class="col-span-3">Machine Code (Hex)</div>
      <div class="col-span-4">Instruction</div>
      <div class="col-span-3">Annotation / LVO</div>
    </div>

    <!-- Table Body -->
    <div id="disasm-rows" class="flex-1 overflow-y-auto divide-y divide-studio-border/30">
      <div class="text-center py-12 text-studio-muted">
        Assemble code to view live disassembly trace
      </div>
    </div>
  `;

  const rowsContainer = container.querySelector('#disasm-rows') as HTMLElement;
  const countEl = container.querySelector('#disasm-count') as HTMLElement;

  function updateLines(lines: DisassembleLineItem[]) {
    if (!lines || lines.length === 0) {
      rowsContainer.innerHTML = '<div class="text-center py-12 text-studio-muted">No instructions disassembled</div>';
      countEl.textContent = '(0 instructions)';
      return;
    }

    countEl.textContent = `(${lines.length} instructions)`;

    rowsContainer.innerHTML = lines
      .map(
        (l) => `
        <div class="grid grid-cols-12 gap-2 px-3 py-1 hover:bg-studio-hover items-center transition">
          <div class="col-span-2 text-blue-400 font-semibold">${l.address_hex}</div>
          <div class="col-span-3 text-studio-muted tracking-widest text-[11px]">${l.instruction_hex}</div>
          <div class="col-span-4 text-white font-medium">${l.text}</div>
          <div class="col-span-3 text-emerald-400 truncate text-[11px]">${l.comment || ''}</div>
        </div>
      `
      )
      .join('');
  }

  return { element: container, updateLines };
}
