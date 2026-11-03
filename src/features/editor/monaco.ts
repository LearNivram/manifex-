// Monaco setup: bundled locally (no CDN, CSP-friendly) with the YAML language
// server running in a web worker.

import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import EditorWorker from "./editor.worker?worker";
import { configureMonacoYaml, type JSONSchema } from "monaco-yaml";
import YamlWorker from "./yaml.worker?worker";

(self as any).MonacoEnvironment = {
  getWorker(_: string, label: string) {
    return label === "yaml" ? new YamlWorker() : new EditorWorker();
  },
};

loader.config({ monaco });

monaco.editor.defineTheme("manifex-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "type", foreground: "8fa1ff" },
    { token: "string.yaml", foreground: "9fd4a3" },
    { token: "number", foreground: "e7a93b" },
    { token: "comment", foreground: "6c6c7a", fontStyle: "italic" },
  ],
  colors: {
    "editor.background": "#0e0f13",
    "editor.lineHighlightBackground": "#16171d",
    "editorLineNumber.foreground": "#3a3b47",
    "editorLineNumber.activeForeground": "#9a9aa8",
    "editorGutter.background": "#0e0f13",
    "editor.selectionBackground": "#2d3570",
    "editorWidget.background": "#1b1c23",
    "editorWidget.border": "#33343f",
    "diffEditor.insertedTextBackground": "#3ccf7a22",
    "diffEditor.removedTextBackground": "#f2656522",
  },
});

monaco.editor.defineTheme("manifex-light", {
  base: "vs",
  inherit: true,
  rules: [
    { token: "type", foreground: "3a47c9" },
    { token: "comment", foreground: "8b8b98", fontStyle: "italic" },
  ],
  colors: {
    "editor.background": "#ffffff",
    "editor.lineHighlightBackground": "#f6f6f8",
    "editorLineNumber.foreground": "#c4c4cc",
    "editorGutter.background": "#ffffff",
  },
});

export const monacoYaml = configureMonacoYaml(monaco, {
  enableSchemaRequest: false,
  validate: true,
  completion: true,
  hover: true,
  isKubernetes: true,
  schemas: [],
});

const schemaByModel = new Map<string, { uri: string; schema: JSONSchema }>();

/** Associates a JSON schema (from the cluster's OpenAPI) with an editor model. */
export function setModelSchema(modelUri: string, schemaKey: string, schema: unknown | null) {
  if (schema) schemaByModel.set(modelUri, { uri: `manifex://schemas/${schemaKey}`, schema: schema as JSONSchema });
  else schemaByModel.delete(modelUri);
  monacoYaml.update({
    schemas: Array.from(schemaByModel.entries()).map(([model, s]) => ({ uri: s.uri, fileMatch: [model], schema: s.schema })),
  });
}

export { monaco };
