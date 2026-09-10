//! Hardware Registers & Bitfield Explorer Component.

import { api, HardwareRegisterInfo } from '../api';

export function createHardwareRegistersPanel(onCopyValue: (val: string) => void): {
  element: HTMLElement;
  loadRegisters: () => Promise<void>;
} {
  const container = document.createElement('div');
  container.className = 'flex-1 flex flex-col bg-studio-bg overflow-hidden text-xs font-sans select-none';

  container.innerHTML = `
    <!-- Top Search & Filter Bar -->
    <div class="h-12 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-4 shrink-0">
      <div class="flex items-center space-x-3">
        <span class="font-bold text-white uppercase text-[11px] tracking-wider">🎯 Hardware Register Database</span>
        <div class="flex items-center space-x-1 bg-studio-panel border border-studio-border rounded px-2 py-1">
          <span>🔍</span>
          <input type="text" id="reg-search-input" placeholder="Search registers (e.g. DMACON, BPLCON0, VDP)..." class="bg-transparent text-white text-xs outline-none w-64 placeholder-studio-muted">
        </div>
      </div>

      <!-- System Selector Tabs -->
      <div class="flex items-center space-x-1">
        <button class="sys-tab-btn px-2.5 py-1 rounded font-bold text-white bg-studio-panel border border-studio-border cursor-pointer" data-sys="all">
          All Systems
        </button>
        <button class="sys-tab-btn px-2.5 py-1 rounded font-medium text-studio-muted hover:text-white transition cursor-pointer" data-sys="amiga">
          🕹️ Amiga
        </button>
        <button class="sys-tab-btn px-2.5 py-1 rounded font-medium text-studio-muted hover:text-white transition cursor-pointer" data-sys="megadrive">
          🎮 Mega Drive
        </button>
      </div>
    </div>

    <!-- Main Content: Left List / Right Details -->
    <div class="flex-1 flex overflow-hidden">
      <!-- Register List -->
      <div class="w-80 border-r border-studio-border flex flex-col overflow-y-auto bg-studio-sidebar/40 divide-y divide-studio-border/30" id="reg-list-container">
        <div class="p-4 text-center text-studio-muted">Loading register database...</div>
      </div>

      <!-- Register Detail View -->
      <div class="flex-1 p-6 overflow-y-auto space-y-4" id="reg-detail-container">
        <div class="text-center py-20 text-studio-muted">
          Select a hardware register on the left to inspect bitfields and compute mask values
        </div>
      </div>
    </div>
  `;

  let allRegisters: HardwareRegisterInfo[] = [];
  let currentFilter = 'all';
  let searchQuery = '';
  let selectedRegister: HardwareRegisterInfo | null = null;

  const listContainer = container.querySelector('#reg-list-container') as HTMLElement;
  const detailContainer = container.querySelector('#reg-detail-container') as HTMLElement;
  const searchInput = container.querySelector('#reg-search-input') as HTMLInputElement;

  async function loadRegisters() {
    allRegisters = await api.getHardwareRegisters();
    renderList();
  }

  function renderList() {
    const filtered = allRegisters.filter((r) => {
      const matchSys = currentFilter === 'all' || r.system === currentFilter;
      const matchSearch =
        searchQuery === '' ||
        r.name.toLowerCase().includes(searchQuery) ||
        r.address_hex.toLowerCase().includes(searchQuery) ||
        r.description.toLowerCase().includes(searchQuery);
      return matchSys && matchSearch;
    });

    if (filtered.length === 0) {
      listContainer.innerHTML = '<div class="p-6 text-center text-studio-muted">No registers match query</div>';
      return;
    }

    listContainer.innerHTML = filtered
      .map(
        (r) => `
      <div class="p-2.5 hover:bg-studio-hover cursor-pointer transition reg-item ${
        selectedRegister?.name === r.name ? 'bg-blue-600/20 border-l-2 border-blue-500' : ''
      }" data-name="${r.name}">
        <div class="flex items-center justify-between font-mono">
          <span class="font-bold text-white text-xs">${r.name}</span>
          <span class="text-blue-400 font-semibold text-[11px]">${r.address_hex}</span>
        </div>
        <div class="text-[10px] text-studio-muted truncate mt-0.5">${r.description}</div>
      </div>
    `
      )
      .join('');

    listContainer.querySelectorAll('.reg-item').forEach((el) => {
      el.addEventListener('click', (e) => {
        const name = (e.currentTarget as HTMLElement).getAttribute('data-name');
        selectedRegister = allRegisters.find((r) => r.name === name) || null;
        renderList();
        renderDetail();
      });
    });
  }

  function renderDetail() {
    if (!selectedRegister) {
      detailContainer.innerHTML = '<div class="text-center py-20 text-studio-muted">Select a register</div>';
      return;
    }

    const r = selectedRegister;

    detailContainer.innerHTML = `
      <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
        <div class="flex items-center justify-between">
          <div>
            <div class="flex items-center space-x-2">
              <span class="text-base font-bold text-white font-mono">${r.name}</span>
              <span class="px-2 py-0.5 rounded bg-blue-600/20 text-blue-400 font-mono text-xs font-bold">${r.address_hex}</span>
              <span class="px-2 py-0.5 rounded bg-studio-bg text-studio-muted font-mono text-[10px]">${r.read_write}</span>
            </div>
            <p class="text-studio-muted text-xs mt-1">${r.description}</p>
          </div>
          <button id="btn-copy-address" class="px-3 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-white font-mono text-xs transition cursor-pointer">
            📋 Copy Address
          </button>
        </div>
      </div>

      <!-- Bitfields Table -->
      <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
        <div class="font-bold text-white uppercase text-[11px] tracking-wider">Bitfield Breakdown & Documentation</div>
        <div class="space-y-1.5 divide-y divide-studio-border/30">
          ${r.bitfields
            .map(
              (bf) => `
            <div class="pt-2 flex items-start space-x-3">
              <span class="w-14 font-mono font-bold text-amber-400 text-xs shrink-0">Bit ${bf.bit_range}</span>
              <div class="flex-1">
                <span class="font-bold text-white font-mono text-xs mr-2">${bf.name}</span>
                <span class="text-studio-muted text-xs">${bf.description}</span>
              </div>
            </div>
          `
            )
            .join('')}
        </div>
      </div>
    `;

    detailContainer.querySelector('#btn-copy-address')?.addEventListener('click', () => {
      onCopyValue(r.address_hex);
      const btn = detailContainer.querySelector('#btn-copy-address') as HTMLElement;
      btn.textContent = '✅ Copied!';
      setTimeout(() => (btn.textContent = '📋 Copy Address'), 1500);
    });
  }

  // Search input
  searchInput.addEventListener('input', () => {
    searchQuery = searchInput.value.toLowerCase().trim();
    renderList();
  });

  // System tabs
  container.querySelectorAll('.sys-tab-btn').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      container.querySelectorAll('.sys-tab-btn').forEach((b) => {
        b.className = 'sys-tab-btn px-2.5 py-1 rounded font-medium text-studio-muted hover:text-white transition cursor-pointer';
      });
      (e.currentTarget as HTMLElement).className =
        'sys-tab-btn px-2.5 py-1 rounded font-bold text-white bg-studio-panel border border-studio-border cursor-pointer';
      currentFilter = (e.currentTarget as HTMLElement).getAttribute('data-sys') || 'all';
      renderList();
    });
  });

  return { element: container, loadRegisters };
}
