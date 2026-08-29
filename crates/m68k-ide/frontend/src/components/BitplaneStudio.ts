//! Amiga Bitplane and Palette Conversion Studio Component.

import { api, BitplaneConvertResponse } from '../api';

export function createBitplaneStudio(): HTMLElement {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-y-auto p-4 space-y-4 text-xs font-sans select-none';

  container.innerHTML = `
    <div class="flex items-center justify-between border-b border-studio-border pb-3">
      <div>
        <h2 class="text-sm font-bold text-white tracking-wide">🎨 AMIGA BITPLANE & COPPER PALETTE STUDIO</h2>
        <p class="text-studio-muted text-xs">Convert PNG/BMP images into 1-8 planar/interleaved bitplanes and Copper color lists.</p>
      </div>
      <div class="flex items-center space-x-3">
        <label class="flex items-center space-x-1.5 cursor-pointer">
          <span class="text-studio-muted">Max Bitplanes:</span>
          <select id="select-planes" class="bg-studio-panel border border-studio-border text-white px-2 py-1 rounded outline-none">
            <option value="1">1 Plane (2 Colors)</option>
            <option value="2">2 Planes (4 Colors)</option>
            <option value="3">3 Planes (8 Colors)</option>
            <option value="4">4 Planes (16 Colors)</option>
            <option value="5" selected>5 Planes (32 Colors / OCS)</option>
          </select>
        </label>
        <label class="flex items-center space-x-1.5 cursor-pointer">
          <input type="checkbox" id="check-interleaved" class="rounded bg-studio-panel border-studio-border text-blue-500">
          <span class="text-studio-text">Interleaved Format</span>
        </label>
      </div>
    </div>

    <!-- Drop Zone -->
    <div id="drop-zone" class="border-2 border-dashed border-studio-border hover:border-studio-accent rounded-lg p-8 text-center cursor-pointer transition bg-studio-panel/40">
      <input type="file" id="file-input" class="hidden" accept="image/png, image/bmp, image/jpeg">
      <div class="space-y-2">
        <div class="text-3xl">🖼️</div>
        <div class="text-white font-medium">Drag & Drop an image here or <span class="text-blue-400 underline">browse</span></div>
        <div class="text-studio-muted text-[11px]">Supports PNG, BMP, JPEG (Auto-quantized to Amiga 12-bit RGB444 color palette)</div>
      </div>
    </div>

    <!-- Results Section -->
    <div id="result-section" class="hidden space-y-4">
      <!-- Summary Info & Palette Previews -->
      <div class="grid grid-cols-2 gap-4">
        <!-- Palette Preview Card -->
        <div class="p-3 bg-studio-panel border border-studio-border rounded-lg space-y-2">
          <div class="font-bold text-white flex justify-between">
            <span>Amiga 12-bit Palette (<span id="color-count">0</span> Colors)</span>
            <span id="image-dims" class="text-studio-muted font-mono">320x256</span>
          </div>
          <div id="palette-swatches" class="flex flex-wrap gap-1.5"></div>
        </div>

        <!-- Conversion Stats -->
        <div class="p-3 bg-studio-panel border border-studio-border rounded-lg space-y-2">
          <div class="font-bold text-white">Asset Details</div>
          <div class="space-y-1 text-studio-text font-mono text-[11px]">
            <div>Bitplanes Used: <span id="bitplanes-count" class="text-blue-400 font-bold">5</span></div>
            <div>Total Raw Bytes: <span id="raw-byte-count" class="text-emerald-400 font-bold">0</span> Bytes</div>
            <div>Plane Layout: <span id="layout-type" class="text-amber-400">Planar</span></div>
          </div>
        </div>
      </div>

      <!-- Generated Assembly Outputs -->
      <div class="grid grid-cols-2 gap-4">
        <!-- Copper Palette Code -->
        <div class="space-y-1.5">
          <div class="flex items-center justify-between">
            <span class="font-bold text-white">Copper Palette Assembly (COLOR00-COLOR31):</span>
            <button id="btn-copy-copper" class="px-2 py-0.5 text-[10px] bg-studio-hover hover:bg-studio-accent text-white rounded transition">
              📋 Copy Copper
            </button>
          </div>
          <textarea id="asm-copper-output" class="w-full h-44 bg-studio-panel border border-studio-border text-white font-mono text-[11px] p-2.5 rounded outline-none resize-none" readonly></textarea>
        </div>

        <!-- Bitplane Raw Data Assembly -->
        <div class="space-y-1.5">
          <div class="flex items-center justify-between">
            <span class="font-bold text-white">Bitplane Raw Data (DC.W):</span>
            <button id="btn-copy-bitplanes" class="px-2 py-0.5 text-[10px] bg-studio-hover hover:bg-studio-accent text-white rounded transition">
              📋 Copy DC.W
            </button>
          </div>
          <textarea id="asm-bitplane-output" class="w-full h-44 bg-studio-panel border border-studio-border text-white font-mono text-[11px] p-2.5 rounded outline-none resize-none" readonly></textarea>
        </div>
      </div>
    </div>
  `;

  const dropZone = container.querySelector('#drop-zone') as HTMLElement;
  const fileInput = container.querySelector('#file-input') as HTMLInputElement;
  const selectPlanes = container.querySelector('#select-planes') as HTMLSelectElement;
  const checkInterleaved = container.querySelector('#check-interleaved') as HTMLInputElement;
  const resultSection = container.querySelector('#result-section') as HTMLElement;

  let currentImageBytes: number[] | null = null;

  dropZone.addEventListener('click', () => fileInput.click());

  dropZone.addEventListener('dragover', (e) => {
    e.preventDefault();
    dropZone.classList.add('border-blue-500', 'bg-blue-500/10');
  });

  dropZone.addEventListener('dragleave', () => {
    dropZone.classList.remove('border-blue-500', 'bg-blue-500/10');
  });

  dropZone.addEventListener('drop', (e) => {
    e.preventDefault();
    dropZone.classList.remove('border-blue-500', 'bg-blue-500/10');
    if (e.dataTransfer?.files.length) {
      handleFile(e.dataTransfer.files[0]);
    }
  });

  fileInput.addEventListener('change', () => {
    if (fileInput.files?.length) {
      handleFile(fileInput.files[0]);
    }
  });

  async function handleFile(file: File) {
    const arrayBuffer = await file.arrayBuffer();
    currentImageBytes = Array.from(new Uint8Array(arrayBuffer));
    await processConversion();
  }

  async function processConversion() {
    if (!currentImageBytes) return;

    const maxPlanes = parseInt(selectPlanes.value, 10);
    const interleaved = checkInterleaved.checked;

    const res: BitplaneConvertResponse = await api.convertBitplanes(currentImageBytes, maxPlanes, interleaved);

    resultSection.classList.remove('hidden');

    (container.querySelector('#color-count') as HTMLElement).textContent = `${res.palette_hex.length}`;
    (container.querySelector('#image-dims') as HTMLElement).textContent = `${res.width}x${res.height}`;
    (container.querySelector('#bitplanes-count') as HTMLElement).textContent = `${res.bitplanes_used}`;
    (container.querySelector('#raw-byte-count') as HTMLElement).textContent = `${res.total_bytes}`;
    (container.querySelector('#layout-type') as HTMLElement).textContent = interleaved ? 'Interleaved' : 'Planar';

    const swatchesContainer = container.querySelector('#palette-swatches') as HTMLElement;
    swatchesContainer.innerHTML = res.palette_hex
      .map(
        (hex, idx) => `
        <div class="flex items-center space-x-1 px-1.5 py-0.5 bg-studio-bg border border-studio-border rounded">
          <div class="w-3.5 h-3.5 rounded-sm border border-white/20" style="background-color: ${hex}"></div>
          <span class="text-[10px] font-mono text-studio-muted">${idx}:${hex}</span>
        </div>
      `
      )
      .join('');

    (container.querySelector('#asm-copper-output') as HTMLTextAreaElement).value = res.copper_palette_asm;
    (container.querySelector('#asm-bitplane-output') as HTMLTextAreaElement).value = res.bitplane_asm_dc;
  }

  selectPlanes.addEventListener('change', processConversion);
  checkInterleaved.addEventListener('change', processConversion);

  container.querySelector('#btn-copy-copper')?.addEventListener('click', () => {
    const val = (container.querySelector('#asm-copper-output') as HTMLTextAreaElement).value;
    navigator.clipboard.writeText(val);
  });

  container.querySelector('#btn-copy-bitplanes')?.addEventListener('click', () => {
    const val = (container.querySelector('#asm-bitplane-output') as HTMLTextAreaElement).value;
    navigator.clipboard.writeText(val);
  });

  return container;
}
