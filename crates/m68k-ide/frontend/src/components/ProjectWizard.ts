//! Project Wizard & Template Selection Dialog for m68k Studio.

import {
  ProjectTemplate,
  getAllProjectTemplates,
  createProjectFromTemplate,
  saveCustomTemplate,
  StudioProject,
} from '../project';
import { loadAllProfiles } from '../profiles';
import { showToast } from './NotificationSystem';

export interface ProjectWizardOptions {
  onProjectCreated: (project: StudioProject) => void;
  currentProject?: StudioProject;
}

export function showNewProjectWizard(options: ProjectWizardOptions) {
  const overlay = document.createElement('div');
  overlay.id = 'project-wizard-overlay';
  overlay.className =
    'fixed inset-0 z-[9999] bg-black/80 backdrop-blur-md flex items-center justify-center p-6 select-none';

  const templates = getAllProjectTemplates();
  let selectedTemplateId = templates[0]?.id || 'amiga500_py';
  let activeFilter: 'all' | 'amiga' | 'megadrive' | 'atarist' | 'baremetal' = 'all';

  overlay.innerHTML = `
    <div class="bg-studio-panel border border-studio-border rounded-2xl max-w-3xl w-full h-[620px] flex flex-col shadow-2xl overflow-hidden animate-fade-in text-xs font-sans">
      <!-- Wizard Header -->
      <div class="px-6 py-4 border-b border-studio-border bg-studio-bg/60 flex items-center justify-between shrink-0">
        <div class="flex items-center space-x-3">
          <div class="w-8 h-8 rounded-xl bg-blue-600/20 text-blue-400 flex items-center justify-center text-lg font-bold">
            📦
          </div>
          <div>
            <h2 class="text-sm font-bold text-white tracking-wide">Neues Projekt aus Vorlage</h2>
            <p class="text-[11px] text-studio-muted mt-0.5">Wähle eine Zielplattform & Entwicklungssprache (Python oder m68k Assembler)</p>
          </div>
        </div>
        <button id="wizard-btn-close" class="text-studio-muted hover:text-white p-1 text-base cursor-pointer">✕</button>
      </div>

      <!-- Wizard Main Content Area -->
      <div class="flex-1 flex overflow-hidden">
        <!-- Left: Template List & Filter -->
        <div class="w-2/5 border-r border-studio-border bg-studio-bg/30 p-4 flex flex-col space-y-3 overflow-hidden">
          <!-- Platform Filter Pills -->
          <div class="flex flex-wrap gap-1.5 shrink-0" id="wizard-filter-pills">
            <button class="filter-pill px-2.5 py-1 rounded-lg text-[10px] font-bold transition cursor-pointer bg-blue-600 text-white" data-filter="all">Alle</button>
            <button class="filter-pill px-2.5 py-1 rounded-lg text-[10px] font-medium transition cursor-pointer bg-studio-panel text-studio-muted hover:text-white" data-filter="amiga">Amiga</button>
            <button class="filter-pill px-2.5 py-1 rounded-lg text-[10px] font-medium transition cursor-pointer bg-studio-panel text-studio-muted hover:text-white" data-filter="megadrive">Mega Drive</button>
            <button class="filter-pill px-2.5 py-1 rounded-lg text-[10px] font-medium transition cursor-pointer bg-studio-panel text-studio-muted hover:text-white" data-filter="baremetal">Bare Metal</button>
          </div>

          <!-- Template Cards List -->
          <div class="flex-1 overflow-y-auto space-y-2 pr-1" id="wizard-template-list">
            <!-- Rendered by renderTemplateCards() -->
          </div>
        </div>

        <!-- Right: Template Details & Project Config -->
        <div class="w-3/5 p-6 flex flex-col justify-between overflow-y-auto space-y-5" id="wizard-details-pane">
          <!-- Rendered dynamically -->
        </div>
      </div>

      <!-- Wizard Footer -->
      <div class="px-6 py-3.5 border-t border-studio-border bg-studio-bg/60 flex items-center justify-between shrink-0">
        <button id="wizard-btn-save-as-tpl" class="px-3 py-1.5 bg-studio-panel hover:bg-studio-hover text-studio-muted hover:text-white border border-studio-border rounded-lg text-xs font-medium transition flex items-center space-x-1.5 cursor-pointer">
          <span>💾 Aktuelles Projekt als Template sichern</span>
        </button>
        <div class="flex items-center space-x-2.5">
          <button id="wizard-btn-cancel" class="px-4 py-2 bg-studio-bg hover:bg-studio-hover text-studio-text hover:text-white border border-studio-border rounded-lg font-semibold transition cursor-pointer">
            Abbrechen
          </button>
          <button id="wizard-btn-create" class="px-5 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg font-bold transition shadow cursor-pointer flex items-center space-x-1.5">
            <span>🚀 Projekt erstellen</span>
          </button>
        </div>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);

  const close = () => overlay.remove();
  overlay.querySelector('#wizard-btn-close')?.addEventListener('click', close);
  overlay.querySelector('#wizard-btn-cancel')?.addEventListener('click', close);

  // Render Template Cards
  function renderTemplateCards() {
    const listEl = overlay.querySelector('#wizard-template-list');
    if (!listEl) return;

    const filtered = templates.filter((t) => {
      if (activeFilter === 'all') return true;
      return t.system === activeFilter;
    });

    listEl.innerHTML = filtered
      .map(
        (t) => `
      <div class="template-card p-3 rounded-xl border transition cursor-pointer ${
        t.id === selectedTemplateId
          ? 'bg-blue-600/20 border-blue-500 text-white shadow-md'
          : 'bg-studio-panel/70 border-studio-border/60 text-studio-text hover:bg-studio-panel hover:border-studio-border'
      }" data-tid="${t.id}">
        <div class="flex items-center justify-between">
          <div class="flex items-center space-x-2 font-bold text-xs">
            <span>${t.icon}</span>
            <span class="truncate">${t.name}</span>
          </div>
          <span class="px-1.5 py-0.5 rounded text-[9px] font-mono font-bold uppercase ${
            t.language === 'python' ? 'bg-amber-500/20 text-amber-300' : 'bg-blue-500/20 text-blue-300'
          }">
            ${t.language === 'python' ? '🐍 Python' : '⚙️ ASM'}
          </span>
        </div>
        <p class="text-[10px] text-studio-muted mt-1.5 line-clamp-2 leading-relaxed">${t.description}</p>
      </div>
    `
      )
      .join('');

    listEl.querySelectorAll('.template-card').forEach((card) => {
      card.addEventListener('click', () => {
        selectedTemplateId = card.getAttribute('data-tid') || '';
        renderTemplateCards();
        renderTemplateDetails();
      });
    });
  }

  // Render Details Pane
  function renderTemplateDetails() {
    const detailsPane = overlay.querySelector('#wizard-details-pane');
    if (!detailsPane) return;

    const t = templates.find((tpl) => tpl.id === selectedTemplateId) || templates[0];
    if (!t) return;

    const profiles = loadAllProfiles();

    detailsPane.innerHTML = `
      <div class="space-y-4">
        <div>
          <div class="flex items-center space-x-2">
            <span class="text-2xl">${t.icon}</span>
            <div>
              <h3 class="text-sm font-bold text-white">${t.name}</h3>
              <div class="flex items-center space-x-2 mt-0.5 text-[10px] text-studio-muted">
                <span>System: <strong class="text-studio-text uppercase">${t.system}</strong></span>
                <span>•</span>
                <span>Sprache: <strong class="text-studio-text uppercase">${t.language === 'python' ? 'Python (m68k Transpiler)' : 'Pure 68000 Assembly'}</strong></span>
              </div>
            </div>
          </div>
          <p class="text-xs text-studio-muted mt-3 leading-relaxed bg-studio-bg/60 p-3 rounded-xl border border-studio-border/50">${t.description}</p>
        </div>

        <!-- Project Settings Form -->
        <div class="space-y-3 pt-1">
          <div class="space-y-1">
            <label class="font-bold text-white text-xs">Projektname:</label>
            <input type="text" id="wizard-proj-name" value="${t.name.replace(/[^a-zA-Z0-9_-]/g, '')}" class="w-full bg-studio-bg border border-studio-border focus:border-blue-500 text-white px-3 py-2 rounded-lg outline-none font-medium text-xs shadow-inner">
          </div>

          <div class="space-y-1">
            <label class="font-bold text-white text-xs">Ziel-Profil (Hardware & Toolchain):</label>
            <select id="wizard-proj-profile" class="w-full bg-studio-bg border border-studio-border focus:border-blue-500 text-white px-3 py-2 rounded-lg outline-none font-medium text-xs shadow-inner cursor-pointer">
              ${Object.values(profiles)
                .map(
                  (p) =>
                    `<option value="${p.id}" ${p.id === t.defaultProfileId ? 'selected' : ''}>${p.name} (CPU: ${p.targetCpu})</option>`
                )
                .join('')}
            </select>
          </div>
        </div>

        <!-- Initial Files Preview -->
        <div class="space-y-1.5">
          <label class="font-bold text-studio-muted uppercase text-[10px] tracking-wider">Enthaltene Dateien:</label>
          <div class="bg-studio-bg/80 border border-studio-border rounded-xl p-2.5 space-y-1 font-mono text-[11px] max-h-28 overflow-y-auto">
            ${Object.keys(t.files)
              .map(
                (f) => `
              <div class="flex items-center space-x-2 text-studio-text">
                <span class="text-studio-muted">${f.endsWith('.py') ? '🐍' : f.endsWith('.s') ? '⚙️' : '📄'}</span>
                <span>${f}</span>
                ${f === t.mainFile ? '<span class="text-[9px] px-1.5 py-0.2 rounded bg-blue-500/20 text-blue-400 font-bold">Main</span>' : ''}
              </div>
            `
              )
              .join('')}
          </div>
        </div>
      </div>
    `;
  }

  // Filter Pills events
  overlay.querySelectorAll('.filter-pill').forEach((pill) => {
    pill.addEventListener('click', () => {
      overlay.querySelectorAll('.filter-pill').forEach((p) => {
        p.classList.remove('bg-blue-600', 'text-white', 'font-bold');
        p.classList.add('bg-studio-panel', 'text-studio-muted', 'font-medium');
      });
      pill.classList.remove('bg-studio-panel', 'text-studio-muted', 'font-medium');
      pill.classList.add('bg-blue-600', 'text-white', 'font-bold');

      activeFilter = (pill.getAttribute('data-filter') as any) || 'all';
      renderTemplateCards();
    });
  });

  // Create Project Click
  overlay.querySelector('#wizard-btn-create')?.addEventListener('click', () => {
    const t = templates.find((tpl) => tpl.id === selectedTemplateId) || templates[0];
    if (!t) return;

    const nameInput = overlay.querySelector('#wizard-proj-name') as HTMLInputElement;
    const profileSelect = overlay.querySelector('#wizard-proj-profile') as HTMLSelectElement;

    const projName = nameInput?.value.trim() || t.name;
    const selectedProfId = profileSelect?.value || t.defaultProfileId;

    const newProject = createProjectFromTemplate(t, projName);
    newProject.profileId = selectedProfId;

    close();
    options.onProjectCreated(newProject);
    showToast(`Neues Projekt "${newProject.name}" erstellt!`, 'success');
  });

  // Save current project as template
  overlay.querySelector('#wizard-btn-save-as-tpl')?.addEventListener('click', () => {
    if (!options.currentProject) {
      showToast('Kein aktives Projekt zum Sichern vorhanden.', 'warning');
      return;
    }
    const cur = options.currentProject;
    const customTpl: ProjectTemplate = {
      id: `custom_tpl_${Date.now()}`,
      name: `${cur.name} (Benutzer-Template)`,
      system: 'custom',
      language: Object.keys(cur.files).some((f) => f.endsWith('.py')) ? 'python' : 'm68k',
      defaultProfileId: cur.profileId,
      description: `Benutzerdefiniertes Template aus Projekt ${cur.name}.`,
      icon: '⭐',
      files: { ...cur.files },
      mainFile: cur.mainFile,
      isCustom: true,
    };
    saveCustomTemplate(customTpl);
    showToast(`Vorlage "${customTpl.name}" gespeichert!`, 'success');
    renderTemplateCards();
  });

  // Initial render
  renderTemplateCards();
  renderTemplateDetails();
}
