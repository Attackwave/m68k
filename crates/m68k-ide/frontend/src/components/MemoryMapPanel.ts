//! Visual Memory Map & Section Footprint Analyzer Component.

export function createMemoryMapPanel(): {
  element: HTMLElement;
  updateStats: (
    codeBytes: number,
    dataBytes?: number,
    bssBytes?: number,
    totalRamKb?: number,
    ramLabel?: string,
    cpuName?: string
  ) => void;
} {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-y-auto p-5 space-y-4 text-xs font-sans select-none';

  container.innerHTML = `
    <!-- Header -->
    <div class="border-b border-studio-border pb-3">
      <h2 class="text-sm font-bold text-white tracking-wide">🗺️ MEMORY MAP & FOOTPRINT ANALYZER</h2>
      <p class="text-studio-muted text-xs">Visual breakdown of target memory architecture usage, sections (CODE, DATA, BSS), and memory limits.</p>
    </div>

    <!-- Memory Map Visual Bar -->
    <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
      <div class="flex items-center justify-between">
        <span class="font-bold text-white uppercase text-[11px] tracking-wider" id="mem-title-label">Target Memory Allocation (512 KB)</span>
        <span class="text-xs font-mono text-emerald-400 font-bold" id="mem-used-percent">0.2% Used</span>
      </div>

      <!-- Segmented Bar -->
      <div class="h-6 w-full bg-studio-bg border border-studio-border rounded-lg overflow-hidden flex shadow-inner">
        <div id="bar-code" class="bg-blue-500 h-full transition-all duration-300 relative group cursor-pointer" style="width: 1%;">
          <div class="absolute bottom-full mb-1 hidden group-hover:block bg-black/90 text-white text-[10px] px-2 py-0.5 rounded shadow whitespace-nowrap z-20 font-mono">
            CODE: <span id="tooltip-code">0 B</span>
          </div>
        </div>
        <div id="bar-data" class="bg-purple-500 h-full transition-all duration-300 relative group cursor-pointer" style="width: 0.5%;">
          <div class="absolute bottom-full mb-1 hidden group-hover:block bg-black/90 text-white text-[10px] px-2 py-0.5 rounded shadow whitespace-nowrap z-20 font-mono">
            DATA: <span id="tooltip-data">0 B</span>
          </div>
        </div>
        <div id="bar-bss" class="bg-amber-500 h-full transition-all duration-300 relative group cursor-pointer" style="width: 0.5%;">
          <div class="absolute bottom-full mb-1 hidden group-hover:block bg-black/90 text-white text-[10px] px-2 py-0.5 rounded shadow whitespace-nowrap z-20 font-mono">
            BSS: <span id="tooltip-bss">0 B</span>
          </div>
        </div>
        <div id="bar-free" class="bg-studio-sidebar/40 h-full flex-1"></div>
      </div>

      <!-- Legend -->
      <div class="flex items-center space-x-6 text-[11px] pt-1 font-mono">
        <div class="flex items-center space-x-1.5">
          <div class="w-3 h-3 rounded bg-blue-500"></div>
          <span class="text-white">CODE: <span id="label-code-size">0 B</span></span>
        </div>
        <div class="flex items-center space-x-1.5">
          <div class="w-3 h-3 rounded bg-purple-500"></div>
          <span class="text-white">DATA: <span id="label-data-size">0 B</span></span>
        </div>
        <div class="flex items-center space-x-1.5">
          <div class="w-3 h-3 rounded bg-amber-500"></div>
          <span class="text-white">BSS: <span id="label-bss-size">0 B</span></span>
        </div>
        <div class="flex items-center space-x-1.5">
          <div class="w-3 h-3 rounded bg-studio-sidebar border border-studio-border"></div>
          <span class="text-studio-muted">FREE: <span id="label-free-size">512 KB</span></span>
        </div>
      </div>
    </div>

    <!-- Statistics Cards -->
    <div class="grid grid-cols-4 gap-4">
      <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-1">
        <div class="text-[10px] text-studio-muted uppercase font-bold">Total Assembled Size</div>
        <div class="text-lg font-bold text-white font-mono" id="stat-total-size">0 Bytes</div>
      </div>
      <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-1">
        <div class="text-[10px] text-studio-muted uppercase font-bold">Free Memory</div>
        <div class="text-lg font-bold text-emerald-400 font-mono" id="stat-free-kb">512.0 KB</div>
      </div>
      <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-1">
        <div class="text-[10px] text-studio-muted uppercase font-bold">Target Architecture</div>
        <div class="text-lg font-bold text-blue-400 font-mono" id="stat-arch-label">Amiga 500</div>
      </div>
      <div class="p-3.5 bg-studio-panel border border-studio-border rounded-lg space-y-1">
        <div class="text-[10px] text-studio-muted uppercase font-bold">Target CPU Architecture</div>
        <div class="text-lg font-bold text-amber-400 font-mono" id="stat-cpu-label">Motorola 68000</div>
      </div>
    </div>
  `;

  const barCode = container.querySelector('#bar-code') as HTMLElement;
  const barData = container.querySelector('#bar-data') as HTMLElement;
  const barBss = container.querySelector('#bar-bss') as HTMLElement;

  const tooltipCode = container.querySelector('#tooltip-code') as HTMLElement;
  const tooltipData = container.querySelector('#tooltip-data') as HTMLElement;
  const tooltipBss = container.querySelector('#tooltip-bss') as HTMLElement;

  const labelCode = container.querySelector('#label-code-size') as HTMLElement;
  const labelData = container.querySelector('#label-data-size') as HTMLElement;
  const labelBss = container.querySelector('#label-bss-size') as HTMLElement;
  const labelFree = container.querySelector('#label-free-size') as HTMLElement;

  const statTotal = container.querySelector('#stat-total-size') as HTMLElement;
  const statFree = container.querySelector('#stat-free-kb') as HTMLElement;
  const statArch = container.querySelector('#stat-arch-label') as HTMLElement;
  const statCpu = container.querySelector('#stat-cpu-label') as HTMLElement;
  const memUsedPercent = container.querySelector('#mem-used-percent') as HTMLElement;
  const titleLabel = container.querySelector('#mem-title-label') as HTMLElement;

  function updateStats(
    codeBytes: number,
    dataBytes: number = 0,
    bssBytes: number = 0,
    totalRamKb: number = 512,
    ramLabel: string = '512 KB Chip RAM',
    cpuName: string = 'Motorola 68000'
  ) {
    const totalBytes = totalRamKb * 1024;
    const used = codeBytes + dataBytes + bssBytes;
    const free = Math.max(0, totalBytes - used);

    const codePct = Math.max(0.5, (codeBytes / totalBytes) * 100);
    const dataPct = Math.max(0.2, (dataBytes / totalBytes) * 100);
    const bssPct = Math.max(0.2, (bssBytes / totalBytes) * 100);

    barCode.style.width = `${codePct}%`;
    barData.style.width = `${dataPct}%`;
    barBss.style.width = `${bssPct}%`;

    tooltipCode.textContent = `${codeBytes} B`;
    tooltipData.textContent = `${dataBytes} B`;
    tooltipBss.textContent = `${bssBytes} B`;

    labelCode.textContent = `${codeBytes} B`;
    labelData.textContent = `${dataBytes} B`;
    labelBss.textContent = `${bssBytes} B`;
    labelFree.textContent = `${(free / 1024).toFixed(1)} KB`;

    statTotal.textContent = `${used} Bytes (${(used / 1024).toFixed(2)} KB)`;
    statFree.textContent = `${(free / 1024).toFixed(1)} KB`;
    statArch.textContent = ramLabel;
    statCpu.textContent = cpuName;
    titleLabel.textContent = `Memory Allocation (${ramLabel})`;
    memUsedPercent.textContent = `${((used / totalBytes) * 100).toFixed(2)}% Used`;
  }

  return { element: container, updateStats };
}
