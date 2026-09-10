//! Client for the on-disk workspace API.
//!
//! Projects live in real directories, not in localStorage. This module
//! is the only place the frontend talks to the filesystem, and it holds
//! the open project as the single source of truth for the shell.

const API = '/api/workspace';

export type OutputFormat = 'hunk' | 'adf' | 'binary' | 'srecord' | 'intelhex';

export interface BuildConfig {
  main: string;
  cpu: string;
  format: OutputFormat;
  origin: string;
  include_dirs: string[];
  output_name?: string | null;
}

export interface RunConfig {
  emulator: string;
  kickstart_rom?: string | null;
  model?: string | null;
  extra_args: string[];
}

export interface Manifest {
  name: string;
  version: string;
  profile: string;
  build: BuildConfig;
  run: RunConfig;
}

export interface FileEntry {
  path: string;
  name: string;
  is_dir: boolean;
  size: number;
}

export interface OpenProject {
  root: string;
  manifest: Manifest;
  files: FileEntry[];
}

export interface RecentProject {
  root: string;
  name: string;
  opened_at: number;
}

export interface TemplateInfo {
  id: string;
  name: string;
  description: string;
}

export interface BuildDiagnostic {
  file: string;
  line: number;
  message: string;
  severity: string;
}

export interface SymbolInfo {
  name: string;
  address: number;
  section?: string | null;
}

export interface BuildResult {
  success: boolean;
  artifact?: string | null;
  artifact_path?: string | null;
  byte_count: number;
  diagnostics: BuildDiagnostic[];
  symbols: SymbolInfo[];
}

/// Error carrying the backend's message, so the UI can show what
/// actually went wrong rather than a generic failure.
export class WorkspaceError extends Error {}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const detail = await res.json().catch(() => null);
    throw new WorkspaceError(detail?.error ?? `${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${API}${path}`);
  if (!res.ok) throw new WorkspaceError(`${res.status} ${res.statusText}`);
  return res.json() as Promise<T>;
}

export const workspace = {
  open: (root: string) => post<OpenProject>('/open', { root }),
  create: (root: string, name: string, template: string) =>
    post<OpenProject>('/create', { root, name, template }),
  recent: () => get<RecentProject[]>('/recent'),
  templates: () => get<TemplateInfo[]>('/templates'),
  files: (root: string) => post<FileEntry[]>('/files', { root }),
  readFile: (root: string, path: string) => post<string>('/read', { root, path }),
  writeFile: (root: string, path: string, contents: string) =>
    post<boolean>('/write', { root, path, contents }),
  build: (root: string) => post<BuildResult>('/build', { root }),
};

/// The project currently open in the shell.
///
/// Only the root path is remembered locally — everything else is read
/// from disk on open, so the file on disk always wins over stale
/// browser state.
const LAST_PROJECT_KEY = 'm68k.lastProjectRoot';

export function rememberLastProject(root: string) {
  try {
    localStorage.setItem(LAST_PROJECT_KEY, root);
  } catch {
    // Private browsing or blocked storage: reopening from the start
    // screen still works, so this is not worth surfacing.
  }
}

export function lastProjectRoot(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT_KEY);
  } catch {
    return null;
  }
}

export function forgetLastProject() {
  try {
    localStorage.removeItem(LAST_PROJECT_KEY);
  } catch {
    /* nothing to clear */
  }
}
