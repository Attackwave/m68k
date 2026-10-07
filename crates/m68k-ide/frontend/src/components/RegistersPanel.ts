//! 68000 Registers Dashboard and Condition Codes Panel.

export function createRegistersPanel(): HTMLElement {
  const panel = document.createElement('div');
  panel.className = 'w-72 bg-studio-sidebar border-l border-studio-border flex flex-col shrink-0 text-xs font-mono select-none';

  panel.innerHTML = `
    <!-- Header -->
    <div class="h-9 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-3 shrink-0">
      <span class="font-bold text-white uppercase text-[11px] tracking-wider">68000 Registers</span>
      <span class="text-studio-muted text-[10px]">Active State</span>
    </div>

    <div class="flex-1 overflow-y-auto p-3 space-y-4">
      <!-- Data Registers (D0-D7) -->
      <div class="space-y-1.5">
        <div class="text-[10px] font-bold text-studio-muted uppercase tracking-wider">Data Registers (32-bit)</div>
        <div class="grid grid-cols-2 gap-1.5">
          ${[0, 1, 2, 3, 4, 5, 6, 7]
            .map(
              (i) => `
            <div class="p-1.5 bg-studio-panel border border-studio-border rounded flex justify-between items-center">
              <span class="text-blue-400 font-bold">D${i}:</span>
              <span class="text-white text-[11px]" id="reg-d${i}">$00000000</span>
            </div>
          `
            )
            .join('')}
        </div>
      </div>

      <!-- Address Registers (A0-A7 / SP) -->
      <div class="space-y-1.5">
        <div class="text-[10px] font-bold text-studio-muted uppercase tracking-wider">Address Registers (32-bit)</div>
        <div class="grid grid-cols-2 gap-1.5">
          ${[0, 1, 2, 3, 4, 5, 6, 7]
            .map(
              (i) => `
            <div class="p-1.5 bg-studio-panel border border-studio-border rounded flex justify-between items-center">
              <span class="text-amber-400 font-bold">${i === 7 ? 'SP' : `A${i}`}:</span>
              <span class="text-white text-[11px]" id="reg-a${i}">$00000000</span>
            </div>
          `
            )
            .join('')}
        </div>
      </div>

      <!-- Program Counter & Status Register -->
      <div class="space-y-1.5">
        <div class="text-[10px] font-bold text-studio-muted uppercase tracking-wider">Control Registers</div>
        <div class="space-y-1">
          <div class="p-1.5 bg-studio-panel border border-studio-border rounded flex justify-between items-center">
            <span class="text-purple-400 font-bold">PC:</span>
            <span class="text-white text-[11px]" id="reg-pc">$00000000</span>
          </div>
          <div class="p-1.5 bg-studio-panel border border-studio-border rounded flex justify-between items-center">
            <span class="text-emerald-400 font-bold">SR:</span>
            <span class="text-white text-[11px]" id="reg-sr">$2700 (Supervisor)</span>
          </div>
        </div>
      </div>

      <!-- Condition Code Register (CCR) Flags -->
      <div class="space-y-1.5">
        <div class="text-[10px] font-bold text-studio-muted uppercase tracking-wider">Condition Codes (CCR)</div>
        <div class="grid grid-cols-5 gap-1 text-center">
          ${['X', 'N', 'Z', 'V', 'C']
            .map(
              (flag) => `
            <button class="ccr-flag p-1 bg-studio-panel hover:bg-studio-hover border border-studio-border rounded font-bold transition text-studio-muted cursor-pointer" data-flag="${flag}">
              <div class="text-[9px] text-studio-muted">${flag}</div>
              <div class="text-xs text-white" id="flag-${flag.toLowerCase()}">0</div>
            </button>
          `
            )
            .join('')}
        </div>
      </div>
    </div>
  `;

  // Toggle flag interactive clicks
  panel.querySelectorAll('.ccr-flag').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      const flag = (e.currentTarget as HTMLElement).getAttribute('data-flag')?.toLowerCase();
      if (flag) {
        const valEl = panel.querySelector(`#flag-${flag}`);
        if (valEl) {
          const current = valEl.textContent === '1' ? '0' : '1';
          valEl.textContent = current;
          valEl.className = current === '1' ? 'text-xs text-emerald-400 font-bold' : 'text-xs text-white';
        }
      }
    });
  });

  return panel;
}
