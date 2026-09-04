import { useEffect, useRef, useState } from "react";
import { API_DOCS, LESSONS, type Block } from "../guide/content";
import { renderInline } from "../guide/GuideText";

interface Props {
  /** Open a piece of example code as a new, unsaved automation. */
  onTry: (title: string, code: string) => void;
}

/** Wrap bare example code so it runs as an automation. */
function asAutomation(title: string, code: string): string {
  if (code.includes("automation {")) return code;
  const body = code.trimEnd().split("\n").map((line) => (line ? "        " + line : line)).join("\n");
  return `automation {\n    name = ${JSON.stringify(title)},\n\n    run = function(ctx)\n${body}\n    end\n}\n`;
}

function CodeBlock({ code, runnable, title, onTry }: { code: string; runnable?: boolean; title: string; onTry: Props["onTry"] }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="guide-code">
      <pre>{code}</pre>
      <div className="guide-code-actions">
        <button
          className="link small"
          onClick={() => {
            navigator.clipboard?.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
          }}
        >
          {copied ? "Copied" : "Copy"}
        </button>
        {runnable && (
          <button className="small" onClick={() => onTry(title, asAutomation(title, code))}>
            Open in editor
          </button>
        )}
      </div>
    </div>
  );
}

function renderBlock(block: Block, i: number, title: string, onTry: Props["onTry"]) {
  switch (block.kind) {
    case "text":
      return <p key={i}>{renderInline(block.text)}</p>;
    case "code":
      return <CodeBlock key={i} code={block.code} runnable={block.runnable} title={title} onTry={onTry} />;
    case "tip":
      return <div key={i} className="callout tip">💡 {renderInline(block.text)}</div>;
    case "warning":
      return <div key={i} className="callout warning">⚠️ {renderInline(block.text)}</div>;
  }
}

export default function GuidePage({ onTry }: Props) {
  const [active, setActive] = useState<string>(LESSONS[0].id);
  const content = useRef<HTMLDivElement>(null);

  useEffect(() => {
    content.current?.scrollTo({ top: 0 });
  }, [active]);

  const lesson = LESSONS.find((l) => l.id === active);
  const index = LESSONS.findIndex((l) => l.id === active);

  return (
    <div className="guide">
      <nav className="guide-toc">
        <div className="guide-toc-title">Learn LocalFlow</div>
        {LESSONS.map((l) => (
          <button key={l.id} className={active === l.id ? "active" : ""} onClick={() => setActive(l.id)}>
            <span>{l.title}</span>
            <span className="muted small">{l.summary}</span>
          </button>
        ))}
        <button className={active === "reference" ? "active" : ""} onClick={() => setActive("reference")}>
          <span>Function reference</span>
          <span className="muted small">Everything scripts can use.</span>
        </button>
      </nav>

      <div className="guide-content" ref={content}>
        {lesson ? (
          <article>
            <h1>{lesson.title}</h1>
            <p className="muted">{lesson.summary}</p>
            {lesson.blocks.map((b, i) => renderBlock(b, i, lesson.title.replace(/^\d+\.\s*/, ""), onTry))}
            <div className="guide-nav">
              {index > 0 && (
                <button className="secondary" onClick={() => setActive(LESSONS[index - 1].id)}>
                  ← {LESSONS[index - 1].title}
                </button>
              )}
              <button
                className="primary push-right"
                onClick={() => setActive(index < LESSONS.length - 1 ? LESSONS[index + 1].id : "reference")}
              >
                {index < LESSONS.length - 1 ? `${LESSONS[index + 1].title} →` : "Function reference →"}
              </button>
            </div>
          </article>
        ) : (
          <article>
            <h1>Function reference</h1>
            <p className="muted">
              These are the functions LocalFlow adds to Lua. You can also hover over them in the editor, or open the
              Help panel next to the code.
            </p>
            {API_DOCS.map((doc) => (
              <section key={doc.name} className="api-doc">
                <h3>
                  <code>{doc.signature}</code>
                </h3>
                <p>{renderInline(doc.summary)}</p>
                {doc.returns && <p className="muted small">Returns {renderInline(doc.returns)}.</p>}
                <CodeBlock code={doc.example} title={doc.name} onTry={onTry} />
              </section>
            ))}
          </article>
        )}
      </div>
    </div>
  );
}
