import { Fragment, type ReactNode } from "react";

/** Render `code` and **bold** inside guide text. Newlines become line breaks. */
export function renderInline(text: string): ReactNode {
  return text.split("\n").map((line, lineIndex) => (
    <Fragment key={lineIndex}>
      {lineIndex > 0 && <br />}
      {line.split(/(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*)/).map((part, i) => {
        if (part.startsWith("`") && part.endsWith("`")) return <code key={i}>{part.slice(1, -1)}</code>;
        if (part.startsWith("**")) return <strong key={i}>{part.slice(2, -2)}</strong>;
        if (part.startsWith("*") && part.length > 2) return <em key={i}>{part.slice(1, -1)}</em>;
        return part;
      })}
    </Fragment>
  ));
}
