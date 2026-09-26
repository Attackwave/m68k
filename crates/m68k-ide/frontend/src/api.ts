//! API client for communicating with the m68k backend.

export interface AssembleRequest {
  source: String;
  cpu?: string;
  base_address?: number;
}

export interface AssembleErrorItem {
  line: number;
  message: string;
}

export interface AssembleResponse {
  success: boolean;
  byte_count: number;
  errors: AssembleErrorItem[];
  warnings: string[];
  binary_base64?: string;
  hex_dump?: string;
}

export interface DisassembleLineItem {
  address: number;
  address_hex: string;
  instruction_hex: string;
  text: string;
  comment?: string;
}

export interface DisassembleResponse {
  lines: DisassembleLineItem[];
}

export interface AdfEntryItem {
  name: string;
  is_dir: boolean;
  size: number;
  block: number;
}

export interface AdfInfoResponse {
  volume_name: string;
  is_ffs: boolean;
  free_blocks: number;
  total_blocks: number;
  entries: AdfEntryItem[];
}

export interface BitplaneConvertResponse {
  width: number;
  height: number;
  bitplanes_used: number;
  palette_hex: string[];
  copper_palette_asm: string;
  bitplane_asm_dc: string;
  total_bytes: number;
}

export interface CopperInstructionItem {
  op_type: string;
  word1: number;
  word2: number;
  description: string;
  vpos?: number;
  hpos?: number;
  reg_name?: string;
  color_preview_hex?: string;
}

export interface ParseCopperResponse {
  instructions: CopperInstructionItem[];
}

export interface ProjectConfig {
  name: string;
  target_cpu: string;
  main_file: string;
  output_format: string;
  origin_address: string;
  emulator_profile: string;
  include_dirs: string[];
}

export interface EmulatorProfile {
  name: string;
  executable_path: string;
  default_args: string[];
  supported_formats: string[];
  available: boolean;
}

export interface IdeDiagnosticItem {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
  message: string;
  severity: number;
}

export interface IdeCompletionItem {
  label: string;
  kind: string;
  detail?: string;
  documentation?: string;
  insert_text?: string;
}

export interface IdeHoverResponse {
  contents: string;
}

export interface IdeSymbolItem {
  name: string;
  kind: string;
  line: number;
  character: number;
}

const API_BASE = '/api';

export const api = {
  async assemble(req: AssembleRequest): Promise<AssembleResponse> {
    const res = await fetch(`${API_BASE}/assemble`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req),
    });
    return res.json();
  },

  async disassemble(bytes: number[], origin: number = 0, cpu: string = '68000'): Promise<DisassembleResponse> {
    const res = await fetch(`${API_BASE}/disassemble`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ bytes, origin, cpu }),
    });
    return res.json();
  },

  async createAdf(disk_name: string, is_ffs: boolean, boot_code?: number[]): Promise<number[]> {
    const res = await fetch(`${API_BASE}/floppy/create`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ disk_name, is_ffs, boot_code }),
    });
    return res.json();
  },

  async inspectAdf(adf_bytes: number[]): Promise<AdfInfoResponse> {
    const res = await fetch(`${API_BASE}/floppy/inspect`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(adf_bytes),
    });
    return res.json();
  },

  async convertBitplanes(image_bytes: number[], max_bitplanes: number = 5, interleaved: boolean = false): Promise<BitplaneConvertResponse> {
    const res = await fetch(`${API_BASE}/bitplane/convert`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ image_bytes, max_bitplanes, interleaved }),
    });
    return res.json();
  },

  async parseCopper(bytes: number[]): Promise<ParseCopperResponse> {
    const res = await fetch(`${API_BASE}/copper/parse`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(bytes),
    });
    return res.json();
  },

  async scaffoldProject(target_directory: string, template: string, project_name: string): Promise<ProjectConfig> {
    const res = await fetch(`${API_BASE}/project/scaffold`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ target_directory, template, project_name }),
    });
    return res.json();
  },

  async detectEmulators(): Promise<EmulatorProfile[]> {
    const res = await fetch(`${API_BASE}/emulator/detect`);
    return res.json();
  },

  async launchEmulator(emulator_type: string, target_file_path: string): Promise<{ success: boolean; message: string }> {
    const res = await fetch(`${API_BASE}/emulator/launch`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ emulator_type, target_file_path }),
    });
    return res.json();
  },

  // LSP Bridge queries
  async lspHover(source: string, line: number, character: number): Promise<IdeHoverResponse | null> {
    const res = await fetch(`${API_BASE}/lsp/hover`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source, line, character }),
    });
    return res.json();
  },

  async lspCompletion(source: string, line: number, character: number): Promise<IdeCompletionItem[]> {
    const res = await fetch(`${API_BASE}/lsp/completion`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source, line, character }),
    });
    return res.json();
  },

  async lspDiagnostics(source: string, cpu: string = '68000'): Promise<IdeDiagnosticItem[]> {
    const res = await fetch(`${API_BASE}/lsp/diagnostics`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify([source, cpu]),
    });
    return res.json();
  },

  async lspFormat(source: string, tab_size: number = 4): Promise<string> {
    const res = await fetch(`${API_BASE}/lsp/format`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify([source, tab_size]),
    });
    return res.json();
  },

  async lspSymbols(source: string): Promise<IdeSymbolItem[]> {
    const res = await fetch(`${API_BASE}/lsp/symbols`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(source),
    });
    return res.json();
  },

  async lspDefinition(source: string, line: number, character: number): Promise<{ line: number; character: number } | null> {
    const res = await fetch(`${API_BASE}/lsp/definition`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ source, line, character }),
    });
    return res.json();
  },
};
