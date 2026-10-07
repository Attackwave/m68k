//! Retro & Hardware Studio Sidebar Launcher View for m68k Studio.

export type RetroToolId = 'blitter' | 'bitplanes' | 'copper' | 'memorymap' | 'hwregs';

export interface RetroStudioViewProps {
  onOpenTool: (toolId: RetroToolId) => void;
}

export function createRetroStudioView(props: RetroStudioViewProps): {
  element: HTMLElement;
  setCapabilities: (hasBlitter: boolean, hasCopper: boolean, hasBitplanes: boolean) => void;
} {
  const container = document.createElement('div');
  container.className = 'h-full flex flex-col bg-studio-bg text-studio-text select-none text-xs font-sans';

  container.innerHTML = `
    <!-- Top Header -->
    <div class="px-3 py-2.5 border-b border-studio-border bg-studio-panel/70 flex items-center justify-between shrink-0">
      <span class="font-bold text-xs text-white uppercase tracking-wider">RETRO & HARDWARE TOOLS</span>
    </div>

    <!-- Tools List -->
    <div class="flex-1 overflow-y-auto p-3 space-y-2">
      <!-- Blitter Card -->
      <div id="retro-card-blitter" class="tool-card p-3 rounded-xl border border-studio-border/60 bg-studio-panel/60 hover:bg-studio-panel hover:border-blue-500 transition cursor-pointer" data-tool="blitter">
        <div class="flex items-center space-x-2 font-bold text-white text-xs">
          <span>🧮</span>
          <span>Amiga Blitter Studio</span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">
          Minterm-Rechner, Logic-Equations & Code-Generator für Amiga Custom Chip Blitter.
        </p>
      </div>

      <!-- Bitplanes Card -->
      <div id="retro-card-bitplanes" class="tool-card p-3 rounded-xl border border-studio-border/60 bg-studio-panel/60 hover:bg-studio-panel hover:border-blue-500 transition cursor-pointer" data-tool="bitplanes">
        <div class="flex items-center space-x-2 font-bold text-white text-xs">
          <span>🎨</span>
          <span>Planar Bitplane Studio</span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">
          RGB444 Farbquantisierer, ILBM/Planar Konverter & Palette-Exporter für 1-6 Bitplanes.
        </p>
      </div>

      <!-- Copper Card -->
      <div id="retro-card-copper" class="tool-card p-3 rounded-xl border border-studio-border/60 bg-studio-panel/60 hover:bg-studio-panel hover:border-blue-500 transition cursor-pointer" data-tool="copper">
        <div class="flex items-center space-x-2 font-bold text-white text-xs">
          <span>🌈</span>
          <span>Copperlist Visualizer</span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">
          Simulierter CRT-Rasterstrahl für Amiga Copper WAIT/MOVE Instruktionen.
        </p>
      </div>

      <!-- Memory Map Card -->
      <div id="retro-card-memorymap" class="tool-card p-3 rounded-xl border border-studio-border/60 bg-studio-panel/60 hover:bg-studio-panel hover:border-blue-500 transition cursor-pointer" data-tool="memorymap">
        <div class="flex items-center space-x-2 font-bold text-white text-xs">
          <span>🗺️</span>
          <span>Memory Map & RAM Layout</span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">
          Visuelle Übersicht des Adressraums (Chip RAM, Fast RAM, ROM, IO).
        </p>
      </div>

      <!-- HW Registers Card -->
      <div id="retro-card-hwregs" class="tool-card p-3 rounded-xl border border-studio-border/60 bg-studio-panel/60 hover:bg-studio-panel hover:border-blue-500 transition cursor-pointer" data-tool="hwregs">
        <div class="flex items-center space-x-2 font-bold text-white text-xs">
          <span>⚙️</span>
          <span>Hardware-Register ($DFF000 / VDP)</span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">
          Dokumentation & Adressdatenbank aller Amiga- & Mega Drive-Register.
        </p>
      </div>
    </div>
  `;

  container.querySelectorAll('.tool-card').forEach((card) => {
    card.addEventListener('click', () => {
      const toolId = card.getAttribute('data-tool') as RetroToolId;
      if (toolId) props.onOpenTool(toolId);
    });
  });

  function setCapabilities(hasBlitter: boolean, hasCopper: boolean, hasBitplanes: boolean) {
    const bCard = container.querySelector('#retro-card-blitter') as HTMLElement | null;
    const cCard = container.querySelector('#retro-card-copper') as HTMLElement | null;
    const pCard = container.querySelector('#retro-card-bitplanes') as HTMLElement | null;

    if (bCard) bCard.style.display = hasBlitter ? '' : 'none';
    if (cCard) cCard.style.display = hasCopper ? '' : 'none';
    if (pCard) pCard.style.display = hasBitplanes ? '' : 'none';
  }

  return {
    element: container,
    setCapabilities,
  };
}
