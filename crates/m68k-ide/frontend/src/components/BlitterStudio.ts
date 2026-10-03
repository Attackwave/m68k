//! Amiga Blitter Studio & Minterm Calculator Component.

export function createBlitterStudio(onInsertCode: (code: string) => void): HTMLElement {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-y-auto p-4 space-y-4 text-xs font-sans select-none';

  container.innerHTML = `
    <!-- Header -->
    <div class="flex items-center justify-between border-b border-studio-border pb-3 shrink-0">
      <div>
        <h2 class="text-sm font-bold text-white tracking-wide">🧮 AMIGA BLITTER STUDIO & MINTERM CALCULATOR</h2>
        <p class="text-studio-muted text-xs">Configure Blitter DMA channels (A, B, C, D), logic minterms, shifts, and masks with live register code generation.</p>
      </div>
      <div class="flex items-center space-x-2">
        <button id="btn-copy-blitter-asm" class="px-3 py-1.5 bg-studio-panel hover:bg-studio-hover border border-studio-border rounded text-white font-medium transition cursor-pointer flex items-center space-x-1">
          <span>📋 Copy Code</span>
        </button>
        <button id="btn-insert-blitter-asm" class="px-3 py-1.5 bg-blue-600 hover:bg-blue-500 rounded text-white font-medium shadow transition cursor-pointer flex items-center space-x-1">
          <span>📝 Insert to Editor</span>
        </button>
      </div>
    </div>

    <!-- Main Grid -->
    <div class="grid grid-cols-12 gap-4">
      <!-- Left Column: Channel & Logic Selection -->
      <div class="col-span-7 space-y-4">
        <!-- 1. Channel DMA Enables -->
        <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-2.5">
          <div class="font-bold text-white uppercase text-[11px] tracking-wider">1. DMA Channels (Source & Destination)</div>
          <div class="grid grid-cols-4 gap-2">
            <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer hover:border-blue-500">
              <input type="checkbox" id="chk-usea" checked class="rounded text-blue-500">
              <div>
                <div class="font-bold text-white">Channel A</div>
                <div class="text-[10px] text-studio-muted">Source / Mask</div>
              </div>
            </label>
            <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer hover:border-blue-500">
              <input type="checkbox" id="chk-useb" checked class="rounded text-blue-500">
              <div>
                <div class="font-bold text-white">Channel B</div>
                <div class="text-[10px] text-studio-muted">Source / Pattern</div>
              </div>
            </label>
            <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer hover:border-blue-500">
              <input type="checkbox" id="chk-usec" checked class="rounded text-blue-500">
              <div>
                <div class="font-bold text-white">Channel C</div>
                <div class="text-[10px] text-studio-muted">Background</div>
              </div>
            </label>
            <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer hover:border-blue-500">
              <input type="checkbox" id="chk-used" checked class="rounded text-blue-500">
              <div>
                <div class="font-bold text-white">Channel D</div>
                <div class="text-[10px] text-studio-muted">Destination</div>
              </div>
            </label>
          </div>
        </div>

        <!-- 2. Logic Presets & Minterm Function -->
        <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-3">
          <div class="flex items-center justify-between">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">2. Logic Operation Presets</div>
            <div class="text-xs font-mono text-emerald-400 font-bold" id="minterm-hex-badge">LF = $CA (Cookie-Cut)</div>
          </div>

          <div class="grid grid-cols-3 gap-2">
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0xCA">
              <div class="font-bold text-white text-xs">🍪 Cookie-Cut ($CA)</div>
              <div class="text-[10px] text-studio-muted">D = (A & B) | (~A & C)</div>
            </button>
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0xF0">
              <div class="font-bold text-white text-xs">📄 Copy A -> D ($F0)</div>
              <div class="text-[10px] text-studio-muted">D = A (Direct copy)</div>
            </button>
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0xCC">
              <div class="font-bold text-white text-xs">🎨 Copy B -> D ($CC)</div>
              <div class="text-[10px] text-studio-muted">D = B (Pattern copy)</div>
            </button>
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0x60">
              <div class="font-bold text-white text-xs">⚡ XOR Blit ($60)</div>
              <div class="text-[10px] text-studio-muted">D = A ^ C (Invert mask)</div>
            </button>
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0x50">
              <div class="font-bold text-white text-xs">🔄 Invert ($50)</div>
              <div class="text-[10px] text-studio-muted">D = ~A</div>
            </button>
            <button class="preset-btn px-2.5 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-left transition cursor-pointer font-mono" data-lf="0xFF">
              <div class="font-bold text-white text-xs">⬛ Solid Fill ($FF)</div>
              <div class="text-[10px] text-studio-muted">D = 1 (Fill ones)</div>
            </button>
          </div>

          <!-- Minterm Truth Table Bits (M7 to M0) -->
          <div class="space-y-1.5 pt-1">
            <div class="text-[11px] font-semibold text-studio-muted">Minterm Bit Pattern (M7 - M0):</div>
            <div class="grid grid-cols-8 gap-1 font-mono text-center" id="minterm-bits-grid">
              ${[7, 6, 5, 4, 3, 2, 1, 0]
                .map(
                  (b) => `
                <div class="p-1.5 bg-studio-bg border border-studio-border rounded cursor-pointer hover:border-blue-500 transition minterm-bit" data-bit="${b}">
                  <div class="text-[9px] text-studio-muted">M${b}</div>
                  <div class="font-bold text-white text-xs bit-val">0</div>
                </div>
              `
                )
                .join('')}
            </div>
          </div>
        </div>

        <!-- 3. Shifts, Masks & Modes -->
        <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-3">
          <div class="font-bold text-white uppercase text-[11px] tracking-wider">3. Shifts, Masks & Mode Flags</div>
          <div class="grid grid-cols-2 gap-4">
            <div class="space-y-1">
              <label class="text-studio-muted font-semibold flex justify-between">
                <span>Channel A Shift (ASH):</span>
                <span id="label-ash" class="font-mono text-white font-bold">0 px</span>
              </label>
              <input type="range" id="range-ash" min="0" max="15" value="0" class="w-full">
            </div>
            <div class="space-y-1">
              <label class="text-studio-muted font-semibold flex justify-between">
                <span>Channel B Shift (BSH):</span>
                <span id="label-bsh" class="font-mono text-white font-bold">0 px</span>
              </label>
              <input type="range" id="range-bsh" min="0" max="15" value="0" class="w-full">
            </div>
          </div>

          <div class="grid grid-cols-2 gap-4">
            <div class="space-y-1">
              <label class="text-studio-muted font-semibold">First Word Mask (BLTAFWM):</label>
              <input type="text" id="input-afwm" value="$FFFF" class="w-full bg-studio-bg border border-studio-border text-white px-2 py-1 rounded font-mono text-xs outline-none">
            </div>
            <div class="space-y-1">
              <label class="text-studio-muted font-semibold">Last Word Mask (BLTALWM):</label>
              <input type="text" id="input-alwm" value="$FFFF" class="w-full bg-studio-bg border border-studio-border text-white px-2 py-1 rounded font-mono text-xs outline-none">
            </div>
          </div>

          <div class="flex items-center space-x-4 pt-1">
            <label class="flex items-center space-x-1.5 cursor-pointer">
              <input type="checkbox" id="chk-desc" class="rounded text-blue-500">
              <span class="text-studio-text">Descending Mode (DESC)</span>
            </label>
            <label class="flex items-center space-x-1.5 cursor-pointer">
              <input type="checkbox" id="chk-fill" class="rounded text-blue-500">
              <span class="text-studio-text">Area Fill (IFE)</span>
            </label>
            <label class="flex items-center space-x-1.5 cursor-pointer">
              <input type="checkbox" id="chk-line" class="rounded text-blue-500">
              <span class="text-studio-text">Line Mode (LINE)</span>
            </label>
          </div>
        </div>
      </div>

      <!-- Right Column: Computed Registers & Assembly Output -->
      <div class="col-span-5 space-y-4">
        <!-- Live Register Summary -->
        <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-2.5 font-mono">
          <div class="font-bold text-white uppercase text-[11px] tracking-wider font-sans">Computed Hardware Registers</div>
          <div class="space-y-1.5 text-xs">
            <div class="flex justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
              <span class="text-blue-400 font-bold">BLTCON0 ($DFF040):</span>
              <span class="text-white font-bold" id="reg-bltcon0">$0FCA</span>
            </div>
            <div class="flex justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
              <span class="text-blue-400 font-bold">BLTCON1 ($DFF042):</span>
              <span class="text-white font-bold" id="reg-bltcon1">$0000</span>
            </div>
            <div class="flex justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
              <span class="text-amber-400 font-bold">BLTAFWM ($DFF044):</span>
              <span class="text-white" id="reg-bltafwm">$FFFF</span>
            </div>
            <div class="flex justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
              <span class="text-amber-400 font-bold">BLTALWM ($DFF046):</span>
              <span class="text-white" id="reg-bltalwm">$FFFF</span>
            </div>
          </div>
        </div>

        <!-- Generated Assembly Output Code -->
        <div class="space-y-1.5">
          <div class="flex items-center justify-between">
            <span class="font-bold text-white">Generated Blitter Assembly:</span>
            <span class="text-studio-muted text-[10px]">Ready to execute</span>
          </div>
          <textarea id="blitter-asm-output" class="w-full h-72 bg-studio-panel border border-studio-border text-white font-mono text-[11px] p-3 rounded-lg outline-none resize-none leading-relaxed select-text" readonly></textarea>
        </div>
      </div>
    </div>
  `;

  const chkUseA = container.querySelector('#chk-usea') as HTMLInputElement;
  const chkUseB = container.querySelector('#chk-useb') as HTMLInputElement;
  const chkUseC = container.querySelector('#chk-usec') as HTMLInputElement;
  const chkUseD = container.querySelector('#chk-used') as HTMLInputElement;

  const rangeAsh = container.querySelector('#range-ash') as HTMLInputElement;
  const rangeBsh = container.querySelector('#range-bsh') as HTMLInputElement;
  const labelAsh = container.querySelector('#label-ash') as HTMLElement;
  const labelBsh = container.querySelector('#label-bsh') as HTMLElement;

  const inputAfwm = container.querySelector('#input-afwm') as HTMLInputElement;
  const inputAlwm = container.querySelector('#input-alwm') as HTMLInputElement;

  const chkDesc = container.querySelector('#chk-desc') as HTMLInputElement;
  const chkFill = container.querySelector('#chk-fill') as HTMLInputElement;
  const chkLine = container.querySelector('#chk-line') as HTMLInputElement;

  const regBltcon0 = container.querySelector('#reg-bltcon0') as HTMLElement;
  const regBltcon1 = container.querySelector('#reg-bltcon1') as HTMLElement;
  const regBltafwm = container.querySelector('#reg-bltafwm') as HTMLElement;
  const regBltalwm = container.querySelector('#reg-bltalwm') as HTMLElement;
  const asmOutput = container.querySelector('#blitter-asm-output') as HTMLTextAreaElement;
  const mintermBadge = container.querySelector('#minterm-hex-badge') as HTMLElement;

  let currentLf = 0xca;

  function updateMintermBitsDisplay() {
    container.querySelectorAll('.minterm-bit').forEach((el) => {
      const bitIdx = parseInt(el.getAttribute('data-bit') || '0', 10);
      const isSet = (currentLf & (1 << bitIdx)) !== 0;
      const valEl = el.querySelector('.bit-val');
      if (valEl) {
        valEl.textContent = isSet ? '1' : '0';
        valEl.className = isSet ? 'font-bold text-emerald-400 text-xs bit-val' : 'font-bold text-studio-muted text-xs bit-val';
      }
    });
  }

  function compute() {
    const ash = parseInt(rangeAsh.value, 10);
    const bsh = parseInt(rangeBsh.value, 10);
    labelAsh.textContent = `${ash} px`;
    labelBsh.textContent = `${bsh} px`;

    let bltcon0 = (ash << 12) | (currentLf & 0xff);
    if (chkUseA.checked) bltcon0 |= 1 << 11;
    if (chkUseB.checked) bltcon0 |= 1 << 10;
    if (chkUseC.checked) bltcon0 |= 1 << 9;
    if (chkUseD.checked) bltcon0 |= 1 << 8;

    let bltcon1 = bsh << 12;
    if (chkDesc.checked) bltcon1 |= 1 << 1;
    if (chkFill.checked) bltcon1 |= 1 << 3;
    if (chkLine.checked) bltcon1 |= 1 << 0;

    const afwm = inputAfwm.value.trim() || '$FFFF';
    const alwm = inputAlwm.value.trim() || '$FFFF';

    const hexCon0 = `$${bltcon0.toString(16).toUpperCase().padStart(4, '0')}`;
    const hexCon1 = `$${bltcon1.toString(16).toUpperCase().padStart(4, '0')}`;

    regBltcon0.textContent = hexCon0;
    regBltcon1.textContent = hexCon1;
    regBltafwm.textContent = afwm;
    regBltalwm.textContent = alwm;

    mintermBadge.textContent = `LF = $${currentLf.toString(16).toUpperCase().padStart(2, '0')}`;
    updateMintermBitsDisplay();

    // Generate clean Amiga assembly routine
    const code = `; Amiga Blitter Blit Routine
; Generated by m68k Studio Blitter Calculator
WaitBlit:
    btst    #6,$DFF002          ; DMACONR BBUSY bit
    bne.s   WaitBlit

    lea     $DFF000,a6          ; Custom Chip Base
    move.w  #${hexCon0},BLTCON0(a6) ; Channels & Logic ($${currentLf.toString(16).toUpperCase().padStart(2, '0')})
    move.w  #${hexCon1},BLTCON1(a6) ; Shift & Mode
    move.w  #${afwm},BLTAFWM(a6)    ; First Word Mask
    move.w  #${alwm},BLTALWM(a6)    ; Last Word Mask

    ; Pointers & Modulos (Configure for your bitmap)
    move.l  #SourceA,BLTAPTH(a6)
    move.l  #SourceB,BLTBPTH(a6)
    move.l  #SourceC,BLTCPTH(a6)
    move.l  #DestD,BLTDPTH(a6)
    move.w  #0,BLTAMOD(a6)
    move.w  #0,BLTDMOD(a6)

    ; Start Blit: (Height * 64) + (Width in Words)
    move.w  #(64*64)+2,BLTSIZE(a6)
`;
    asmOutput.value = code;
  }

  // Presets wiring
  container.querySelectorAll('.preset-btn').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      const lfVal = parseInt((e.currentTarget as HTMLElement).getAttribute('data-lf') || '0', 16);
      currentLf = lfVal;
      compute();
    });
  });

  // Minterm bits interactive toggle
  container.querySelectorAll('.minterm-bit').forEach((el) => {
    el.addEventListener('click', (e) => {
      const bitIdx = parseInt((e.currentTarget as HTMLElement).getAttribute('data-bit') || '0', 10);
      currentLf ^= 1 << bitIdx;
      compute();
    });
  });

  [chkUseA, chkUseB, chkUseC, chkUseD, rangeAsh, rangeBsh, chkDesc, chkFill, chkLine].forEach((el) => {
    el.addEventListener('input', compute);
    el.addEventListener('change', compute);
  });

  inputAfwm.addEventListener('input', compute);
  inputAlwm.addEventListener('input', compute);

  container.querySelector('#btn-copy-blitter-asm')?.addEventListener('click', () => {
    navigator.clipboard.writeText(asmOutput.value);
    const btn = container.querySelector('#btn-copy-blitter-asm') as HTMLElement;
    btn.textContent = '✅ Copied!';
    setTimeout(() => (btn.textContent = '📋 Copy Code'), 1500);
  });

  container.querySelector('#btn-insert-blitter-asm')?.addEventListener('click', () => {
    onInsertCode(asmOutput.value);
  });

  compute();

  return container;
}
