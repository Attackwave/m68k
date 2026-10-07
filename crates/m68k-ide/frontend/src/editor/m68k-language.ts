//! Monaco Editor configuration for Motorola 68000 Assembly Language.

import * as monaco from 'monaco-editor';
import { api } from '../api';

export function registerM68kLanguage() {
  monaco.languages.register({ id: 'm68k' });

  // Token provider
  monaco.languages.setMonarchTokensProvider('m68k', {
    defaultToken: '',
    ignoreCase: true,
    tokenPostfix: '.m68k',

    keywords: [
      'ABCD', 'ADD', 'ADDA', 'ADDI', 'ADDQ', 'ADDX', 'AND', 'ANDI', 'ASL', 'ASR',
      'BCC', 'BCS', 'BEQ', 'BGE', 'BGT', 'BHI', 'BLE', 'BLS', 'BLT', 'BMI', 'BNE', 'BPL',
      'BVC', 'BVS', 'BRA', 'BSR', 'BTST', 'BSET', 'BCLR', 'BCHG',
      'CHK', 'CLR', 'CMP', 'CMPA', 'CMPI', 'CMPM', 'DBCC', 'DBCS', 'DBEQ', 'DBF',
      'DBGE', 'DBGT', 'DBHI', 'DBLE', 'DBLS', 'DBLT', 'DBMI', 'DBNE', 'DBPL', 'DBRA',
      'DBT', 'DBVC', 'DBVS', 'DIVS', 'DIVU', 'EOR', 'EORI', 'EXG', 'EXT', 'ILLEGAL',
      'JMP', 'JSR', 'LEA', 'LINK', 'LSL', 'LSR', 'MOVE', 'MOVEA', 'MOVEM', 'MOVEP',
      'MOVEQ', 'MULS', 'MULU', 'NBCD', 'NEG', 'NEGX', 'NOP', 'NOT', 'OR', 'ORI',
      'PEA', 'RESET', 'ROL', 'ROR', 'ROXL', 'ROXR', 'RTE', 'RTR', 'RTS', 'SBCD',
      'SCC', 'SCS', 'SEQ', 'SF', 'SGE', 'SGT', 'SHI', 'SLE', 'SLS', 'SLT', 'SMI',
      'SNE', 'SPL', 'ST', 'STOP', 'SVC', 'SVS', 'SUB', 'SUBA', 'SUBI', 'SUBQ',
      'SUBX', 'SWAP', 'TAS', 'TRAP', 'TRAPV', 'TST', 'UNLK',
      // 68020+
      'BFCHG', 'BFCLR', 'BFEXTS', 'BFEXTU', 'BFFFO', 'BFINS', 'BFSET', 'BFTST',
      'CAS', 'CAS2', 'CHK2', 'CMP2', 'MOVEC', 'MOVES', 'PACK', 'UNPK', 'RTD'
    ],

    directives: [
      'ORG', 'SECTION', 'DC', 'DCB', 'DS', 'EQU', 'SET', 'EVEN', 'ALIGN', 'CNOP',
      'INCLUDE', 'INCBIN', 'MACRO', 'ENDM', 'REPT', 'ENDR', 'IF', 'IFD', 'IFND',
      'ELSE', 'ENDIF', 'RS', 'RSRESET', 'RSSET', 'OPT', 'END', 'FAIL', 'WARNING'
    ],

    registers: [
      'D0', 'D1', 'D2', 'D3', 'D4', 'D5', 'D6', 'D7',
      'A0', 'A1', 'A2', 'A3', 'A4', 'A5', 'A6', 'A7',
      'SP', 'PC', 'SR', 'CCR', 'USP', 'SSP', 'VBR', 'CACR', 'CAAR',
      'FP0', 'FP1', 'FP2', 'FP3', 'FP4', 'FP5', 'FP6', 'FP7',
      'FPCR', 'FPSR', 'FPIAR'
    ],

    tokenizer: {
      root: [
        // Comments
        [/^[;*].*$/, 'comment'],
        [/;.*$/, 'comment'],

        // Directives
        [/\b(ORG|SECTION|DC|DCB|DS|EQU|SET|EVEN|ALIGN|CNOP|INCLUDE|INCBIN|MACRO|ENDM|REPT|ENDR|IF|IFD|IFND|ELSE|ENDIF|RS|RSRESET|RSSET|OPT|END)\b/i, 'keyword.directive'],

        // Registers
        [/\b(D[0-7]|A[0-7]|SP|PC|SR|CCR|USP|SSP|VBR|CACR|FP[0-7]|FPCR|FPSR|FPIAR)\b/i, 'variable.register'],

        // Sizes (.b, .w, .l, .s, .d, .x, .p)
        [/\.[bwlsdxp]\b/i, 'type.size'],

        // Numbers
        [/\$[0-9a-fA-F]+/, 'number.hex'],
        [/%[01]+/, 'number.binary'],
        [/#?[0-9]+/, 'number'],
        [/#[a-zA-Z0-9_$]+/, 'number.immediate'],

        // Strings
        [/"([^"\\]|\\.)*"/, 'string'],
        [/'([^'\\]|\\.)*'/, 'string'],

        // Labels
        [/^[a-zA-Z_.$][a-zA-Z0-9_.$]*:?/, 'entity.name.function.label'],

        // Instructions
        [/[a-zA-Z_][a-zA-Z0-9_]*/, {
          cases: {
            '@keywords': 'keyword',
            '@directives': 'keyword.directive',
            '@registers': 'variable.register',
            '@default': 'identifier'
          }
        }],

        // Delimiters
        [/[,()]/, 'delimiter'],
      ],
    },
  });

  // Language configuration (brackets, comments)
  monaco.languages.setLanguageConfiguration('m68k', {
    comments: {
      lineComment: ';',
    },
    brackets: [
      ['(', ')'],
      ['[', ']'],
      ['{', '}'],
    ],
    autoClosingPairs: [
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
      { open: '\'', close: '\'' },
    ],
  });

  // Hover Provider (via backend m68k-lsp)
  monaco.languages.registerHoverProvider('m68k', {
    provideHover: async (model, position) => {
      const source = model.getValue();
      const hover = await api.lspHover(source, position.lineNumber - 1, position.column - 1);
      if (!hover || !hover.contents) return null;

      return {
        range: new monaco.Range(position.lineNumber, 1, position.lineNumber, model.getLineMaxColumn(position.lineNumber)),
        contents: [{ value: hover.contents }],
      };
    },
  });

  // Completion Provider (via backend m68k-lsp)
  monaco.languages.registerCompletionItemProvider('m68k', {
    triggerCharacters: ['.', '#', '_', ':'],
    provideCompletionItems: async (model, position) => {
      const source = model.getValue();
      const items = await api.lspCompletion(source, position.lineNumber - 1, position.column - 1);

      const suggestions = items.map((item) => ({
        label: item.label,
        kind: monaco.languages.CompletionItemKind.Keyword,
        detail: item.detail,
        documentation: item.documentation ? { value: item.documentation } : undefined,
        insertText: item.insert_text || item.label,
        range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column),
      }));

      return { suggestions };
    },
  });

  // Definition Provider (via backend m68k-lsp)
  monaco.languages.registerDefinitionProvider('m68k', {
    provideDefinition: async (model, position) => {
      const source = model.getValue();
      const def = await api.lspDefinition(source, position.lineNumber - 1, position.column - 1);
      if (!def) return null;

      return {
        uri: model.uri,
        range: new monaco.Range(def.line + 1, def.character + 1, def.line + 1, def.character + 10),
      };
    },
  });

  // Document Formatting Provider (via backend m68k-lsp)
  monaco.languages.registerDocumentFormattingEditProvider('m68k', {
    provideDocumentFormattingEdits: async (model, options) => {
      const source = model.getValue();
      const formatted = await api.lspFormat(source, options.tabSize);
      if (formatted === source) return [];

      return [
        {
          range: model.getFullModelRange(),
          text: formatted,
        },
      ];
    },
  });

  // Register VS Code Dark Studio Theme (m68k-dark)
  monaco.editor.defineTheme('m68k-dark', {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'keyword', foreground: '3B82F6', fontStyle: 'bold' },
      { token: 'keyword.directive', foreground: '818CF8', fontStyle: 'bold' },
      { token: 'variable.register', foreground: 'F59E0B', fontStyle: 'bold' },
      { token: 'type.size', foreground: '38BDF8' },
      { token: 'number.hex', foreground: '34D399' },
      { token: 'number.binary', foreground: '10B981' },
      { token: 'number', foreground: '34D399' },
      { token: 'number.immediate', foreground: 'F472B6' },
      { token: 'string', foreground: 'F472B6' },
      { token: 'comment', foreground: '64748B', fontStyle: 'italic' },
      { token: 'entity.name.function.label', foreground: 'FBBF24', fontStyle: 'bold' },
      { token: 'delimiter', foreground: '94A3B8' },
      { token: 'identifier', foreground: 'E2E8F0' },
    ],
    colors: {
      'editor.background': '#0F1117',
      'editor.foreground': '#E2E8F0',
      'editor.lineHighlightBackground': '#161922',
      'editor.selectionBackground': '#1E3A8A66',
      'editor.inactiveSelectionBackground': '#1E293B55',
      'editorCursor.foreground': '#3B82F6',
      'editorLineNumber.foreground': '#475569',
      'editorLineNumber.activeForeground': '#94A3B8',
      'editorGutter.background': '#0F1117',
      'editorIndentGuide.background': '#1E222D',
      'editorIndentGuide.activeBackground': '#3B82F6',
      'editorWhitespace.foreground': '#2A2F3D',
    },
  });

  // Register Amiga Retro Theme (amiga-theme)
  monaco.editor.defineTheme('amiga-theme', {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'keyword', foreground: 'FF8800', fontStyle: 'bold' },
      { token: 'keyword.directive', foreground: 'FFAA33', fontStyle: 'bold' },
      { token: 'variable.register', foreground: 'FFFFFF', fontStyle: 'bold' },
      { token: 'type.size', foreground: '55AAFF' },
      { token: 'number.hex', foreground: 'AAAAAA' },
      { token: 'number.binary', foreground: '55AAFF' },
      { token: 'number', foreground: 'AAAAAA' },
      { token: 'string', foreground: 'FFCC00' },
      { token: 'comment', foreground: '55AAFF', fontStyle: 'italic' },
      { token: 'entity.name.function.label', foreground: 'FF8800', fontStyle: 'bold' },
      { token: 'delimiter', foreground: 'FFFFFF' },
      { token: 'identifier', foreground: 'FFFFFF' },
    ],
    colors: {
      'editor.background': '#0055AA',
      'editor.foreground': '#FFFFFF',
      'editor.lineHighlightBackground': '#004488',
      'editor.selectionBackground': '#002255',
      'editorCursor.foreground': '#FF8800',
      'editorLineNumber.foreground': '#55AAFF',
      'editorLineNumber.activeForeground': '#FFFFFF',
      'editorGutter.background': '#0055AA',
    },
  });
}
