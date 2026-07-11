import { useEffect, useMemo, useState } from "react";
import CodeMirror, { keymap, type Extension } from "@uiw/react-codemirror";
import { StreamLanguage } from "@codemirror/language";
import { lua } from "@codemirror/legacy-modes/mode/lua";
import { linter, lintGutter, type Diagnostic } from "@codemirror/lint";
import { autocompletion, type CompletionContext, type Completion } from "@codemirror/autocomplete";
import { hoverTooltip, type EditorView } from "@codemirror/view";
import { api } from "../api";
import { API_DOCS, explainError } from "../guide/content";

/** The LocalFlow Lua API, offered as autocomplete suggestions. */
const API_COMPLETIONS: Completion[] = [
  ...API_DOCS.map((doc) => ({
    label: doc.name,
    type: doc.name.startsWith("ctx.") ? "property" : "function",
    detail: doc.signature.slice(doc.name.length),
    info: doc.summary.replace(/`/g, ""),
  })),
  {
    label: "automation",
    type: "keyword",
    detail: "{ ... }",
    apply: 'automation {\n    name = "",\n\n    run = function(ctx)\n        \n    end\n}',
  },
];

/** Show the reference entry when hovering over an API name like `fs.move`. */
const apiHover = hoverTooltip((view, pos) => {
  const line = view.state.doc.lineAt(pos);
  const re = /[A-Za-z_][\w.]*/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(line.text))) {
    const from = line.from + match.index;
    const to = from + match[0].length;
    if (pos < from || pos > to) continue;
    const doc = API_DOCS.find((d) => d.name === match![0]);
    if (!doc) return null;
    return {
      pos: from,
      end: to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "api-tooltip";
        const title = document.createElement("code");
        title.textContent = doc.signature;
        const body = document.createElement("div");
        body.textContent = doc.summary.replace(/`/g, "");
        dom.append(title, body);
        if (doc.returns) {
          const returns = document.createElement("div");
          returns.className = "muted";
          returns.textContent = "Returns " + doc.returns.replace(/`/g, "");
          dom.append(returns);
        }
        return { dom };
      },
    };
  }
  return null;
});

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
      message: (() => {
        const message = error.replace(/^Lua syntax error: /, "");
        const hint = explainError(message);
        return hint ? `${message}

💡 ${hint.replace(/`/g, "")}` : message;
      })(),
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
  /** Receives the editor so others can insert text at the cursor. */
  onReady?: (view: EditorView) => void;
}

export default function CodeEditor({ value, onChange, onSave, onTest, onReady }: Props) {
  const dark = usePrefersDark();

  const extensions = useMemo<Extension[]>(
    () => [
      StreamLanguage.define(lua),
      luaLinter,
      apiHover,
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
      onCreateEditor={onReady}
      basicSetup={{ tabSize: 4, foldGutter: false }}
    />
  );
}
