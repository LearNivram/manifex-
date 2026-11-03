import Editor from "@monaco-editor/react";
import "./monaco";
import { useResolvedTheme } from "@/hooks/useTheme";
import { useApp } from "@/store/app";

/** Read-only YAML view (used in the detail panel, Helm manifests, …). */
export default function YamlViewer({ value, language = "yaml" }: { value: string; language?: string }) {
  const theme = useResolvedTheme();
  const fontSize = useApp((s) => s.settings?.editorFontSize ?? 13);
  return (
    <Editor
      value={value}
      language={language}
      theme={theme === "dark" ? "manifex-dark" : "manifex-light"}
      options={{
        readOnly: true,
        domReadOnly: true,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        fontFamily: "JetBrains Mono Variable, ui-monospace, monospace",
        fontSize: fontSize - 1,
        lineNumbersMinChars: 3,
        folding: true,
        automaticLayout: true,
        padding: { top: 8 },
        renderLineHighlight: "none",
      }}
    />
  );
}
