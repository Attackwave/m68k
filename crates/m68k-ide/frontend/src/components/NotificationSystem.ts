//! Professional Studio Notifications, Toast System, and Custom Modal Dialogs.

export type ToastType = 'success' | 'error' | 'info' | 'warning';

let toastContainer: HTMLElement | null = null;

function ensureToastContainer(): HTMLElement {
  if (!toastContainer || !document.body.contains(toastContainer)) {
    toastContainer = document.createElement('div');
    toastContainer.id = 'studio-toast-container';
    toastContainer.className =
      'fixed bottom-5 right-5 z-[9999] flex flex-col space-y-2 pointer-events-none max-w-md w-full';
    document.body.appendChild(toastContainer);
  }
  return toastContainer;
}

export function showToast(message: string, type: ToastType = 'info', durationMs: number = 4000) {
  const container = ensureToastContainer();

  const toast = document.createElement('div');
  toast.className = `pointer-events-auto p-3.5 rounded-xl border shadow-2xl flex items-start space-x-3 transition-all duration-300 transform translate-y-2 opacity-0 text-xs font-sans select-none ${
    type === 'success'
      ? 'bg-studio-panel border-emerald-500/50 text-white'
      : type === 'error'
      ? 'bg-studio-panel border-red-500/50 text-white'
      : type === 'warning'
      ? 'bg-studio-panel border-amber-500/50 text-white'
      : 'bg-studio-panel border-blue-500/50 text-white'
  }`;

  const icon =
    type === 'success'
      ? '<span class="text-emerald-400 font-bold text-sm">✓</span>'
      : type === 'error'
      ? '<span class="text-red-400 font-bold text-sm">✕</span>'
      : type === 'warning'
      ? '<span class="text-amber-400 font-bold text-sm">⚠</span>'
      : '<span class="text-blue-400 font-bold text-sm">ℹ</span>';

  toast.innerHTML = `
    <div class="shrink-0 mt-0.5">${icon}</div>
    <div class="flex-1 text-studio-text leading-relaxed font-medium break-words">${message}</div>
    <button class="shrink-0 text-studio-muted hover:text-white transition cursor-pointer p-0.5">✕</button>
  `;

  const closeBtn = toast.querySelector('button');
  const dismiss = () => {
    toast.classList.add('opacity-0', 'translate-y-2');
    setTimeout(() => toast.remove(), 300);
  };

  closeBtn?.addEventListener('click', dismiss);

  container.appendChild(toast);

  // Trigger animation
  requestAnimationFrame(() => {
    toast.classList.remove('opacity-0', 'translate-y-2');
  });

  if (durationMs > 0) {
    setTimeout(dismiss, durationMs);
  }
}

export interface ConfirmDialogOptions {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  isDanger?: boolean;
  onConfirm: () => void;
}

export function showConfirmDialog(options: ConfirmDialogOptions) {
  const overlay = document.createElement('div');
  overlay.className =
    'fixed inset-0 z-[9999] bg-black/75 backdrop-blur-sm flex items-center justify-center p-4 select-none';

  overlay.innerHTML = `
    <div class="bg-studio-panel border border-studio-border rounded-2xl max-w-md w-full p-6 space-y-4 shadow-2xl animate-fade-in text-xs font-sans">
      <div class="flex items-center space-x-2.5">
        <div class="w-8 h-8 rounded-full ${
          options.isDanger ? 'bg-red-500/20 text-red-400' : 'bg-blue-500/20 text-blue-400'
        } flex items-center justify-center font-bold text-base">
          ${options.isDanger ? '⚠️' : '❓'}
        </div>
        <h3 class="text-sm font-bold text-white tracking-wide">${options.title}</h3>
      </div>
      <p class="text-studio-muted text-xs leading-relaxed">${options.message}</p>
      <div class="flex items-center justify-end space-x-2.5 pt-2">
        <button id="modal-btn-cancel" class="px-4 py-2 bg-studio-bg hover:bg-studio-hover text-studio-text hover:text-white border border-studio-border rounded-lg font-semibold transition cursor-pointer">
          ${options.cancelText || 'Abbrechen'}
        </button>
        <button id="modal-btn-confirm" class="px-4 py-2 ${
          options.isDanger ? 'bg-red-600 hover:bg-red-500' : 'bg-blue-600 hover:bg-blue-500'
        } text-white rounded-lg font-bold transition shadow cursor-pointer">
          ${options.confirmText || 'Bestätigen'}
        </button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);

  const close = () => overlay.remove();

  overlay.querySelector('#modal-btn-cancel')?.addEventListener('click', close);
  overlay.querySelector('#modal-btn-confirm')?.addEventListener('click', () => {
    close();
    options.onConfirm();
  });

  overlay.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') close();
    if (e.key === 'Enter') {
      close();
      options.onConfirm();
    }
  });
}

export interface PromptDialogOptions {
  title: string;
  message?: string;
  placeholder?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: (value: string) => void;
}

export function showPromptDialog(options: PromptDialogOptions) {
  const overlay = document.createElement('div');
  overlay.className =
    'fixed inset-0 z-[9999] bg-black/75 backdrop-blur-sm flex items-center justify-center p-4 select-none';

  overlay.innerHTML = `
    <div class="bg-studio-panel border border-studio-border rounded-2xl max-w-md w-full p-6 space-y-4 shadow-2xl text-xs font-sans">
      <div class="flex items-center space-x-2.5">
        <div class="w-8 h-8 rounded-full bg-blue-500/20 text-blue-400 flex items-center justify-center font-bold text-base">
          ✏️
        </div>
        <h3 class="text-sm font-bold text-white tracking-wide">${options.title}</h3>
      </div>
      ${options.message ? `<p class="text-studio-muted text-xs">${options.message}</p>` : ''}
      <div class="space-y-1.5">
        <input type="text" id="modal-prompt-input" value="${options.defaultValue || ''}" placeholder="${
    options.placeholder || ''
  }" class="w-full bg-studio-bg border border-studio-border focus:border-blue-500 text-white px-3.5 py-2 rounded-lg outline-none font-medium text-xs shadow-inner">
      </div>
      <div class="flex items-center justify-end space-x-2.5 pt-2">
        <button id="modal-btn-cancel" class="px-4 py-2 bg-studio-bg hover:bg-studio-hover text-studio-text hover:text-white border border-studio-border rounded-lg font-semibold transition cursor-pointer">
          ${options.cancelText || 'Abbrechen'}
        </button>
        <button id="modal-btn-confirm" class="px-5 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg font-bold transition shadow cursor-pointer">
          ${options.confirmText || 'OK'}
        </button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);

  const input = overlay.querySelector('#modal-prompt-input') as HTMLInputElement;
  input.focus();
  input.select();

  const close = () => overlay.remove();

  const handleConfirm = () => {
    const val = input.value.trim();
    if (val) {
      close();
      options.onConfirm(val);
    }
  };

  overlay.querySelector('#modal-btn-cancel')?.addEventListener('click', close);
  overlay.querySelector('#modal-btn-confirm')?.addEventListener('click', handleConfirm);

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') handleConfirm();
    if (e.key === 'Escape') close();
  });
}
