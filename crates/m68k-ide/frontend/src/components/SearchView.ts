//! VS Code Style Project Search & Replace Sidebar View for m68k Studio.

export interface SearchMatch {
  file: string;
  line: number;
  column: number;
  preview: string;
  matchText: string;
}

export interface SearchViewProps {
  files: Record<string, string>;
  onNavigate: (file: string, line: number, column: number) => void;
  onReplaceAll: (fileMatches: Map<string, Array<{ line: number; oldText: string; newText: string }>>) => void;
}

export function createSearchView(props: SearchViewProps): {
  element: HTMLElement;
  updateFiles: (files: Record<string, string>) => void;
} {
  const container = document.createElement('div');
  container.className = 'h-full flex flex-col bg-studio-bg text-studio-text select-none text-xs font-sans';

  let currentFiles = { ...props.files };
  let matches: SearchMatch[] = [];

  container.innerHTML = `
    <!-- Search Top Header -->
    <div class="px-3 py-2.5 border-b border-studio-border bg-studio-panel/70 flex items-center justify-between shrink-0">
      <span class="font-bold text-xs text-white uppercase tracking-wider">SUCHE & ERSETZEN</span>
      <span class="text-[10px] text-studio-muted font-mono" id="search-match-count">0 Treffer</span>
    </div>

    <!-- Search Inputs Form -->
    <div class="p-3 border-b border-studio-border bg-studio-sidebar/40 space-y-2 shrink-0">
      <div class="space-y-1">
        <div class="flex items-center space-x-1.5 bg-studio-bg border border-studio-border rounded-lg px-2.5 py-1 focus-within:border-blue-500">
          <span class="text-studio-muted text-xs">🔍</span>
          <input type="text" id="search-query" placeholder="Suchen in Dateien..." class="w-full bg-transparent text-white outline-none text-xs">
          <button id="search-case-toggle" title="Groß-/Kleinschreibung beachten (Alt+C)" class="text-[10px] font-bold px-1 rounded text-studio-muted hover:text-white cursor-pointer">Aa</button>
        </div>
      </div>

      <div class="space-y-1">
        <div class="flex items-center space-x-1.5 bg-studio-bg border border-studio-border rounded-lg px-2.5 py-1 focus-within:border-blue-500">
          <span class="text-studio-muted text-xs">✏️</span>
          <input type="text" id="search-replace" placeholder="Ersetzen mit..." class="w-full bg-transparent text-white outline-none text-xs">
          <button id="search-btn-replace-all" title="Alle ersetzen (Ctrl+Alt+Enter)" class="text-xs text-studio-muted hover:text-white p-0.5 rounded cursor-pointer">🔁</button>
        </div>
      </div>
    </div>

    <!-- Matches Tree / List -->
    <div class="flex-1 overflow-y-auto p-2 space-y-2" id="search-results-container">
      <div class="text-center py-10 text-studio-muted text-xs">
        Suchbegriff oben eingeben
      </div>
    </div>
  `;

  const queryInput = container.querySelector('#search-query') as HTMLInputElement;
  const replaceInput = container.querySelector('#search-replace') as HTMLInputElement;
  const caseToggle = container.querySelector('#search-case-toggle') as HTMLElement;
  const matchCountEl = container.querySelector('#search-match-count') as HTMLElement;
  const resultsContainer = container.querySelector('#search-results-container') as HTMLElement;

  let isMatchCase = false;

  caseToggle.addEventListener('click', () => {
    isMatchCase = !isMatchCase;
    caseToggle.classList.toggle('text-blue-400', isMatchCase);
    caseToggle.classList.toggle('bg-blue-500/20', isMatchCase);
    performSearch();
  });

  queryInput.addEventListener('input', performSearch);

  function performSearch() {
    const q = queryInput.value;
    if (!q) {
      matches = [];
      matchCountEl.textContent = '0 Treffer';
      resultsContainer.innerHTML = '<div class="text-center py-10 text-studio-muted text-xs">Suchbegriff oben eingeben</div>';
      return;
    }

    matches = [];
    const searchTarget = isMatchCase ? q : q.toLowerCase();

    for (const [filePath, content] of Object.entries(currentFiles)) {
      const lines = content.split('\n');
      for (let lineIdx = 0; lineIdx < lines.length; lineIdx++) {
        const lineContent = lines[lineIdx];
        const compareContent = isMatchCase ? lineContent : lineContent.toLowerCase();

        let startIndex = 0;
        while ((startIndex = compareContent.indexOf(searchTarget, startIndex)) !== -1) {
          matches.push({
            file: filePath,
            line: lineIdx + 1,
            column: startIndex + 1,
            preview: lineContent.trim(),
            matchText: lineContent.substring(startIndex, startIndex + q.length),
          });
          startIndex += Math.max(1, q.length);
        }
      }
    }

    matchCountEl.textContent = `${matches.length} Treffer in ${new Set(matches.map((m) => m.file)).size} Dateien`;
    renderResults();
  }

  function renderResults() {
    if (matches.length === 0) {
      resultsContainer.innerHTML = '<div class="text-center py-10 text-studio-muted text-xs">Keine Treffer gefunden</div>';
      return;
    }

    // Group matches by file
    const byFile = new Map<string, SearchMatch[]>();
    for (const m of matches) {
      if (!byFile.has(m.file)) byFile.set(m.file, []);
      byFile.get(m.file)!.push(m);
    }

    resultsContainer.innerHTML = Array.from(byFile.entries())
      .map(([file, fileMatches]) => `
        <div class="space-y-1 bg-studio-panel/40 rounded-xl p-2 border border-studio-border/50">
          <div class="flex items-center justify-between font-bold text-white text-xs px-1">
            <span class="truncate">${file}</span>
            <span class="text-[10px] text-studio-muted font-mono bg-studio-bg px-1.5 py-0.2 rounded">${fileMatches.length}</span>
          </div>
          <div class="space-y-0.5 pt-1">
            ${fileMatches
              .map(
                (m) => `
              <div class="search-match-row px-2 py-1 hover:bg-studio-hover rounded text-studio-text hover:text-white transition cursor-pointer flex items-baseline justify-between text-xs" data-file="${m.file}" data-line="${m.line}" data-col="${m.column}">
                <span class="font-mono truncate flex-1 pr-2">${m.preview}</span>
                <span class="text-[10px] text-studio-muted font-mono shrink-0">L${m.line}</span>
              </div>
            `
              )
              .join('')}
          </div>
        </div>
      `)
      .join('');

    resultsContainer.querySelectorAll('.search-match-row').forEach((row) => {
      row.addEventListener('click', () => {
        const file = row.getAttribute('data-file') || '';
        const line = parseInt(row.getAttribute('data-line') || '1', 10);
        const col = parseInt(row.getAttribute('data-col') || '1', 10);
        props.onNavigate(file, line, col);
      });
    });
  }

  container.querySelector('#search-btn-replace-all')?.addEventListener('click', () => {
    const q = queryInput.value;
    const rep = replaceInput.value;
    if (!q || matches.length === 0) return;

    const fileReplacements = new Map<string, Array<{ line: number; oldText: string; newText: string }>>();
    for (const m of matches) {
      if (!fileReplacements.has(m.file)) fileReplacements.set(m.file, []);
      fileReplacements.get(m.file)!.push({
        line: m.line,
        oldText: q,
        newText: rep,
      });
    }
    props.onReplaceAll(fileReplacements);
  });

  return {
    element: container,
    updateFiles: (newFiles: Record<string, string>) => {
      currentFiles = { ...newFiles };
      performSearch();
    },
  };
}
