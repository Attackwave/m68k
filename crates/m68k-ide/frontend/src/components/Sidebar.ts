//! Left Sidebar Component with Explorer, Target Disk/ROM Artifacts, and Symbol Outline.

import { AdfInfoResponse } from '../api';
import { PlatformProfile } from '../profiles';

export interface SidebarProps {
  explorerElement: HTMLElement;
  currentProfile: PlatformProfile;
  onDownloadArtifact: () => void;
  adfInfo: AdfInfoResponse | null;
}

export function createSidebar(props: SidebarProps): {
  element: HTMLElement;
  updateAdf: (info: AdfInfoResponse) => void;
  updateProfile: (profile: PlatformProfile) => void;
  updateSymbols: (symbols: any[]) => void;
  setCapabilities: (hasArtifact: boolean) => void;
} {
  const sidebar = document.createElement('aside');
  sidebar.className =
    'w-64 bg-studio-sidebar border-r border-studio-border flex flex-col shrink-0 text-xs select-none';

  sidebar.innerHTML = `
    <!-- Sidebar Tabs -->
    <div class="h-9 border-b border-studio-border flex items-center px-2 space-x-1 bg-studio-bg shrink-0">
      <button id="tab-files" class="px-2.5 py-1 text-xs font-medium rounded text-white bg-studio-panel border border-studio-border flex items-center space-x-1.5 cursor-pointer">
        <span>📂 Projekt</span>
      </button>
      <button id="tab-artifact" class="px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5 cursor-pointer">
        <span id="tab-artifact-label">💾 Target-Disk</span>
      </button>
      <button id="tab-symbols" class="px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5 cursor-pointer">
        <span>🌳 Outline</span>
      </button>
    </div>

    <!-- Panel 1: Project Explorer Container -->
    <div id="view-files" class="flex-1 flex flex-col overflow-hidden">
      <!-- Embedded ProjectExplorer element -->
    </div>

    <!-- Panel 2: Dynamic Target Artifact / Disk Manager -->
    <div id="view-artifact" class="flex-1 hidden flex-col overflow-y-auto p-3 space-y-3">
      <div class="flex items-center justify-between px-1 text-studio-muted uppercase font-bold text-[10px] tracking-wider">
        <span id="artifact-section-title">Target Artifact</span>
        <button id="btn-download-artifact" title="Download Target Artifact to PC" class="px-2 py-0.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px] font-medium transition cursor-pointer">
          ⬇️ Download
        </button>
      </div>

      <div class="p-2.5 bg-studio-panel border border-studio-border rounded-xl text-xs space-y-1.5" id="artifact-info-box">
        <div class="flex justify-between">
          <span class="text-studio-muted">Typ:</span>
          <span id="artifact-type" class="font-semibold text-white">Amiga Bootable Floppy</span>
        </div>
        <div class="flex justify-between">
          <span class="text-studio-muted">Format / Ziel:</span>
          <span id="artifact-format" class="text-emerald-400 font-mono">OFS / ADF (880 KB)</span>
        </div>
        <div class="flex justify-between">
          <span class="text-studio-muted">Status:</span>
          <span id="artifact-status" class="text-studio-muted font-mono">Build ausstehend (F7)</span>
        </div>
      </div>

      <!-- File Entries / Blocks list -->
      <div class="space-y-1">
        <div class="text-[11px] font-semibold text-studio-muted px-1" id="artifact-list-title">Inhalt im Dateisystem:</div>
        <div id="artifact-entries-list" class="space-y-0.5 max-h-48 overflow-y-auto bg-studio-bg p-1 rounded-lg border border-studio-border">
          <div class="text-center py-4 text-studio-muted text-xs">Projekt kompilieren (F7) zur Vorschau</div>
        </div>
      </div>
    </div>

    <!-- Panel 3: Symbols Outline -->
    <div id="view-symbols" class="flex-1 hidden flex-col overflow-y-auto p-2">
      <div class="flex items-center justify-between px-1 mb-1.5 text-studio-muted uppercase font-bold text-[10px] tracking-wider">
        <span>Symbols & Outline</span>
      </div>
      <div id="symbols-list" class="space-y-0.5">
        <div class="text-center py-4 text-studio-muted text-xs">Keine Labels/Symbole gefunden</div>
      </div>
    </div>
  `;

  // Attach Explorer element
  const viewFilesEl = sidebar.querySelector('#view-files') as HTMLElement;
  if (viewFilesEl) {
    viewFilesEl.appendChild(props.explorerElement);
  }

  // Tabs logic
  const tabFiles = sidebar.querySelector('#tab-files') as HTMLElement;
  const tabArtifact = sidebar.querySelector('#tab-artifact') as HTMLElement;
  const tabSymbols = sidebar.querySelector('#tab-symbols') as HTMLElement;

  const viewFiles = sidebar.querySelector('#view-files') as HTMLElement;
  const viewArtifact = sidebar.querySelector('#view-artifact') as HTMLElement;
  const viewSymbols = sidebar.querySelector('#view-symbols') as HTMLElement;

  function switchTab(activeTab: HTMLElement, activeView: HTMLElement) {
    [tabFiles, tabArtifact, tabSymbols].forEach((t) => {
      t.className =
        'px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5 cursor-pointer';
    });
    [viewFiles, viewArtifact, viewSymbols].forEach((v) => v.classList.add('hidden'));

    activeTab.className =
      'px-2.5 py-1 text-xs font-medium rounded text-white bg-studio-panel border border-studio-border flex items-center space-x-1.5 cursor-pointer';
    activeView.classList.remove('hidden');
  }

  tabFiles.addEventListener('click', () => switchTab(tabFiles, viewFiles));
  tabArtifact.addEventListener('click', () => switchTab(tabArtifact, viewArtifact));
  tabSymbols.addEventListener('click', () => switchTab(tabSymbols, viewSymbols));

  sidebar.querySelector('#btn-download-artifact')?.addEventListener('click', props.onDownloadArtifact);

  function renderProfileDetails(profile: PlatformProfile) {
    const tabLabel = sidebar.querySelector('#tab-artifact-label');
    const secTitle = sidebar.querySelector('#artifact-section-title');
    const btnDl = sidebar.querySelector('#btn-download-artifact');
    const artType = sidebar.querySelector('#artifact-type');
    const artFormat = sidebar.querySelector('#artifact-format');

    if (profile.system === 'amiga') {
      if (tabLabel) tabLabel.textContent = '💾 ADF-Disk';
      if (secTitle) secTitle.textContent = 'Amiga Floppy (ADF)';
      if (btnDl) btnDl.textContent = '⬇️ Download ADF';
      if (artType) artType.textContent = 'Amiga Bootable Floppy';
      if (artFormat) artFormat.textContent = 'OFS/FFS (880 KB Diskette)';
    } else if (profile.system === 'megadrive') {
      if (tabLabel) tabLabel.textContent = '🎮 ROM (.BIN)';
      if (secTitle) secTitle.textContent = 'Sega Mega Drive ROM';
      if (btnDl) btnDl.textContent = '⬇️ Download ROM';
      if (artType) artType.textContent = 'Genesis Cartridge Image';
      if (artFormat) artFormat.textContent = 'Raw ROM Header + Binary';
    } else if (profile.system === 'atarist') {
      if (tabLabel) tabLabel.textContent = '🖥️ TOS (.PRG)';
      if (secTitle) secTitle.textContent = 'Atari ST Executable';
      if (btnDl) btnDl.textContent = '⬇️ Download PRG';
      if (artType) artType.textContent = 'TOS Executable Binary';
      if (artFormat) artFormat.textContent = 'GEMDOS / TOS Header';
    } else {
      if (tabLabel) tabLabel.textContent = '📦 Binary';
      if (secTitle) secTitle.textContent = 'Bare Metal Binary';
      if (btnDl) btnDl.textContent = '⬇️ Download Binary';
      if (artType) artType.textContent = 'Motorola 68k Binary';
      if (artFormat) artFormat.textContent = profile.buildOptions?.defaultOutputFormat || 'Raw Flat Binary';
    }
  }

  renderProfileDetails(props.currentProfile);

  function updateAdf(info: AdfInfoResponse) {
    const artStatus = sidebar.querySelector('#artifact-status');
    const artEntries = sidebar.querySelector('#artifact-entries-list');

    if (artStatus) artStatus.innerHTML = `<span class="text-emerald-400 font-bold">Erfolgreich generiert (${info.free_blocks * 512} B frei)</span>`;

    if (artEntries) {
      if (info.entries.length === 0) {
        artEntries.innerHTML = '<div class="text-center py-4 text-studio-muted text-xs">Diskette ist leer</div>';
      } else {
        artEntries.innerHTML = info.entries
          .map(
            (entry) => `
          <div class="flex items-center justify-between p-1 hover:bg-studio-hover rounded text-studio-text">
            <span class="flex items-center space-x-1.5">
              <span>${entry.is_dir ? '📁' : '📄'}</span>
              <span class="font-mono">${entry.name}</span>
            </span>
            <span class="text-studio-muted font-mono text-[10px]">${entry.size} B</span>
          </div>
        `
          )
          .join('');
      }
    }
  }

  function updateSymbols(symbols: any[]) {
    const listEl = sidebar.querySelector('#symbols-list');
    if (!listEl) return;

    if (symbols.length === 0) {
      listEl.innerHTML = '<div class="text-center py-4 text-studio-muted text-xs">Keine Labels/Symbole gefunden</div>';
      return;
    }

    listEl.innerHTML = symbols
      .map(
        (s) => `
      <div class="px-2 py-1 hover:bg-studio-hover rounded flex items-center justify-between text-studio-text cursor-pointer group">
        <span class="font-mono text-xs group-hover:text-blue-400">${s.name}</span>
        <span class="text-[10px] text-studio-muted font-mono">L${s.line + 1}</span>
      </div>
    `
      )
      .join('');
  }

  function setCapabilities(hasArtifact: boolean) {
    if (tabArtifact) {
      tabArtifact.classList.toggle('hidden', !hasArtifact);
    }
  }

  return {
    element: sidebar,
    updateAdf,
    updateProfile: renderProfileDetails,
    updateSymbols,
    setCapabilities,
  };
}
