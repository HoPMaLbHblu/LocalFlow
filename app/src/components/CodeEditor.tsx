import { useEffect, useMemo, useState } from "react";
import CodeMirror, { keymap, type Extension } from "@uiw/react-codemirror";
import { StreamLanguage } from "@codemirror/language";
import { lua } from "@codemirror/legacy-modes/mode/lua";
import { linter, lintGutter, type Diagnostic } from "@codemirror/lint";
import { autocompletion, type CompletionContext, type Completion } from "@codemirror/autocomplete";
import { api } from "../api";

/** The LocalFlow Lua API, offered as autocomplete suggestions. */
const API_COMPLETIONS: Completion[] = [
  { label: "fs.list", type: "function", detail: "(path, pattern)", info: "Files in a folder matching a wildcard like \"*.pdf\"." },
  { label: "fs.move", type: "function", detail: "(source, destination)", info: "Move or rename a file. Returns the new path." },
  { label: "fs.copy", type: "function", detail: "(source, destination)", info: "Copy a file. Returns the new path." },
  { label: "fs.exists", type: "function", detail: "(path)", info: "true if the file or folder exists." },
  { label: "fs.delete", type: "function", detail: "(path)", info: "Delete a file or empty folder." },
  { label: "fs.mkdir", type: "function", detail: "(path)", info: "Create a folder and its parents." },
  { label: "fs.basename", type: "function", detail: "(path)", info: "The file name part of a path." },
  { label: "fs.join", type: "function", detail: "(a, b, ...)", info: "Join path parts." },
  { label: "log", type: "function", detail: "(message)", info: "Write a line to the log." },
  { label: "notify", type: "function", detail: "(message)", info: "Show a desktop notification." },
  { label: "print", type: "function", detail: "(...)", info: "Same as log." },
  { label: "ctx.name", type: "property", info: "Name of this automation." },
  { label: "ctx.id", type: "property", info: "Id of this automation." },
  { label: "ctx.trigger", type: "property", info: "\"manual\", \"schedule\" or \"test\"." },
  {
    label: "automation",
    type: "keyword",
    detail: "{ ... }",
    apply: 'automation {\n    name = "",\n\n    run = function(ctx)\n        \n    end\n}',
  },
];

function completeApi(context: CompletionContext) {
  const word = context.matchBefore(/[\w.]+/);
  if (!word || (word.from === word.to && !context.explicit)) return null;
  return { from: word.from, options: API_COMPLETIONS, validFor: /^[\w.]*$/ };
}

/** Ask the backend to compile the code and turn its error into an editor diagnostic. */
const luaLinter = linter(
  async (view) => {
    const code = view.state.doc.toString();
    const error = await api.validateCode(code);
    if (!error) return [];
    const lineNumber = Number(/automation:(\d+):/.exec(error)?.[1] ?? 1);
    const line = view.state.doc.line(Math.min(Math.max(lineNumber, 1), view.state.doc.lines));
    const diagnostic: Diagnostic = {
      from: line.from,
      to: Math.max(line.to, line.from + 1),
      severity: "error",
      message: error.replace(/^Lua syntax error: /, ""),
    };
    return [diagnostic];
  },
  { delay: 400 },
);

function usePrefersDark() {
  const query = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const onChange = () => setDark(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);
  return dark;
}

interface Props {
  value: string;
  onChange: (value: string) => void;
  onSave: () => void;
  onTest: () => void;
}

export default function CodeEditor({ value, onChange, onSave, onTest }: Props) {
  const dark = usePrefersDark();

  const extensions = useMemo<Extension[]>(
    () => [
      StreamLanguage.define(lua),
      luaLinter,
      lintGutter(),
      autocompletion({ override: [completeApi] }),
      keymap.of([
        { key: "Mod-s", preventDefault: true, run: () => (onSave(), true) },
        { key: "Mod-Enter", preventDefault: true, run: () => (onTest(), true) },
      ]),
    ],
    [onSave, onTest],
  );

  return (
    <CodeMirror
      className="code-editor"
      value={value}
      height="100%"
      theme={dark ? "dark" : "light"}
      extensions={extensions}
      onChange={onChange}
      basicSetup={{ tabSize: 4, foldGutter: false }}
    />
  );
}
