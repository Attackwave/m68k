//! VS Code Style Run & Debug Sidebar View for m68k Studio.

import { PlatformProfile } from '../profiles';

export interface RunDebugViewProps {
  currentProfile: PlatformProfile;
  profiles: Record<string, PlatformProfile>;
  onProfileChange: (profileId: string) => void;
  onRun: () => void;
  onBuild: () => void;
  onOpenSettings: () => void;
}

export function createRunDebugView(props: RunDebugViewProps): {
  element: HTMLElement;
  updateProfile: (profile: PlatformProfile) => void;
} {
  const container = document.createElement('div');
  container.className =
    'h-full flex flex-col bg-studio-bg text-studio-text select-none text-xs font-sans';

  let profile = props.currentProfile;

  container.innerHTML = `
    <!-- Run Top Header -->
    <div class="px-3 py-2.5 border-b border-studio-border bg-studio-panel/70 flex items-center justify-between shrink-0">
      <span class="font-bold text-xs text-white uppercase tracking-wider">AUSFÜHREN & EMULATION</span>
      <button id="rundbg-btn-settings" aria-label="Emulator-Einstellungen öffnen" class="p-1 hover:text-white rounded transition cursor-pointer text-studio-muted" title="Emulator- und Profileinstellungen">
        ⚙️
      </button>
    </div>

    <!-- Main Content Area -->
    <div class="flex-1 overflow-y-auto p-3 space-y-4">
      <!-- Target Configuration Card -->
      <div class="bg-studio-panel border border-studio-border rounded-xl p-3 space-y-2.5">
        <div class="text-[10px] font-bold uppercase text-studio-muted tracking-wider">Ziel-Konfiguration</div>
        <div class="space-y-1">
          <label class="text-[11px] font-semibold text-white">Aktives Profil:</label>
          <select id="rundbg-profile-select" aria-label="Aktives Plattform-Profil" class="w-full bg-studio-bg border border-studio-border text-white px-2.5 py-1.5 rounded-lg outline-none focus:border-blue-500 text-xs font-medium cursor-pointer">
            ${renderProfileOptions(props.profiles, profile.id)}
          </select>
        </div>

        <div class="pt-1">
          <button id="rundbg-btn-run" class="w-full py-2 bg-emerald-600 hover:bg-emerald-500 text-white font-bold rounded-lg transition shadow flex items-center justify-center space-x-1.5 cursor-pointer">
            <span>▶ Starten im Emulator (F5)</span>
          </button>
        </div>
      </div>

      <!-- Drive & Media Slots Mapping -->
      <div class="bg-studio-panel/50 border border-studio-border/60 rounded-xl p-3 space-y-2 text-xs" id="rundbg-slots-card">
        <div class="text-[10px] font-bold uppercase text-studio-muted tracking-wider">Virtuelle Laufwerke & Medien</div>
        <div class="space-y-2 font-mono text-[11px]">
          <div class="flex items-center justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
            <span class="text-blue-400 font-bold" id="slot-0-name">DF0: (Boot)</span>
            <span class="text-studio-muted text-[10px] truncate max-w-[120px]" id="slot-0-val">Build-Output Disk</span>
          </div>
          <div class="flex items-center justify-between p-1.5 bg-studio-bg rounded border border-studio-border">
            <span class="text-studio-muted font-bold" id="slot-1-name">DF1:</span>
            <span class="text-studio-muted text-[10px]" id="slot-1-val">[Leer]</span>
          </div>
        </div>
      </div>

      <!-- Hardware Spec Details -->
      <div class="bg-studio-panel/50 border border-studio-border/60 rounded-xl p-3 space-y-2 text-xs">
        <div class="text-[10px] font-bold uppercase text-studio-muted tracking-wider">Hardware-Spezifikation</div>
        <div class="space-y-1 font-mono text-[11px]">
          <div class="flex justify-between">
            <span class="text-studio-muted">CPU:</span>
            <span class="text-white font-bold" id="rundbg-cpu">Motorola ${profile.targetCpu}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-studio-muted">Emulator:</span>
            <span class="text-blue-400 font-semibold" id="rundbg-emu">${profile.defaultEmulator.toUpperCase()}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-studio-muted">RAM:</span>
            <span class="text-studio-text" id="rundbg-ram">${profile.memory.ramLabel}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-studio-muted">Format:</span>
            <span class="text-amber-400 uppercase" id="rundbg-fmt">${
              profile.buildOptions?.defaultOutputFormat || profile.defaultOutputFormat
            }</span>
          </div>
        </div>
      </div>
    </div>
  `;

  function renderProfileOptions(
    profiles: Record<string, PlatformProfile>,
    activeId: string
  ): string {
    return Object.values(profiles)
      .map((p) => `<option value="${p.id}" ${p.id === activeId ? 'selected' : ''}>${p.name}</option>`)
      .join('');
  }

  const selectEl = container.querySelector('#rundbg-profile-select') as HTMLSelectElement;
  selectEl.addEventListener('change', () => props.onProfileChange(selectEl.value));

  container.querySelector('#rundbg-btn-run')?.addEventListener('click', props.onRun);
  container.querySelector('#rundbg-btn-settings')?.addEventListener('click', props.onOpenSettings);

  function updateProfile(newProfile: PlatformProfile) {
    profile = newProfile;
    if (selectEl) selectEl.value = newProfile.id;
    const cpuEl = container.querySelector('#rundbg-cpu');
    const emuEl = container.querySelector('#rundbg-emu');
    const ramEl = container.querySelector('#rundbg-ram');
    const fmtEl = container.querySelector('#rundbg-fmt');
    const slot0Name = container.querySelector('#slot-0-name');
    const slot0Val = container.querySelector('#slot-0-val');

    if (cpuEl) cpuEl.textContent = `Motorola ${newProfile.targetCpu}`;
    if (emuEl) emuEl.textContent = newProfile.defaultEmulator.toUpperCase();
    if (ramEl) ramEl.textContent = newProfile.memory.ramLabel;
    if (fmtEl)
      fmtEl.textContent = (
        newProfile.buildOptions?.defaultOutputFormat || newProfile.defaultOutputFormat
      ).toUpperCase();

    if (newProfile.system === 'megadrive') {
      if (slot0Name) slot0Name.textContent = 'CART:';
      if (slot0Val) slot0Val.textContent = 'Genesis ROM (.bin)';
    } else if (newProfile.system === 'atarist') {
      if (slot0Name) slot0Name.textContent = 'FLOPPY A:';
      if (slot0Val) slot0Val.textContent = 'TOS Executable (.prg)';
    } else {
      if (slot0Name) slot0Name.textContent = 'DF0: (Boot)';
      if (slot0Val) slot0Val.textContent = 'Amiga Boot Disk (.adf)';
    }
  }

  return {
    element: container,
    updateProfile,
  };
}
