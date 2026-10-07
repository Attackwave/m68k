//! Interactive Hex Memory & Binary Data Inspector Component.

import { api } from '../api';

export function createHexMemoryViewer(): {
  element: HTMLElement;
  updateData: (bytes: number[], origin?: number) => void;
} {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-hidden text-xs font-mono select-none';

  container.innerHTML = `
    <!-- Hex Header Toolbar -->
    <div class="h-10 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-4 shrink-0">
      <div class="flex items-center space-x-3">
        <span class="font-bold text-white font-sans text-xs uppercase tracking-wider">🔍 Live Hex & Memory Inspector</span>
        <div class="flex items-center space-x-1">
          <span class="text-studio-muted text-[11px]">Jump to:</span>
          <input type="text" id="hex-jump-input" placeholder="$0000" class="w-20 bg-studio-panel border border-studio-border rounded px-2 py-0.5 text-white outline-none text-xs">
          <button id="btn-hex-jump" class="px-2 py-0.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-[11px] font-medium font-sans cursor-pointer transition">
            Go
          </button>
        </div>
      </div>

      <div class="text-[11px] text-studio-muted font-sans" id="hex-size-info">
        0 bytes loaded
      </div>
    </div>

    <!-- Main Grid: Hex Dump Table + Right Inspector Box -->
    <div class="flex-1 flex overflow-hidden">
      <!-- 16-byte Hex Dump Grid -->
      <div class="flex-1 overflow-y-auto p-3 select-text" id="hex-table-container">
        <div class="text-center py-20 text-studio-muted font-sans">
          Assemble code or load binary to inspect memory bytes
        </div>
      </div>

      <!-- Right Data Inspector Box -->
      <div class="w-72 bg-studio-sidebar/60 border-l border-studio-border p-4 flex flex-col space-y-3 font-sans shrink-0">
        <div class="font-bold text-white uppercase text-[11px] tracking-wider border-b border-studio-border pb-1">
          Data Type Inspector
        </div>

        <div class="space-y-2 font-mono text-xs" id="inspector-values">
          <div class="p-2 bg-studio-panel border border-studio-border rounded space-y-1">
            <div class="text-[10px] text-studio-muted">Selected Address:</div>
            <div class="text-blue-400 font-bold" id="insp-addr">$00000000</div>
          </div>

          <div class="p-2 bg-studio-panel border border-studio-border rounded space-y-1.5">
            <div class="flex justify-between">
              <span class="text-studio-muted">Byte (u8/i8):</span>
              <span class="text-white font-bold" id="insp-u8">0 / $00</span>
            </div>
            <div class="flex justify-between">
              <span class="text-studio-muted">Word (u16/i16):</span>
              <span class="text-white font-bold" id="insp-u16">0 / $0000</span>
            </div>
            <div class="flex justify-between">
              <span class="text-studio-muted">Long (u32/i32):</span>
              <span class="text-white font-bold" id="insp-u32">0 / $00000000</span>
            </div>
            <div class="flex justify-between">
              <span class="text-studio-muted">Binary:</span>
              <span class="text-emerald-400 font-bold text-[11px]" id="insp-bin">%00000000</span>
            </div>
          </div>

          <div class="p-2 bg-studio-panel border border-studio-border rounded space-y-1">
            <div class="text-[10px] text-studio-muted">Disassembly at offset:</div>
            <div class="text-amber-400 font-bold text-xs truncate" id="insp-disasm">-</div>
          </div>
        </div>
      </div>
    </div>
  `;

  let currentBytes: number[] = [];
  let baseOrigin = 0;
  let selectedOffset = 0;

  const tableContainer = container.querySelector('#hex-table-container') as HTMLElement;
  const sizeInfo = container.querySelector('#hex-size-info') as HTMLElement;
  const jumpInput = container.querySelector('#hex-jump-input') as HTMLInputElement;

  const inspAddr = container.querySelector('#insp-addr') as HTMLElement;
  const inspU8 = container.querySelector('#insp-u8') as HTMLElement;
  const inspU16 = container.querySelector('#insp-u16') as HTMLElement;
  const inspU32 = container.querySelector('#insp-u32') as HTMLElement;
  const inspBin = container.querySelector('#insp-bin') as HTMLElement;
  const inspDisasm = container.querySelector('#insp-disasm') as HTMLElement;

  function updateData(bytes: number[], origin: number = 0) {
    currentBytes = bytes;
    baseOrigin = origin;
    sizeInfo.textContent = `${bytes.length} bytes (Origin: $${origin.toString(16).toUpperCase()})`;
    renderHex();
    updateInspector(selectedOffset);
  }

  function renderHex() {
    if (currentBytes.length === 0) {
      tableContainer.innerHTML = '<div class="text-center py-20 text-studio-muted font-sans">No binary data to display</div>';
      return;
    }

    let html = `
      <table class="w-full text-left font-mono text-xs border-collapse">
        <thead class="text-studio-muted border-b border-studio-border select-none">
          <tr>
            <th class="py-1 px-2 w-24">Address</th>
            <th class="py-1 px-2">00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F</th>
            <th class="py-1 px-2 w-40">ASCII</th>
          </tr>
        </thead>
        <tbody>
    `;

    for (let i = 0; i < currentBytes.length; i += 16) {
      const addrHex = `$${(baseOrigin + i).toString(16).toUpperCase().padStart(8, '0')}`;
      let hexCols = '';
      let asciiCols = '';

      for (let j = 0; j < 16; j++) {
        const byteIdx = i + j;
        if (j === 8) hexCols += ' ';
        if (byteIdx < currentBytes.length) {
          const b = currentBytes[byteIdx];
          const isSelected = byteIdx === selectedOffset;
          hexCols += `<span class="hex-byte px-1 rounded cursor-pointer ${
            isSelected ? 'bg-blue-600 text-white font-bold' : 'hover:bg-studio-hover text-studio-text'
          }" data-idx="${byteIdx}">${b.toString(16).toUpperCase().padStart(2, '0')}</span> `;
          asciiCols += b >= 32 && b <= 126 ? String.fromCharCode(b) : '.';
        } else {
          hexCols += '   ';
          asciiCols += ' ';
        }
      }

      html += `
        <tr class="hover:bg-studio-panel/40 border-b border-studio-border/20">
          <td class="py-1 px-2 text-blue-400 font-bold select-none">${addrHex}</td>
          <td class="py-1 px-2 font-mono whitespace-pre">${hexCols}</td>
          <td class="py-1 px-2 text-studio-muted font-mono whitespace-pre">${asciiCols}</td>
        </tr>
      `;
    }

    html += '</tbody></table>';
    tableContainer.innerHTML = html;

    tableContainer.querySelectorAll('.hex-byte').forEach((el) => {
      el.addEventListener('click', (e) => {
        const idx = parseInt((e.currentTarget as HTMLElement).getAttribute('data-idx') || '0', 10);
        selectedOffset = idx;
        renderHex();
        updateInspector(idx);
      });
    });
  }

  async function updateInspector(offset: number) {
    if (offset >= currentBytes.length) return;

    const addr = baseOrigin + offset;
    inspAddr.textContent = `$${addr.toString(16).toUpperCase().padStart(8, '0')}`;

    const u8 = currentBytes[offset];
    inspU8.textContent = `${u8} / $${u8.toString(16).toUpperCase().padStart(2, '0')}`;
    inspBin.textContent = `%${u8.toString(2).padStart(8, '0')}`;

    if (offset + 1 < currentBytes.length) {
      const u16 = (currentBytes[offset] << 8) | currentBytes[offset + 1];
      inspU16.textContent = `${u16} / $${u16.toString(16).toUpperCase().padStart(4, '0')}`;
    } else {
      inspU16.textContent = '-';
    }

    if (offset + 3 < currentBytes.length) {
      const u32 =
        ((currentBytes[offset] << 24) |
          (currentBytes[offset + 1] << 16) |
          (currentBytes[offset + 2] << 8) |
          currentBytes[offset + 3]) >>>
        0;
      inspU32.textContent = `${u32} / $${u32.toString(16).toUpperCase().padStart(8, '0')}`;
    } else {
      inspU32.textContent = '-';
    }

    // Quick disassembly for 1 instruction
    const slice = currentBytes.slice(offset, Math.min(offset + 10, currentBytes.length));
    const disRes = await api.disassemble(slice, addr, '68000');
    if (disRes.lines.length > 0) {
      inspDisasm.textContent = disRes.lines[0].text;
    } else {
      inspDisasm.textContent = '-';
    }
  }

  container.querySelector('#btn-hex-jump')?.addEventListener('click', () => {
    let val = jumpInput.value.trim();
    if (val.startsWith('$')) val = val.substring(1);
    const targetAddr = parseInt(val, 16);
    if (!isNaN(targetAddr) && targetAddr >= baseOrigin) {
      const offset = targetAddr - baseOrigin;
      if (offset < currentBytes.length) {
        selectedOffset = offset;
        renderHex();
        updateInspector(offset);
      }
    }
  });

  return { element: container, updateData };
}
