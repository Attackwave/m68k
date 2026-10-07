//! The start screen: what m68k Studio shows when no project is open.
//!
//! Deliberately not a generated demo project. A developer opening a tool
//! expects to reach their own work — so this offers opening a folder,
//! the projects they had open before, and creating a new one.

import { workspace, OpenProject, RecentProject, TemplateInfo } from '../workspace';

/// Single-stroke icons, drawn rather than borrowed from an emoji font,
/// so weight and terminals stay consistent with the rest of the shell.
const icons = {
  folderOpen: `<path d="M2 5.5A1.5 1.5 0 0 1 3.5 4h3.2a1 1 0 0 1 .8.4l.9 1.2h4.1A1.5 1.5 0 0 1 14 7.1M2 5.5V12a1 1 0 0 0 1 1h9.3a1 1 0 0 0 .96-.73l1.2-4.2A.8.8 0 0 0 13.7 7H5.6a1 1 0 0 0-.96.73L3 13"/>`,
  plus: `<path d="M8 3.5v9M3.5 8h9"/>`,
  clock: `<circle cx="8" cy="8" r="6"/><path d="M8 4.8V8l2.2 1.6"/>`,
  chip: `<rect x="4.5" y="4.5" width="7" height="7" rx="1"/><path d="M6.5 2v2.5M9.5 2v2.5M6.5 11.5V14M9.5 11.5V14M2 6.5h2.5M2 9.5h2.5M11.5 6.5H14M11.5 9.5H14"/>`,
};

function icon(name: keyof typeof icons, cls = 'w-4 h-4'): string {
  return `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"
    stroke-linecap="round" stroke-linejoin="round" class="${cls}" aria-hidden="true">${icons[name]}</svg>`;
}

/// Escape text interpolated into markup — project names and paths come
/// from the filesystem, not from us.
function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!
  );
}

/// The directory a project sits in, without its own folder name — that
/// is already the entry's title, and repeating it wastes the width that
/// tells the user which of two same-named projects this is.
function displayPath(root: string): string {
  const parent = root.replace(/\/[^/]*\/?$/, '');
  const home = '/home/';
  // Collapse the home prefix the way a shell prompt does.
  if (parent.startsWith(home)) {
    const rest = parent.slice(home.length).split('/').slice(1).join('/');
    return rest ? `~/${rest}` : '~';
  }
  return parent || '/';
}

/// "vor 3 Tagen" reads better than a timestamp for a recent list.
function relativeTime(unixSeconds: number): string {
  const diff = Math.max(0, Date.now() / 1000 - unixSeconds);
  const days = Math.floor(diff / 86400);
  if (days > 30) return `vor ${Math.floor(days / 30)} Mon.`;
  if (days > 0) return `vor ${days} ${days === 1 ? 'Tag' : 'Tagen'}`;
  const hours = Math.floor(diff / 3600);
  if (hours > 0) return `vor ${hours} Std.`;
  const mins = Math.floor(diff / 60);
  return mins > 0 ? `vor ${mins} Min.` : 'gerade eben';
}

export interface StartScreenOptions {
  /// Called once a project is open, with the shell taking over.
  onProjectOpened: (project: OpenProject) => void;
}

export function createStartScreen(opts: StartScreenOptions): HTMLElement {
  const root = document.createElement('div');
  // Anchored near the top rather than vertically centred: a centred
  // block drifts as the recent list grows and leaves the window looking
  // empty when it is short.
  root.className =
    'h-full w-full flex flex-col items-center bg-studio-bg text-studio-text overflow-auto pt-[14vh]';

  root.innerHTML = `
    <div class="w-full max-w-3xl px-8 py-12">
      <header class="mb-10">
        <h1 class="text-2xl font-semibold tracking-tight text-white">m68k Studio</h1>
        <p class="mt-1.5 text-sm text-studio-muted">
          Entwicklungsumgebung für Motorola 68000 und Amiga.
        </p>
      </header>

      <div class="grid gap-10 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <section aria-labelledby="start-actions">
          <h2 id="start-actions" class="text-xs font-semibold uppercase tracking-wider text-studio-muted mb-3">
            Start
          </h2>
          <div class="flex flex-col gap-1.5">
            <button data-action="open"
              class="group flex items-center gap-2.5 rounded-md px-3 py-2.5 text-left text-sm
                     bg-studio-panel hover:bg-studio-hover transition-colors
                     focus-visible:outline focus-visible:outline-2 focus-visible:outline-studio-accent">
              <span class="text-studio-accent">${icon('folderOpen')}</span>
              <span class="flex-1">
                <span class="block font-medium text-white">Projekt öffnen</span>
                <span class="block text-xs text-studio-muted">Vorhandenen Ordner mit m68k.json</span>
              </span>
            </button>
            <button data-action="new"
              class="group flex items-center gap-2.5 rounded-md px-3 py-2.5 text-left text-sm
                     bg-studio-panel hover:bg-studio-hover transition-colors
                     focus-visible:outline focus-visible:outline-2 focus-visible:outline-studio-accent">
              <span class="text-studio-accent">${icon('plus')}</span>
              <span class="flex-1">
                <span class="block font-medium text-white">Neues Projekt</span>
                <span class="block text-xs text-studio-muted">Aus einer Vorlage anlegen</span>
              </span>
            </button>
          </div>
          <div data-region="form" class="mt-3"></div>
        </section>

        <section aria-labelledby="start-recent">
          <h2 id="start-recent" class="text-xs font-semibold uppercase tracking-wider text-studio-muted mb-3">
            Zuletzt geöffnet
          </h2>
          <div data-region="recent" class="flex flex-col gap-0.5">
            <p class="text-sm text-studio-muted">Wird geladen…</p>
          </div>
        </section>
      </div>

      <p data-region="error" class="mt-8 text-sm text-red-400" hidden></p>
    </div>
  `;

  const formRegion = root.querySelector<HTMLElement>('[data-region="form"]')!;
  const recentRegion = root.querySelector<HTMLElement>('[data-region="recent"]')!;
  const errorRegion = root.querySelector<HTMLElement>('[data-region="error"]')!;

  function showError(message: string) {
    errorRegion.textContent = message;
    errorRegion.hidden = false;
  }

  function clearError() {
    errorRegion.hidden = true;
  }

  async function openRoot(rootPath: string) {
    clearError();
    try {
      const project = await workspace.open(rootPath);
      opts.onProjectOpened(project);
    } catch (e) {
      showError(e instanceof Error ? e.message : String(e));
    }
  }

  // --- Recent projects ---
  void (async () => {
    let recents: RecentProject[] = [];
    try {
      recents = await workspace.recent();
    } catch {
      recentRegion.innerHTML =
        '<p class="text-sm text-studio-muted">Liste nicht verfügbar.</p>';
      return;
    }

    if (recents.length === 0) {
      recentRegion.innerHTML = `
        <p class="text-sm text-studio-muted">
          Noch keine Projekte. Öffne einen Ordner oder lege ein neues an.
        </p>`;
      return;
    }

    recentRegion.innerHTML = recents
      .map(
        (r) => `
        <button data-root="${esc(r.root)}" title="${esc(r.root)}"
          class="flex items-baseline gap-2 rounded-md px-2 py-1.5 text-left
                 hover:bg-studio-hover transition-colors
                 focus-visible:outline focus-visible:outline-2 focus-visible:outline-studio-accent">
          <span class="text-sm text-white shrink-0">${esc(r.name)}</span>
          <span class="text-xs text-studio-muted truncate flex-1">${esc(displayPath(r.root))}</span>
          <span class="text-xs text-studio-muted shrink-0 tabular-nums">${relativeTime(r.opened_at)}</span>
        </button>`
      )
      .join('');

    recentRegion.querySelectorAll<HTMLButtonElement>('button[data-root]').forEach((btn) => {
      btn.addEventListener('click', () => void openRoot(btn.dataset.root!));
    });
  })();

  // --- Open an existing folder ---
  //
  // The browser cannot hand a real filesystem path to the server, so the
  // path is typed. A native folder picker arrives with the desktop shell.
  function renderOpenForm() {
    clearError();
    formRegion.innerHTML = `
      <form class="rounded-md border border-studio-border bg-studio-panel p-3">
        <label class="block text-xs font-medium text-studio-muted mb-1.5" for="open-path">
          Pfad zum Projektordner
        </label>
        <input id="open-path" name="path" type="text" required spellcheck="false"
          placeholder="/home/user/projekte/meine-demo"
          class="w-full rounded bg-studio-bg border border-studio-border px-2.5 py-1.5
                 text-sm font-mono text-white placeholder:text-studio-muted/60
                 focus:outline-none focus:border-studio-accent" />
        <div class="mt-2.5 flex gap-2">
          <button type="submit"
            class="rounded bg-studio-accent hover:bg-studio-accentHover px-3 py-1.5
                   text-sm font-medium text-white transition-colors">Öffnen</button>
          <button type="button" data-action="cancel"
            class="rounded px-3 py-1.5 text-sm text-studio-muted hover:text-white transition-colors">
            Abbrechen</button>
        </div>
      </form>`;

    const form = formRegion.querySelector('form')!;
    form.querySelector<HTMLInputElement>('#open-path')!.focus();
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      const path = new FormData(form).get('path');
      if (typeof path === 'string' && path.trim()) void openRoot(path.trim());
    });
    form
      .querySelector('[data-action="cancel"]')!
      .addEventListener('click', () => (formRegion.innerHTML = ''));
  }

  // --- Create a new project ---
  async function renderNewForm() {
    clearError();
    let templates: TemplateInfo[] = [];
    try {
      templates = await workspace.templates();
    } catch (e) {
      showError(e instanceof Error ? e.message : String(e));
      return;
    }

    formRegion.innerHTML = `
      <form class="rounded-md border border-studio-border bg-studio-panel p-3">
        <label class="block text-xs font-medium text-studio-muted mb-1.5" for="new-name">Name</label>
        <input id="new-name" name="name" type="text" required spellcheck="false"
          placeholder="meine-demo"
          class="w-full rounded bg-studio-bg border border-studio-border px-2.5 py-1.5
                 text-sm text-white placeholder:text-studio-muted/60
                 focus:outline-none focus:border-studio-accent" />

        <label class="mt-3 block text-xs font-medium text-studio-muted mb-1.5" for="new-path">
          Ordner (wird angelegt)
        </label>
        <input id="new-path" name="path" type="text" required spellcheck="false"
          placeholder="/home/user/projekte/meine-demo"
          class="w-full rounded bg-studio-bg border border-studio-border px-2.5 py-1.5
                 text-sm font-mono text-white placeholder:text-studio-muted/60
                 focus:outline-none focus:border-studio-accent" />

        <fieldset class="mt-3">
          <legend class="text-xs font-medium text-studio-muted mb-1.5">Vorlage</legend>
          <div class="flex flex-col gap-1">
            ${templates
              .map(
                (t, i) => `
              <label class="flex items-start gap-2.5 rounded px-2 py-1.5 cursor-pointer
                            hover:bg-studio-hover transition-colors has-[:checked]:bg-studio-accent/10">
                <input type="radio" name="template" value="${esc(t.id)}" ${i === 0 ? 'checked' : ''}
                  class="mt-0.5 accent-[#3B82F6]" />
                <span>
                  <span class="block text-sm text-white">${esc(t.name)}</span>
                  <span class="block text-xs text-studio-muted">${esc(t.description)}</span>
                </span>
              </label>`
              )
              .join('')}
          </div>
        </fieldset>

        <div class="mt-3 flex gap-2">
          <button type="submit"
            class="rounded bg-studio-accent hover:bg-studio-accentHover px-3 py-1.5
                   text-sm font-medium text-white transition-colors">Anlegen</button>
          <button type="button" data-action="cancel"
            class="rounded px-3 py-1.5 text-sm text-studio-muted hover:text-white transition-colors">
            Abbrechen</button>
        </div>
      </form>`;

    const form = formRegion.querySelector('form')!;
    const nameInput = form.querySelector<HTMLInputElement>('#new-name')!;
    const pathInput = form.querySelector<HTMLInputElement>('#new-path')!;
    nameInput.focus();

    // Typing a name fills the folder in, until the user edits it
    // themselves — then their value is left alone.
    let pathEdited = false;
    pathInput.addEventListener('input', () => (pathEdited = true));
    nameInput.addEventListener('input', () => {
      if (pathEdited) return;
      const slug = nameInput.value.trim().replace(/\s+/g, '-');
      pathInput.value = slug ? `./${slug}` : '';
    });

    form.addEventListener('submit', (e) => {
      e.preventDefault();
      const data = new FormData(form);
      const name = String(data.get('name') ?? '').trim();
      const path = String(data.get('path') ?? '').trim();
      const template = String(data.get('template') ?? '');
      if (!name || !path) return;

      clearError();
      void workspace
        .create(path, name, template)
        .then(opts.onProjectOpened)
        .catch((err) => showError(err instanceof Error ? err.message : String(err)));
    });
    form
      .querySelector('[data-action="cancel"]')!
      .addEventListener('click', () => (formRegion.innerHTML = ''));
  }

  root
    .querySelector('[data-action="open"]')!
    .addEventListener('click', renderOpenForm);
  root
    .querySelector('[data-action="new"]')!
    .addEventListener('click', () => void renderNewForm());

  return root;
}
