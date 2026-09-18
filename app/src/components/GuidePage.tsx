import { useEffect, useMemo, useRef, useState } from "react";
import { apiDocs, lessons, type Block } from "../guide/content";
import { renderInline } from "../guide/GuideText";
import { t } from "../i18n";

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
  const [copied, setCopied] = useState<"no" | "yes" | "failed">("no");
  const copy = async () => {
    try {
      if (!navigator.clipboard) throw new Error("no clipboard");
      await navigator.clipboard.writeText(code);
      setCopied("yes");
    } catch {
      // e.g. the window lost focus: say so instead of pretending it worked.
      setCopied("failed");
    }
    setTimeout(() => setCopied("no"), 2000);
  };
  return (
    <div className="guide-code">
      <pre>{code}</pre>
      <div className="guide-code-actions">
        <button className="link small" onClick={copy}>
          {copied === "yes" ? t("guide.copied") : copied === "failed" ? t("guide.copyFailed") : t("guide.copy")}
        </button>
        {runnable && (
          <button className="small" onClick={() => onTry(title, asAutomation(title, code))}>
            {t("guide.openInEditor")}
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
  const allLessons = useMemo(lessons, []);
  const docs = useMemo(apiDocs, []);
  const [active, setActive] = useState<string>(allLessons[0].id);
  const content = useRef<HTMLDivElement>(null);

  useEffect(() => {
    content.current?.scrollTo({ top: 0 });
  }, [active]);

  const lesson = allLessons.find((l) => l.id === active);
  const index = allLessons.findIndex((l) => l.id === active);

  return (
    <div className="guide">
      <nav className="guide-toc">
        <div className="guide-toc-title">{t("guide.title")}</div>
        {allLessons.map((l) => (
          <button key={l.id} className={active === l.id ? "active" : ""} onClick={() => setActive(l.id)}>
            <span>{l.title}</span>
            <span className="muted small">{l.summary}</span>
          </button>
        ))}
        <button className={active === "reference" ? "active" : ""} onClick={() => setActive("reference")}>
          <span>{t("guide.reference")}</span>
          <span className="muted small">{t("guide.referenceSummary")}</span>
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
                <button className="secondary" onClick={() => setActive(allLessons[index - 1].id)}>
                  ← {allLessons[index - 1].title}
                </button>
              )}
              <button
                className="primary push-right"
                onClick={() => setActive(index < allLessons.length - 1 ? allLessons[index + 1].id : "reference")}
              >
                {index < allLessons.length - 1 ? `${allLessons[index + 1].title} →` : `${t("guide.reference")} →`}
              </button>
            </div>
          </article>
        ) : (
          <article>
            <h1>{t("guide.reference")}</h1>
            <p className="muted">{t("guide.referenceIntro")}</p>
            {docs.map((doc) => (
              <section key={doc.name} className="api-doc">
                <h3>
                  <code>{doc.signature}</code>
                </h3>
                <p>{renderInline(doc.summary)}</p>
                {doc.returns && <p className="muted small">{renderInline(t("help.returns", { value: doc.returns }))}</p>}
                <CodeBlock code={doc.example} title={doc.name} onTry={onTry} />
              </section>
            ))}
          </article>
        )}
      </div>
    </div>
  );
}
