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
}
