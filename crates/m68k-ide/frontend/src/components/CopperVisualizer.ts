//! Amiga Copperlist Visualizer Component.

import { CopperInstructionItem } from '../api';

export function createCopperVisualizer(): { element: HTMLElement; updateInstructions: (instructions: CopperInstructionItem[]) => void } {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-hidden text-xs font-mono select-none';

  container.innerHTML = `
    <!-- Top Bar -->
    <div class="h-9 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-3 shrink-0">
      <div class="flex items-center space-x-2">
        <span class="font-bold text-white uppercase text-[11px] tracking-wider">Amiga Copperlist Visualizer</span>
        <span id="copper-count" class="text-studio-muted text-xs">(0 instructions)</span>
      </div>
      <div class="text-[11px] text-studio-muted">
        Raster Beam Display (PAL 312 Lines / NTSC 262 Lines)
      </div>
    </div>

    <!-- Main Visualizer Content -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Instruction Table -->
      <div class="w-1/2 border-r border-studio-border flex flex-col overflow-y-auto">
        <div class="grid grid-cols-12 gap-2 px-3 py-1.5 bg-studio-panel text-studio-muted font-bold text-[10px] uppercase border-b border-studio-border shrink-0">
          <div class="col-span-3">Opcode</div>
          <div class="col-span-3">Raw Words</div>
          <div class="col-span-6">Action / Target</div>
        </div>
        <div id="copper-rows" class="divide-y divide-studio-border/30 overflow-y-auto">
          <div class="text-center py-12 text-studio-muted">No copper instructions parsed</div>
        </div>
      </div>

      <!-- Raster Timeline Preview -->
      <div class="w-1/2 p-4 flex flex-col items-center justify-start overflow-y-auto space-y-2 bg-studio-bg">
        <div class="text-studio-muted text-[11px] font-sans font-semibold">Simulated CRT Raster Timeline</div>
        <div id="raster-canvas-container" class="w-72 h-80 bg-black border-2 border-studio-border rounded-lg relative overflow-hidden flex flex-col justify-start">
          <div class="absolute inset-0 flex items-center justify-center text-studio-muted text-xs font-sans" id="raster-empty-label">
            Assemble copperlist to render beam
          </div>
        </div>
      </div>
    </div>
  `;

  const rowsEl = container.querySelector('#copper-rows') as HTMLElement;
  const countEl = container.querySelector('#copper-count') as HTMLElement;
  const canvasContainer = container.querySelector('#raster-canvas-container') as HTMLElement;
  const emptyLabel = container.querySelector('#raster-empty-label') as HTMLElement;

  function updateInstructions(instructions: CopperInstructionItem[]) {
    if (!instructions || instructions.length === 0) {
      rowsEl.innerHTML = '<div class="text-center py-12 text-studio-muted">No copper instructions parsed</div>';
      countEl.textContent = '(0 instructions)';
      emptyLabel.style.display = 'flex';
      return;
    }

    countEl.textContent = `(${instructions.length} instructions)`;
    emptyLabel.style.display = 'none';

    // Render instruction rows
    rowsEl.innerHTML = instructions
      .map((item) => {
        const opBadge =
          item.op_type === 'Move'
            ? '<span class="text-blue-400 font-bold">MOVE</span>'
            : item.op_type === 'Wait'
            ? '<span class="text-amber-400 font-bold">WAIT</span>'
            : item.op_type === 'Skip'
            ? '<span class="text-purple-400 font-bold">SKIP</span>'
            : '<span class="text-red-400 font-bold">END</span>';

        const colorPreview = item.color_preview_hex
          ? `<span class="inline-block w-3 h-3 rounded-sm border border-white/30 mr-1.5 align-middle" style="background-color: ${item.color_preview_hex}"></span>`
          : '';

        return `
          <div class="grid grid-cols-12 gap-2 px-3 py-1.5 hover:bg-studio-hover items-center">
            <div class="col-span-3 flex items-center space-x-1">${opBadge}</div>
            <div class="col-span-3 text-studio-muted text-[11px]">$${item.word1.toString(16).toUpperCase().padStart(4, '0')} $${item.word2.toString(16).toUpperCase().padStart(4, '0')}</div>
            <div class="col-span-6 text-white text-[11px] flex items-center">${colorPreview}<span>${item.description}</span></div>
          </div>
        `;
      })
      .join('');

    // Render raster gradient simulation
    let currentColor = '#000000';
    let rasterHtml = '';

    for (let line = 0; line < 312; line += 4) {
      // Check if any instruction modified color at or before this scanline
      for (const inst of instructions) {
        if (inst.vpos !== undefined && Math.abs(inst.vpos - line) <= 4 && inst.color_preview_hex) {
          currentColor = inst.color_preview_hex;
        } else if (inst.op_type === 'Move' && inst.color_preview_hex && inst.reg_name?.includes('COLOR00')) {
          currentColor = inst.color_preview_hex;
        }
      }
      rasterHtml += `<div class="w-full h-1" style="background-color: ${currentColor}"></div>`;
    }

    canvasContainer.innerHTML = rasterHtml;
  }

  return { element: container, updateInstructions };
}
